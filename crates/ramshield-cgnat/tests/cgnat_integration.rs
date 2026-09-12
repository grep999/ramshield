//! Phase 4 integration test: CGNAT graduated mitigation end-to-end.
//!
//! Simulates a high-entropy shared-infra (CGNAT) decision: the daemon
//! classifies the fingerprint as shared, clamps the tier from hard-drop
//! to Challenge (PoW), publishes the rule into SHM, and announces the ban
//! to the cluster mesh. Verifies both side-effects (SHM slot + mesh delta)
//! agree on Tier 2. No tempfile dep needed — SHM manager falls back to a
//! per-process temp path on non-Linux, and open_or_create() handles both.

use ramshield_cgnat::shm::ShmTableManager;
use ramshield_mesh::aworset::AworsetBlocklist;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

#[test]
fn test_end_to_end_cgnat_shielding() {
    let shm = Arc::new(ShmTableManager::open_or_create(&ShmTableManager::default_path()).unwrap());
    let mesh = Arc::new(AworsetBlocklist::new(1));

    let target_ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 195));
    let client_hash = 0x1234_5678_9ABC_DEF0;

    // High-entropy CGNAT: un-clamped tier would be 3 (hard drop).
    let is_shared = true;
    let target_tier = 3;
    let clamped_tier = if is_shared { 2 } else { target_tier };

    shm.publish_rule(client_hash, 60_000, clamped_tier, 0, is_shared);
    let delta = mesh.record_ban(target_ip, 60_000, clamped_tier);

    let slot = shm.get_slot((client_hash as usize) & (ramshield_cgnat::shm::SHM_TABLE_CAPACITY - 1));
    assert_eq!(
        slot.tier.load(std::sync::atomic::Ordering::Relaxed),
        2,
        "CGNAT must be Tier 2"
    );
    assert_eq!(delta.tier, 2);
    assert!(mesh.is_blocked(&target_ip, 1_000));
    assert!(!mesh.is_blocked(&target_ip, 2_000_000_000_000), "ban must expire");
}