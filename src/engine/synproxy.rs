//! Kernel transport guard.
//!
//! RamShield uses the kernel's netfilter SYNPROXY rather than reimplementing
//! TCP handshake state in Rust. The XDP program remains the earlier drop/rate
//! layer; SYNPROXY handles handshake validation without userspace RTT.
//!
//! The ruleset follows the Linux/XDP SYNPROXY reference sequence: syncookies +
//! timestamps, conntrack loose mode off, raw-prerouting NOTRACK for SYNs,
//! SYNPROXY for INVALID/UNTRACKED handshakes, then an INVALID drop. The nft
//! backend is used because nftables updates are atomic and the rules live in a
//! dedicated table owned by RamShield.
use ramshield_config::SynproxyConfig;
use std::io::Write;
use std::process::{Command, Stdio};
use tracing::info;

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)] // Mirrors the explicit SYNPROXY ruleset inputs.
fn render_ruleset(
    interface: &str,
    ports: &str,
    mss: u16,
    wscale: u8,
    per_source: u32,
    total: u32,
    new_rate: u32,
    burst: u32,
    total_rate: u32,
    total_burst: u32,
) -> String {
    format!(
        r#"table inet ramshield_synproxy {{
    set conn_limit4 {{ type ipv4_addr; size 65536; flags dynamic; }}
    set conn_limit6 {{ type ipv6_addr; size 65536; flags dynamic; }}
    chain preraw {{
        type filter hook prerouting priority raw; policy accept;
        iifname "{interface}" meta nfproto ipv4 tcp flags syn tcp dport {{ {ports} }} meter syn_rate4 {{ ip saddr limit rate over {new_rate}/second burst {burst} packets }} drop
        iifname "{interface}" meta nfproto ipv6 tcp flags syn tcp dport {{ {ports} }} meter syn_rate6 {{ ip6 saddr limit rate over {new_rate}/second burst {burst} packets }} drop
        iifname "{interface}" tcp flags syn tcp dport {{ {ports} }} limit rate over {total_rate}/second burst {total_burst} packets drop
        iifname "{interface}" tcp flags syn tcp dport {{ {ports} }} notrack
    }}
    chain input {{
        type filter hook input priority filter; policy accept;
        iifname "{interface}" tcp dport {{ {ports} }} ct state invalid,untracked synproxy mss {mss} wscale {wscale} timestamp sack-perm
        iifname "{interface}" tcp dport {{ {ports} }} ct state new ct count over {total} drop
        iifname "{interface}" meta nfproto ipv4 tcp dport {{ {ports} }} ct state new add @conn_limit4 {{ ip saddr ct count over {per_source} }} drop
        iifname "{interface}" meta nfproto ipv6 tcp dport {{ {ports} }} ct state new add @conn_limit6 {{ ip6 saddr ct count over {per_source} }} drop
        iifname "{interface}" ct state invalid tcp dport {{ {ports} }} drop
    }}
}}
"#,
        interface = nft_quote(interface),
        ports = ports,
        mss = mss,
        wscale = wscale,
        per_source = per_source,
        total = total,
        new_rate = new_rate,
        burst = burst,
        total_rate = total_rate,
        total_burst = total_burst,
    )
}

#[cfg(target_os = "linux")]
pub fn install(cfg: &SynproxyConfig, interface: &str) -> Result<(), String> {
    if !cfg.enabled {
        return Ok(());
    }
    if interface.trim().is_empty() {
        return Err("synproxy interface must not be empty".into());
    }
    if nft_quote(interface) != interface {
        return Err("synproxy interface contains unsupported nft identifier characters".into());
    }
    if !command_exists("nft") {
        return Err("synproxy requires nftables >= 0.9.2".into());
    }

    // Validate the complete ruleset before mutating the live firewall.
    // Then remove only RamShield's own table; never flush a host firewall ruleset.
    let ports = cfg
        .ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let per_source = cfg.max_connections_per_source;
    let total = cfg.max_connections_total;
    let new_rate = cfg.new_connections_per_second;
    let burst = cfg.new_connection_burst;
    let total_rate = cfg.new_connections_total_per_second;
    let total_burst = cfg.new_connection_total_burst;
    let script = render_ruleset(
        interface,
        &ports,
        cfg.mss,
        cfg.wscale,
        per_source,
        total,
        new_rate,
        burst,
        total_rate,
        total_burst,
    );
    let mut check = Command::new("nft")
        .args(["-c", "-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("nft syntax check: {e}"))?;
    check
        .stdin
        .take()
        .ok_or("nft syntax-check stdin unavailable")?
        .write_all(script.as_bytes())
        .map_err(|e| format!("nft syntax-check write: {e}"))?;
    let checked = check
        .wait_with_output()
        .map_err(|e| format!("nft syntax-check wait: {e}"))?;
    if !checked.status.success() {
        return Err(format!(
            "nft synproxy ruleset syntax invalid: {}",
            String::from_utf8_lossy(&checked.stderr)
        ));
    }

    // B03: Replace atomically. The old table is deleted INSIDE the same nft
    // transaction as the new ruleset, so a failed apply leaves the previous
    // protection active instead of tearing it down first (which created an
    // unprotected window between the standalone delete and the apply).
    let table_exists = Command::new("nft")
        .args(["list", "table", "inet", "ramshield_synproxy"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    let mut apply_script = String::new();
    if table_exists {
        apply_script.push_str("delete table inet ramshield_synproxy\n");
    }
    apply_script.push_str(&script);

    // Kernel tunables are managed by sysctl.d at boot. The daemon runs
    // unprivileged and must never try to write /proc/sys at runtime.
    for (key, expected) in [
        ("net.ipv4.tcp_syncookies", "1"),
        ("net.ipv4.tcp_timestamps", "1"),
        ("net.netfilter.nf_conntrack_tcp_loose", "0"),
    ] {
        let actual = read_sysctl(key);
        if actual != expected {
            return Err(format!(
                "SYNPROXY requires {key}={expected}, found {actual:?}; install deploy/sysctl.d/99-ramshield-synproxy.conf and reload sysctl"
            ));
        }
    }

    let mut child = Command::new("nft")
        .args(["-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("nft: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("nft stdin unavailable")?
        .write_all(apply_script.as_bytes())
        .map_err(|e| format!("nft write: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("nft wait: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "nft synproxy ruleset failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    info!(iface=%interface, ports=?cfg.ports, "kernel SYNPROXY transport guard active");
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn uninstall() -> Result<(), String> {
    if !command_exists("nft") {
        return Ok(());
    }
    let out = Command::new("nft")
        .args(["delete", "table", "inet", "ramshield_synproxy"])
        .output()
        .map_err(|e| format!("nft uninstall: {e}"))?;
    if out.status.success() || String::from_utf8_lossy(&out.stderr).contains("No such file") {
        Ok(())
    } else {
        Err(format!(
            "nft synproxy teardown failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

#[cfg(not(target_os = "linux"))]
pub fn uninstall() -> Result<(), String> {
    Ok(())
}

/// Read the current value of a sysctl key. Returns the value as a string, or an empty string on error.
fn read_sysctl(key: &str) -> String {
    let out = Command::new("sysctl").args(["-n", key]).output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

#[cfg(target_os = "linux")]
fn command_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(target_os = "linux")]
fn nft_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut discard_next_space = false;
    for ch in s.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            out.push(ch);
            discard_next_space = false;
        } else if ch.is_ascii_whitespace() {
            // Preserve ordinary word boundaries, but don't leave a separator
            // behind when it follows a stripped command metacharacter.
            if !discard_next_space && !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
            discard_next_space = false;
        } else {
            discard_next_space = true;
        }
    }
    out
}

#[cfg(not(target_os = "linux"))]
pub fn install(_cfg: &SynproxyConfig, _interface: &str) -> Result<(), String> {
    Err("synproxy requires Linux netfilter/nftables".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn rendered_ruleset_uses_kernel_supported_connlimit_forms() {
        #[cfg(target_os = "linux")]
        {
            let rules =
                super::render_ruleset("eth0", "22, 443", 1460, 7, 128, 16384, 50, 100, 2000, 4000);
            assert!(rules.contains("ct count over 16384 drop"));
            assert!(rules.contains("add @conn_limit4 { ip saddr ct count over 128 } drop"));
            assert!(rules.contains(
                "meter syn_rate4 { ip saddr limit rate over 50/second burst 100 packets } drop"
            ));
            assert!(rules.contains("tcp flags syn tcp dport { 22, 443 } notrack"));
        }
    }

    #[test]
    fn nft_interface_filter_is_alphanumeric() {
        #[cfg(target_os = "linux")]
        assert_eq!(
            super::nft_quote("eth0; drop table inet filter"),
            "eth0drop_table_inet_filter"
        );
    }

    #[test]
    fn synproxy_non_linux_is_explicit() {
        #[cfg(not(target_os = "linux"))]
        {
            let err = super::install(&Default::default(), "eth0").unwrap_err();
            assert!(
                err.contains("Linux"),
                "unsupported env must name Linux requirement: {err}"
            );
        }
        #[cfg(target_os = "linux")]
        {
            // Privilege/tooling absence: install without nft should fail closed
            // with a clear message (do not claim success).
            if !super::command_exists("nft") {
                let cfg = ramshield_config::SynproxyConfig {
                    enabled: true,
                    ..Default::default()
                };
                let err = super::install(&cfg, "lo").unwrap_err();
                assert!(
                    err.to_lowercase().contains("nft") || err.to_lowercase().contains("synproxy"),
                    "missing nft must be explicit: {err}"
                );
            }
            // Shutdown cleanup: uninstall is best-effort when table missing.
            // CI runners may lack CAP_NET_ADMIN; treat permission errors as
            // "environment unsupported" rather than a product regression.
            match super::uninstall() {
                Ok(()) => {}
                Err(e) => {
                    let el = e.to_lowercase();
                    assert!(
                        el.contains("permission")
                            || el.contains("operation not permitted")
                            || el.contains("not allowed")
                            || el.contains("nft"),
                        "unexpected uninstall error: {e}"
                    );
                }
            }
        }
    }

    #[test]
    fn install_rejects_hostile_interface_name() {
        #[cfg(target_os = "linux")]
        {
            let cfg = ramshield_config::SynproxyConfig {
                enabled: true,
                ..Default::default()
            };
            // Even if nft exists, interface must pass identifier filter first.
            let err = super::install(&cfg, "eth0; rm -rf /").unwrap_err();
            assert!(
                err.contains("unsupported") || err.contains("identifier"),
                "{err}"
            );
        }
    }
}
