use super::*;

/// Capability preflight for the XDP path.
///
/// BPF map creation fails with EPERM (surfaced by aya as "failed to create
/// map") when the process lacks CAP_BPF/CAP_NET_ADMIN. File capabilities live
/// on the inode, so every `cargo build` silently drops them — the failure then
/// looks like a map bug. Read CapEff and hand back the exact setcap command.
#[cfg(feature = "xdp")]
fn xdp_capability_hint() -> String {
    const CAP_NET_ADMIN: u64 = 12;
    const CAP_PERFMON: u64 = 38;
    const CAP_BPF: u64 = 39;
    let needed = [
        ("cap_net_admin", CAP_NET_ADMIN),
        ("cap_perfmon", CAP_PERFMON),
        ("cap_bpf", CAP_BPF),
    ];
    let eff = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("CapEff:"))
                .and_then(|l| u64::from_str_radix(l.split_whitespace().nth(1)?, 16).ok())
        })
        .unwrap_or(0);
    let missing: Vec<&str> = needed
        .iter()
        .filter(|(_, bit)| eff & (1u64 << bit) == 0)
        .map(|(name, _)| *name)
        .collect();
    if missing.is_empty() {
        return "capabilities present; check kernel BPF limits".to_string();
    }
    format!(
        "missing {} — re-apply after every build: sudo setcap 'cap_net_admin,cap_perfmon,cap_bpf+eip' target/release/ramshield",
        missing.join(",")
    )
}

// NOTE: Full file restore incomplete in this commit path — see follow-up.
// Placeholder must not remain. This commit is superseded immediately.
pub(crate) async fn boot_pipeline(engine: Arc<Engine>) -> std::io::Result<()> {
    let _ = engine;
    Err(std::io::Error::other("boot.rs must be restored from master; incomplete push"))
}
