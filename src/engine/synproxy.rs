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
use std::process::{Command, Stdio};
use std::io::Write;
use tracing::info;

#[cfg(target_os = "linux")]
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
    format!(r#"table inet ramshield_synproxy {{
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
        interface = nft_quote(interface), ports = ports, mss = mss, wscale = wscale,
        per_source = per_source, total = total, new_rate = new_rate, burst = burst,
        total_rate = total_rate, total_burst = total_burst,
    )
}

#[cfg(target_os = "linux")]
pub fn install(cfg: &SynproxyConfig, interface: &str) -> Result<(), String> {
    if !cfg.enabled { return Ok(()); }
    if interface.trim().is_empty() { return Err("synproxy interface must not be empty".into()); }
    if nft_quote(interface) != interface { return Err("synproxy interface contains unsupported nft identifier characters".into()); }
    if !command_exists("nft") { return Err("synproxy requires nftables >= 0.9.2".into()); }

    let ports = cfg.ports.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
    let script = render_ruleset(
        interface,
        &ports,
        cfg.mss,
        cfg.wscale,
        cfg.max_connections_per_source,
        cfg.max_connections_total,
        cfg.new_connections_per_second,
        cfg.new_connection_burst,
        cfg.new_connections_total_per_second,
        cfg.new_connection_total_burst,
    );

    // Detect whether the owned table exists so replacement and creation can
    // be submitted as one nft transaction; a failed transaction keeps the old
    // ruleset active instead of leaving a protection gap.
    let existing = table_exists()?;
    let transaction = if existing {
        format!("delete table inet ramshield_synproxy\n{script}")
    } else {
        script
    };

    run_nft_script(&transaction, true)?;

    // Read all old values before the first write. If a later sysctl or the
    // firewall transaction fails, restore the values observed at entry.
    let desired = [
        ("net.ipv4.tcp_syncookies", "1"),
        ("net.ipv4.tcp_timestamps", "1"),
        ("net.netfilter.nf_conntrack_tcp_loose", "0"),
    ];
    let previous = apply_sysctls_transactionally(&desired, read_sysctl, run_sysctl)?;

    if let Err(error) = run_nft_script(&transaction, false) {
        let rollback_errors = restore_sysctls(&previous, run_sysctl);
        if rollback_errors.is_empty() {
            return Err(error);
        }
        return Err(format!(
            "{error}; sysctl rollback incomplete: {}",
            rollback_errors.join("; ")
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
        Err(format!("nft synproxy teardown failed: {}", String::from_utf8_lossy(&out.stderr)))
    }
}

#[cfg(not(target_os = "linux"))]
pub fn uninstall() -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_sysctl(key: &str) -> Result<String, String> {
    let out = Command::new("sysctl")
        .args(["-n", key])
        .output()
        .map_err(|e| format!("sysctl read {key}: {e}"))?;
    if !out.status.success() {
        return Err(format!("sysctl read {key} failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
fn run_sysctl(key: &str, value: &str) -> Result<(), String> {
    let out = Command::new("sysctl")
        .args(["-w", &format!("{key}={value}")])
        .output()
        .map_err(|e| format!("sysctl {key}: {e}"))?;
    if !out.status.success() {
        return Err(format!("sysctl {key} failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_sysctls_transactionally(
    desired: &[(&str, &str)],
    mut read: impl FnMut(&str) -> Result<String, String>,
    mut write: impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<Vec<(String, String)>, String> {
    let previous = desired
        .iter()
        .map(|(key, _)| read(key).map(|value| ((*key).to_string(), value)))
        .collect::<Result<Vec<_>, _>>()?;

    for (index, (key, value)) in desired.iter().enumerate() {
        if let Err(error) = write(key, value) {
            let rollback_errors = restore_sysctls(&previous[..index], &mut write);
            return if rollback_errors.is_empty() {
                Err(error)
            } else {
                Err(format!("{error}; sysctl rollback incomplete: {}", rollback_errors.join("; ")))
            };
        }
    }
    Ok(previous)
}

#[cfg(target_os = "linux")]
fn restore_sysctls(
    previous: &[(String, String)],
    mut write: impl FnMut(&str, &str) -> Result<(), String>,
) -> Vec<String> {
    let mut errors = Vec::new();
    for (key, value) in previous.iter().rev() {
        if let Err(error) = write(key, value) {
            errors.push(error);
        }
    }
    errors
}

#[cfg(target_os = "linux")]
fn table_exists() -> Result<bool, String> {
    let out = Command::new("nft")
        .args(["list", "table", "inet", "ramshield_synproxy"])
        .output()
        .map_err(|e| format!("nft table preflight: {e}"))?;
    if out.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("No such file or directory")
        || stderr.contains("No such table")
        || stderr.contains("does not exist")
    {
        return Ok(false);
    }
    Err(format!("nft table preflight failed: {stderr}"))
}

#[cfg(target_os = "linux")]
fn run_nft_script(script: &str, validate_only: bool) -> Result<(), String> {
    let args: &[&str] = if validate_only { &["-c", "-f", "-"] } else { &["-f", "-"] };
    let mut child = Command::new("nft")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("nft {}: {e}", if validate_only { "syntax check" } else { "apply" }))?;
    child.stdin.take()
        .ok_or("nft stdin unavailable")?
        .write_all(script.as_bytes())
        .map_err(|e| format!("nft script write: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("nft wait: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "nft {} failed: {}",
            if validate_only { "synproxy ruleset syntax check" } else { "synproxy ruleset apply" },
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn command_exists(name: &str) -> bool {
    Command::new(name).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

#[cfg(target_os = "linux")]
fn nft_quote(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == '.').collect()
}

#[cfg(not(target_os = "linux"))]
pub fn install(_cfg: &SynproxyConfig, _interface: &str) -> Result<(), String> {
    Err("synproxy requires Linux netfilter/nftables".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn rendered_ruleset_uses_kernel_supported_connlimit_forms() {
        #[cfg(target_os="linux")]
        {
            let rules = super::render_ruleset("eth0", "22, 443", 1460, 7, 128, 16384, 50, 100, 2000, 4000);
            assert!(rules.contains("ct count over 16384 drop"));
            assert!(rules.contains("add @conn_limit4 { ip saddr ct count over 128 } drop"));
            assert!(rules.contains("meter syn_rate4 { ip saddr limit rate over 50/second burst 100 packets } drop"));
            assert!(rules.contains("tcp flags syn tcp dport { 22, 443 } notrack"));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sysctl_update_failure_rolls_back_prior_values() {
        use std::collections::HashMap;

        let mut values = HashMap::from([
            ("a".to_string(), "old-a".to_string()),
            ("b".to_string(), "old-b".to_string()),
            ("c".to_string(), "old-c".to_string()),
        ]);
        let desired = [("a", "new-a"), ("b", "new-b"), ("c", "new-c")];
        let result = super::apply_sysctls_transactionally(
            &desired,
            |key| values.get(key).cloned().ok_or_else(|| format!("missing {key}")),
            |key, value| {
                if key == "b" && value == "new-b" {
                    return Err("injected write failure".into());
                }
                values.insert(key.to_string(), value.to_string());
                Ok(())
            },
        );

        assert!(result.is_err());
        assert_eq!(values.get("a").map(String::as_str), Some("old-a"));
        assert_eq!(values.get("b").map(String::as_str), Some("old-b"));
        assert_eq!(values.get("c").map(String::as_str), Some("old-c"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sysctl_snapshot_is_complete_before_any_mutation() {
        let mut reads = Vec::new();
        let mut writes = Vec::new();
        let desired = [("a", "1"), ("b", "0")];
        let result = super::apply_sysctls_transactionally(
            &desired,
            |key| {
                reads.push(key.to_string());
                Ok(format!("original-{key}"))
            },
            |key, value| {
                writes.push((key.to_string(), value.to_string()));
                Ok(())
            },
        ).expect("all injected sysctl operations succeed");

        assert_eq!(reads, vec!["a", "b"]);
        assert_eq!(writes, vec![("a".into(), "1".into()), ("b".into(), "0".into())]);
        assert_eq!(result, vec![("a".into(), "original-a".into()), ("b".into(), "original-b".into())]);
    }

    #[test]
    fn nft_interface_filter_is_alphanumeric() {
        #[cfg(target_os="linux")]
        assert_eq!(super::nft_quote("eth0; drop table inet filter"), "eth0drop_table_inet_filter");
    }
}
