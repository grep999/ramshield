//! Read-only host and configuration diagnostics for ramshield doctor.
//!
//! The doctor never attaches/detaches XDP, mounts bpffs, creates pins, or mutates
//! firewall state. Checks that require a running daemon or kernel map handles
//! are reported as unobservable rather than guessed healthy.

use crate::Config;
use std::io::IsTerminal;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const MIN_KERNEL: (u32, u32) = (5, 15);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const BPFFS: &str = "/sys/fs/bpf";
const RAMSHIELD_PIN_DIR: &str = "/sys/fs/bpf/ramshield";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckStatus { Pass, Warning, Fail }

#[derive(Clone, Debug)]
pub struct CheckResult {
    pub subsystem: &'static str,
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
    pub remediation: Option<String>,
}

impl CheckResult {
    fn pass(subsystem: &'static str, name: impl Into<String>, message: impl Into<String>) -> Self {
        Self { subsystem, name: name.into(), status: CheckStatus::Pass, message: message.into(), remediation: None }
    }
    fn warning(subsystem: &'static str, name: impl Into<String>, message: impl Into<String>, remediation: impl Into<String>) -> Self {
        Self { subsystem, name: name.into(), status: CheckStatus::Warning, message: message.into(), remediation: Some(remediation.into()) }
    }
    fn fail(subsystem: &'static str, name: impl Into<String>, message: impl Into<String>, remediation: impl Into<String>) -> Self {
        Self { subsystem, name: name.into(), status: CheckStatus::Fail, message: message.into(), remediation: Some(remediation.into()) }
    }
}

#[derive(Debug)]
pub struct DoctorReport { pub checks: Vec<CheckResult> }

impl DoctorReport {
    pub fn exit_code(&self) -> i32 {
        if self.checks.iter().any(|check| check.status == CheckStatus::Fail) { 1 } else { 0 }
    }
    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|check| check.status == CheckStatus::Fail)
    }
}

pub fn load_config(path: &Path) -> anyhow::Result<Config> {
    let path = path.to_str().ok_or_else(|| anyhow::anyhow!("config path is not valid UTF-8"))?;
    Config::load(path)
}

/// Run every check or one named subsystem. Names: core, config, xdp, maps, stats, mesh.
pub fn diagnose(config_path: Option<&Path>, subsystem: Option<&str>) -> DoctorReport {
    let loaded = config_path.map(load_config).unwrap_or_else(|| {
        let mut config = Config::default();
        config.apply_env_overrides()?;
        config.validate()?;
        Ok(config)
    });
    let config_result = match loaded {
        Ok(config) => CheckResult::pass("config", "Configuration", format!(
            "loaded and schema-validated {}",
            config_path.map(|p| p.display().to_string()).unwrap_or_else(|| "environment/default configuration".into())
        )),
        Err(error) => CheckResult::fail("config", "Configuration", format!("configuration could not be loaded or validated: {error:#}"),
            "Fix the reported TOML/schema error and rerun ramshield doctor --subsystem config."),
    };
    let config = match config_path {
        Some(path) => load_config(path).ok(),
        None => {
            let mut config = Config::default();
            config.apply_env_overrides().and_then(|_| config.validate().map(|_| config)).ok()
        }
    };
    let effective = config.clone().unwrap_or_default();
    let mut all = Vec::new();
    all.push(config_result);
    all.extend(check_privileges_and_kernel(&effective));
    all.extend(check_config_and_schema(config_path, config.as_ref()));
    all.extend(check_xdp_and_interface(&effective));
    all.extend(check_ebpf_maps(&effective));
    all.extend(check_statistical_brains(&effective));
    all.extend(check_federation_mesh(&effective));

    let checks = if let Some(filter) = subsystem {
        match normalize_subsystem(filter) {
            Some(filter) => all.into_iter().filter(|check| check.subsystem == filter).collect(),
            None => vec![CheckResult::fail("core", "Subsystem selector",
                format!("unknown subsystem '{subsystem}'"),
                "Choose one of: core, config, xdp, maps, stats, mesh.")],
        }
    } else {
        all
    };
    DoctorReport { checks }
}

fn normalize_subsystem(value: &str) -> Option<&'static str> {
    match value {
        "core" | "kernel" | "privileges" => Some("core"),
        "config" => Some("config"),
        "xdp" | "interface" => Some("xdp"),
        "maps" | "ebpf" | "bpf" => Some("maps"),
        "stats" | "statistics" | "detection" => Some("stats"),
        "mesh" | "federation" => Some("mesh"),
        _ => None,
    }
}

pub fn check_privileges_and_kernel(config: &Config) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let xdp_required = config.xdp.enabled;
    let uid = effective_uid();
    let caps = effective_capabilities();
    let cap_bpf = caps & (1u64 << 39) != 0;
    let cap_sys_admin = caps & (1u64 << 21) != 0;
    let cap_net_admin = caps & (1u64 << 12) != 0;
    if uid == Some(0) {
        out.push(CheckResult::pass("core", "Privileges", "effective UID is 0 (root)"));
    } else if cap_bpf && cap_net_admin {
        out.push(CheckResult::pass("core", "Privileges", "CAP_BPF and CAP_NET_ADMIN are effective"));
    } else if xdp_required {
        out.push(CheckResult::fail("core", "Privileges",
            format!("not root and required capabilities are missing (CAP_BPF={cap_bpf}, CAP_NET_ADMIN={cap_net_admin}, CAP_SYS_ADMIN={cap_sys_admin})"),
            "Run sudo ramshield doctor or grant the minimum capabilities required by the installed kernel and XDP mode."));
    } else {
        out.push(CheckResult::warning("core", "Privileges", "not running as root; acceptable for a non-XDP userspace configuration",
            "Use sudo or configure CAP_BPF/CAP_NET_ADMIN before enabling XDP."));
    }
    match kernel_release() {
        Some((major, minor, release)) => {
            if (major, minor) >= MIN_KERNEL {
                out.push(CheckResult::pass("core", "Kernel version", format!("Linux {release} meets the 5.15 diagnostic minimum")));
            } else {
                if xdp_required {
                    out.push(CheckResult::fail("core", "Kernel version", format!("Linux {release} is older than the diagnostic minimum 5.15"),
                        "Upgrade to Linux 5.15 or newer for the supported XDP deployment baseline."));
                } else {
                    out.push(CheckResult::warning("core", "Kernel version", format!("Linux {release} is older than the diagnostic minimum 5.15"),
                        "Upgrade to Linux 5.15 or newer for the supported XDP deployment baseline."));
                }
            }
        }
        None => out.push(CheckResult::warning("core", "Kernel version", "could not parse /proc/sys/kernel/osrelease",
            "Run doctor on the Linux host where RamShield will run.")),
    }
    if Path::new("/sys/kernel/btf/vmlinux").is_file() {
        out.push(CheckResult::pass("core", "Kernel BTF", "/sys/kernel/btf/vmlinux is present"));
    } else {
        if xdp_required {
            out.push(CheckResult::fail("core", "Kernel BTF", "kernel BTF file is absent",
                "Use a kernel built with BTF support and verify /sys/kernel/btf/vmlinux is readable."));
        } else {
            out.push(CheckResult::warning("core", "Kernel BTF", "kernel BTF file is absent",
                "Use a kernel built with BTF support and verify /sys/kernel/btf/vmlinux is readable."));
        }
    }
    out
}

pub fn check_config_and_schema(path: Option<&Path>, config: Option<&Config>) -> Vec<CheckResult> {
    if let Some(config) = config {
        match config.validate() {
            Ok(()) => vec![CheckResult::pass("config", "Schema compliance", "configuration passed RamShield schema and boundary validation")],
            Err(error) => vec![CheckResult::fail("config", "Schema compliance", error.to_string(),
                "Correct the named configuration field; use ramshield --validate --config <path> to isolate configuration errors.")],
        }
    } else {
        vec![CheckResult::fail("config", "Schema compliance",
            format!("no valid config available{}", path.map(|p| format!(" at {}", p.display())).unwrap_or_default()),
            "Pass --config <path>, set RAMSHIELD_CONFIG, or install /etc/ramshield/ramshield.toml.")]
    }
}

pub fn check_xdp_and_interface(config: &Config) -> Vec<CheckResult> {
    let mut out = Vec::new();
    if config.xdp.interface.trim().is_empty() {
        return vec![CheckResult::fail("xdp", "Network interface", "xdp.interface is empty", "Set xdp.interface to an existing Linux network interface.")];
    }
    let sysfs = PathBuf::from("/sys/class/net").join(&config.xdp.interface);
    if !sysfs.exists() {
        if config.xdp.enabled {
            out.push(CheckResult::fail("xdp", "Network interface", format!("interface '{}' not found under /sys/class/net", config.xdp.interface),
                "Set xdp.interface to the actual ingress interface and rerun doctor."));
        } else {
            out.push(CheckResult::warning("xdp", "Network interface", format!("interface '{}' not found under /sys/class/net", config.xdp.interface),
                "Set xdp.interface to the actual ingress interface and rerun doctor."));
        }
        return out;
    }
    out.push(CheckResult::pass("xdp", "Network interface", format!("interface '{}' exists", config.xdp.interface)));
    if !config.xdp.enabled {
        out.push(CheckResult::warning("xdp", "XDP attachment", "XDP is disabled in configuration; kernel dataplane enforcement is inactive",
            "Enable xdp.enabled=true only after validating interface, privileges, and fallback policy."));
        return out;
    }
    match std::process::Command::new("ip").args(["-details", "link", "show", "dev", &config.xdp.interface]).output() {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
            if text.contains("prog/xdp") || text.contains("xdp id") || text.contains("xdpdrv") || text.contains("xdpgeneric") {
                out.push(CheckResult::pass("xdp", "XDP attachment", format!("iproute2 reports XDP metadata for {}", config.xdp.interface)));
            } else {
                out.push(CheckResult::warning("xdp", "XDP attachment", "no active XDP program was identified; doctor does not attempt an attach",
                    "Start RamShield with its configured XDP feature and inspect ip -details link show dev <interface>."));
            }
        }
        _ => out.push(CheckResult::warning("xdp", "XDP attachment", "ip is unavailable or could not inspect link details",
            "Install iproute2; this check is read-only and does not attach an XDP program.")),
    }
    if config.xdp.mode == "drv" || config.xdp.mode == "native" {
        out.push(CheckResult::warning("xdp", "Driver-mode support",
            "interface presence does not prove native XDP attach support; a safe attach capability probe is not available offline",
            "Qualify drv/native mode on this host; select skb only if generic-mode fallback is acceptable."));
    } else if config.xdp.mode == "skb" {
        out.push(CheckResult::pass("xdp", "Configured XDP mode", "generic/SKB mode selected; native driver support is not required"));
    } else {
        out.push(CheckResult::fail("xdp", "Configured XDP mode", format!("unsupported mode '{}'", config.xdp.mode),
            "Set xdp.mode to skb, drv, or native."));
    }
    out
}

pub fn check_ebpf_maps(config: &Config) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let mounted = std::fs::read_to_string("/proc/mounts")
        .map(|mounts| mounts.lines().any(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            fields.len() >= 3 && fields[0] == "bpf" && fields[1] == BPFFS && fields[2] == "bpf"
        })).unwrap_or(false);
    if mounted {
        out.push(CheckResult::pass("maps", "BPF filesystem", format!("bpffs is mounted at {BPFFS}")));
    } else {
        if config.xdp.enabled {
            out.push(CheckResult::fail("maps", "BPF filesystem", format!("bpffs does not appear mounted at {BPFFS}"),
                "Inspect mount output; if required, mount bpffs using the host's supported systemd/fstab setup."));
        } else {
            out.push(CheckResult::warning("maps", "BPF filesystem", format!("bpffs does not appear mounted at {BPFFS}"),
                "Inspect mount output; if required, mount bpffs using the host's supported systemd/fstab setup."));
        }
    }
    if Path::new(RAMSHIELD_PIN_DIR).is_dir() {
        out.push(CheckResult::pass("maps", "RamShield pin directory", format!("{RAMSHIELD_PIN_DIR} exists")));
    } else {
        out.push(CheckResult::warning("maps", "RamShield pin directory", format!("{RAMSHIELD_PIN_DIR} is absent; no pinned RamShield maps found at the conventional path"),
            "Verify the configured pin path if this deployment uses pinned maps. Doctor intentionally does not create pins."));
    }
    match std::process::Command::new("bpftool").args(["-j", "map", "show"]).output() {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            let count = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v.as_array().map(Vec::len));
            let suffix = count.map(|n| format!(" ({n} maps)").to_string()).unwrap_or_default();
            out.push(CheckResult::warning("maps", "BPF map inventory",
                format!("bpftool enumerated kernel maps{suffix}; occupancy and per-map capacity cannot be inferred from inventory alone"),
                "Inspect RamShield's pinned map handles and max_entries through bpftool or expose authoritative map telemetry."));
        }
        _ => out.push(CheckResult::warning("maps", "BPF map inventory", "bpftool unavailable or kernel map enumeration denied; map capacity and occupancy were not verified",
            "Install bpftool and run doctor with privileges sufficient to inspect BPF maps.")),
    }
    out
}

pub fn check_statistical_brains(config: &Config) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let f = &config.forecasting;
    if f.enabled && f.ewma_alpha.is_finite() && f.ewma_alpha > 0.0 && f.ewma_alpha <= 1.0 {
        out.push(CheckResult::pass("stats", "EWMA configuration", format!("forecasting enabled with ewma_alpha={}", f.ewma_alpha)));
    } else if !f.enabled {
        out.push(CheckResult::warning("stats", "EWMA configuration", "forecasting is disabled; this does not disable the separate rate detection path",
            "Enable forecasting only if its trend/anomaly signal is required for this deployment."));
    } else {
        out.push(CheckResult::fail("stats", "EWMA configuration", format!("ewma_alpha={} is outside (0, 1]", f.ewma_alpha),
            "Set forecasting.ewma_alpha to a finite value greater than 0 and at most 1."));
    }
    if config.detection.rps_threshold > 0 && config.detection.rate_window_secs > 0
        && config.detection.batch_window_ms > 0 && config.detection.pre_aggs_flush_interval_ms > 0 {
        out.push(CheckResult::pass("stats", "Rate/CUSUM prerequisites", format!(
            "rate_threshold={}, rate_window={}s, batch_window={}ms, pre-aggregation flush={}ms",
            config.detection.rps_threshold, config.detection.rate_window_secs,
            config.detection.batch_window_ms, config.detection.pre_aggs_flush_interval_ms)));
    } else {
        out.push(CheckResult::fail("stats", "Rate/CUSUM prerequisites", "one or more detection thresholds or tick intervals are zero",
            "Set positive detection.rps_threshold, rate_window_secs, batch_window_ms, and pre_aggs_flush_interval_ms."));
    }
    out.push(CheckResult::warning("stats", "Live detector tick state",
        "offline configuration checks cannot prove EWMA/CUSUM worker liveness or live sample state",
        "Expose worker heartbeat and detector-state telemetry from the running daemon for authoritative liveness checks."));
    out
}

pub fn check_federation_mesh(config: &Config) -> Vec<CheckResult> {
    if !config.mesh.enabled {
        return vec![CheckResult::warning("mesh", "Federation mesh", "mesh.enabled=false; no federation peers are configured for production sync",
            "Enable and qualify mesh only when multi-node replication is part of this deployment.")];
    }
    if config.mesh.peers.is_empty() {
        return vec![CheckResult::warning("mesh", "Federation peers", "mesh is enabled but the peer list is empty",
            "Configure mesh.peers, peer_node_ids, and peer_auth_keys together.")];
    }
    let mut out = Vec::new();
    for peer in &config.mesh.peers {
        let addr = match peer.parse::<SocketAddr>() {
            Ok(addr) => addr,
            Err(error) => {
                out.push(CheckResult::fail("mesh", format!("Peer {peer}"), format!("invalid socket address: {error}"),
                    "Use an explicit socket address such as 192.0.2.10:7900 or [2001:db8::1]:7900."));
                continue;
            }
        };
        let start = Instant::now();
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => {
                let elapsed = start.elapsed();
                drop(stream);
                out.push(CheckResult::warning("mesh", format!("Peer {peer}"),
                    format!("TCP connect succeeded in {:.2} ms; this proves reachability only, not authenticated gossip or convergence", elapsed.as_secs_f64() * 1000.0),
                    "Verify the peer's RamShield mesh protocol, authentication, and convergence acknowledgements."));
            }
            Err(error) => out.push(CheckResult::fail("mesh", format!("Peer {peer}"), format!("TCP connect failed: {error}"),
                "Check peer address, routing/firewall rules, listener status, and mesh authentication configuration.")),
        }
    }
    out
}

pub fn render(report: &DoctorReport) {
    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    for check in &report.checks {
        let (symbol, code) = match check.status {
            CheckStatus::Pass => ("[✓]", "\x1b[32m"),
            CheckStatus::Warning => ("[!]", "\x1b[33m"),
            CheckStatus::Fail => ("[✗]", "\x1b[31m"),
        };
        if color { println!("{code}{symbol}\x1b[0m {}: {}", check.name, check.message); }
        else { println!("{symbol} {}: {}", check.name, check.message); }
        if let Some(hint) = &check.remediation { println!("    hint: {hint}"); }
    }
    let failures = report.checks.iter().filter(|c| c.status == CheckStatus::Fail).count();
    let warnings = report.checks.iter().filter(|c| c.status == CheckStatus::Warning).count();
    println!("\nDoctor summary: {} checks, {failures} failure(s), {warnings} warning(s).", report.checks.len());
    if failures == 0 { println!("No critical diagnostic failures detected; this is not a live-traffic qualification."); }
}

pub fn run(config_path: Option<&Path>, subsystem: Option<&str>) -> i32 {
    let report = diagnose(config_path, subsystem);
    render(&report);
    report.exit_code()
}

fn effective_uid() -> Option<u32> {
    #[cfg(target_os = "linux")]
    { Some(unsafe { libc::geteuid() }) }
    #[cfg(not(target_os = "linux"))]
    { None }
}

fn effective_capabilities() -> u64 {
    std::fs::read_to_string("/proc/self/status").ok().and_then(|status| {
        status.lines().find(|line| line.starts_with("CapEff:"))
            .and_then(|line| u64::from_str_radix(line.split_whitespace().nth(1)?, 16).ok())
    }).unwrap_or(0)
}

fn kernel_release() -> Option<(u32, u32, String)> {
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok()?;
    let release = release.trim().to_string();
    let mut parts = release.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()?;
    Some((major, minor, release))
}
