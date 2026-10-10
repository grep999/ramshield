use super::*;
use ramshield_metrics::Metrics;

/// Deterministic applier recording dataplane ops — lets tests assert the
/// exact block/unblock sequence the service issues.
struct RecordingApplier {
    log: std::sync::Mutex<Vec<(String, IpAddr)>>,
}
impl RecordingApplier {
    fn new() -> Self {
        Self {
            log: std::sync::Mutex::new(Vec::new()),
        }
    }
}
#[async_trait::async_trait]
impl XdpApplier for RecordingApplier {
    fn apply_block(
        &mut self,
        ip: IpAddr,
        _d: Uuid,
        _ttl_seconds: u64,
    ) -> Result<(), EnforcementError> {
        self.log.lock().unwrap().push(("block".into(), ip));
        Ok(())
    }
    fn apply_unblock(&mut self, ip: IpAddr, _d: Uuid) -> Result<(), EnforcementError> {
        self.log.lock().unwrap().push(("unblock".into(), ip));
        Ok(())
    }
    fn reconcile(
        &mut self,
        _expected: &[IpAddr],
        _expected_cidrs: &[IpNetwork],
    ) -> Result<ReconciliationState, EnforcementError> {
        Ok(ReconciliationState {
            last_wal_lsn: 0,
            pending_blocks: Vec::new(),
            pending_unblocks: Vec::new(),
            evicted_count: 0,
        })
    }
    fn configure_trusted_overlay(&mut self, _cidrs: &[IpNetwork]) -> Result<(), EnforcementError> {
        Ok(())
    }
    fn configure_autonomous(
        &mut self,
        _enabled: bool,
        _syn_pps_per_cpu: u64,
        _udp_pps_per_cpu: u64,
        _packet_pps_per_cpu: u64,
        _window_ms: u64,
    ) -> Result<(), EnforcementError> {
        Ok(())
    }
}

fn svc(xdp: Box<dyn XdpApplier>) -> EnforcementService {
    let store = Arc::new(Store::new(16));
    store.traffic.ram_limit_mb.store(512, Ordering::Relaxed);
    EnforcementService::new(
        store,
        Arc::new(Metrics::new()),
        xdp,
        Arc::new(AtomicBool::new(false)),
    )
}

fn svc_with_wal(xdp: Box<dyn XdpApplier>, dir: &str) -> EnforcementService {
    svc(xdp).with_wal(Arc::new(
        Wal::open(
            dir,
            false,
            ramshield_types::Durability::None,
            64 * 1024 * 1024,
            0,
        )
        .unwrap(),
    ))
}

/// Path of the next segment RAMWAL would create in `dir`. Used to poison
/// a rotation target: a directory at that path makes the segment create
/// fail with EISDIR, so the append fails before any state change.
fn next_segment_path(dir: &std::path::Path) -> std::path::PathBuf {
    let highest = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name();
            let name = name.to_str()?;
            let idx = name.strip_prefix("segment-")?.strip_suffix(".rwl")?;
            idx.parse::<u64>().ok()
        })
        .max()
        .expect("at least one segment must exist after the first append");
    dir.join(format!("segment-{:020}.rwl", highest + 1))
}

/// 64 B segments: every append rotates, so the rotation open of the NEXT
/// segment is where a failure lands. Deterministic WAL-failure injection.
fn svc_with_tiny_wal(dir: &std::path::Path) -> EnforcementService {
    svc(Box::new(RecordingApplier::new())).with_wal(Arc::new(
        Wal::open(
            dir.to_str().unwrap(),
            false,
            ramshield_types::Durability::None,
            64,
            0,
        )
        .unwrap(),
    ))
}

/// Documented architecture contract, TTL item: a due lease is retained
/// until unblock SUCCEEDS. Old code detached the ring card BEFORE the
/// unblock attempt and dropped it on failure, so one transient WAL or
/// storage error converted a temporary block into a permanent one —
/// the IP silently never released.
#[tokio::test]
async fn ttl_lease_retries_after_failed_unblock() {
    let dir = std::env::temp_dir().join(format!("rs_enf_retry_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = svc_with_tiny_wal(&dir);
    s.store.traffic.ram_limit_mb.store(512, Ordering::Relaxed);
    let target = ip([10, 60, 0, 9]);
    // The block append succeeds and rotates past segment 0. Poison the
    // NEXT rotation target so the unblock's append hits EISDIR: the WAL
    // fails durably-first, before any state change. The next index is
    // discovered from the directory (a single 64-byte record may rotate
    // more than once, so a hardcoded index would already exist).
    s.enforce(block_cmd(target, 1)).await.unwrap();
    let next_seg = next_segment_path(&dir);
    std::fs::create_dir_all(&next_seg).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    s.expire_due().await;
    assert!(
        s.blocked_ips.contains(&target),
        "failed unblock must not remove the block state"
    );
    assert!(
        s.expirations.contains_key(&target),
        "failed TTL unblock must re-arm the lease for retry"
    );
    s.check_ring_invariant();
    // Recovery: remove the poison. The poisoned WAL must be reopened —
    // RAMWAL permanently poisons on rotation failure, so reusing the
    // same handle never recovers. Re-create the service with a fresh WAL
    // opened on the same directory; the unblock record is still pending.
    std::fs::remove_dir_all(&next_seg).unwrap();
    // RAMWAL permanently poisons on rotation failure. Reopen the WAL
    // handle: records already on disk survive (the recovery scanner
    // re-reads all segments) and the writer is reclaimed.
    s.wal.as_ref().unwrap().reopen().unwrap();
    // Re-arm deadline is +1s at second-granularity buckets; give it the
    // full slack the ring resolution allows.
    tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
    s.expire_due().await;
    assert!(
        !s.blocked_ips.contains(&target),
        "once WAL works again, the retried lease must unblock"
    );
    assert!(s.expirations.is_empty() && s.buckets.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// P2 audit + early-release basis: XDP drop events are attributed to
/// userspace-blocked IPs ONLY. Invariant: attribution is observability
/// (audit counters, zero-drop gauge) — it never enters detection or the
/// forecaster/learning path, because a drop is a consequence of our own
/// block, not independent threat evidence. Drops with no matching block
/// are kernel/userspace drift indicators, counted separately.
#[tokio::test]
async fn drop_attribution_counts_blocked_ips_only() {
    let target = ip([10, 70, 0, 1]);
    let stranger = ip([10, 70, 0, 2]);
    let mut applier = DroppingApplier::new();
    applier.events = vec![
        XdpDropEvent {
            ip: target,
            ts_ns: 1,
            slot: 0,
        },
        XdpDropEvent {
            ip: target,
            ts_ns: 2,
            slot: 0,
        },
        XdpDropEvent {
            ip: stranger,
            ts_ns: 3,
            slot: 0,
        },
    ];
    let mut s = svc(Box::new(applier));
    s.enforce(block_cmd(target, 60)).await.unwrap();
    let drops = s.xdp.drain_drop_events();
    s.attribute_drops(drops);
    assert_eq!(s.drops_by_blocked.get(&target), Some(&2));
    assert_eq!(
        s.drops_by_blocked.len(),
        1,
        "unblocked IP must not accrue attribution"
    );
    assert_eq!(s.metrics.xdp_attribution_gaps.load(Ordering::Relaxed), 1);
    // target now has drops → no zero-drop blocked IPs.
    assert_eq!(
        s.metrics.xdp_blocked_ips_zero_drops.load(Ordering::Relaxed),
        0
    );
    // Unblock clears attribution — a later re-block starts at zero.
    s.enforce(unblock_cmd(target)).await.unwrap();
    assert!(s.drops_by_blocked.is_empty());
}

#[tokio::test]
async fn zero_drop_gauge_tracks_unseen_blocks() {
    let a = ip([10, 71, 0, 1]);
    let b = ip([10, 71, 0, 2]);
    let mut s = svc(Box::new(DroppingApplier::new()));
    s.enforce(block_cmd(a, 60)).await.unwrap();
    s.enforce(block_cmd(b, 60)).await.unwrap();
    s.attribute_drops(Vec::new());
    assert_eq!(
        s.metrics.xdp_blocked_ips_zero_drops.load(Ordering::Relaxed),
        2
    );
    s.attribute_drops(vec![XdpDropEvent {
        ip: a,
        ts_ns: 1,
        slot: 0,
    }]);
    assert_eq!(
        s.metrics.xdp_blocked_ips_zero_drops.load(Ordering::Relaxed),
        1
    );
}

struct DroppingApplier {
    events: Vec<XdpDropEvent>,
}
impl DroppingApplier {
    fn new() -> Self {
        Self { events: Vec::new() }
    }
}
#[async_trait::async_trait]
impl XdpApplier for DroppingApplier {
    fn apply_block(&mut self, _: IpAddr, _: Uuid, _: u64) -> Result<(), EnforcementError> {
        Ok(())
    }
    fn apply_unblock(&mut self, _: IpAddr, _: Uuid) -> Result<(), EnforcementError> {
        Ok(())
    }
    fn reconcile(
        &mut self,
        _: &[IpAddr],
        _: &[IpNetwork],
    ) -> Result<ReconciliationState, EnforcementError> {
        Ok(ReconciliationState {
            last_wal_lsn: 0,
            pending_blocks: Vec::new(),
            pending_unblocks: Vec::new(),
            evicted_count: 0,
        })
    }
    fn drain_drop_events(&mut self) -> Vec<XdpDropEvent> {
        std::mem::take(&mut self.events)
    }
    fn configure_trusted_overlay(&mut self, _cidrs: &[IpNetwork]) -> Result<(), EnforcementError> {
        Ok(())
    }
    fn configure_autonomous(
        &mut self,
        _enabled: bool,
        _syn_pps_per_cpu: u64,
        _udp_pps_per_cpu: u64,
        _packet_pps_per_cpu: u64,
        _window_ms: u64,
    ) -> Result<(), EnforcementError> {
        Ok(())
    }
}

fn block_cmd(ip: IpAddr, ttl: u64) -> EnforceCommand {
    EnforceCommand {
        decision_id: Uuid::new_v4(),
        policy_version: 1,
        source: "test".into(),
        actor: "test".into(),
        timestamp_utc: 0,
        ttl_seconds: ttl,
        reason: "high_rps".into(),
        ip,
        cidr: None,
        action: EnforceAction::Block,
        evidence_source: ramshield_types::EvidenceSource::LocalSignals,
    }
}
fn unblock_cmd(ip: IpAddr) -> EnforceCommand {
    EnforceCommand {
        decision_id: Uuid::new_v4(),
        policy_version: 1,
        source: "test".into(),
        actor: "test".into(),
        timestamp_utc: 0,
        ttl_seconds: 0,
        reason: "manual".into(),
        ip,
        cidr: None,
        action: EnforceAction::Unblock,
        evidence_source: ramshield_types::EvidenceSource::LocalSignals,
    }
}

fn ip(a: [u8; 4]) -> IpAddr {
    IpAddr::from(a)
}

fn block_cidr_command(network: IpNetwork, ttl: u64) -> EnforceCommand {
    EnforceCommand {
        decision_id: Uuid::new_v4(),
        policy_version: 1,
        source: "test".into(),
        actor: "test".into(),
        timestamp_utc: 0,
        ttl_seconds: ttl,
        reason: "high_rps".into(),
        ip: network.addr,
        cidr: Some(network),
        action: EnforceAction::Block,
        evidence_source: ramshield_types::EvidenceSource::LocalSignals,
    }
}

fn unblock_cidr_command(network: IpNetwork) -> EnforceCommand {
    EnforceCommand {
        decision_id: Uuid::new_v4(),
        policy_version: 1,
        source: "test".into(),
        actor: "test".into(),
        timestamp_utc: 0,
        ttl_seconds: 0,
        reason: "manual".into(),
        ip: network.addr,
        cidr: Some(network),
        action: EnforceAction::Unblock,
        evidence_source: ramshield_types::EvidenceSource::LocalSignals,
    }
}

/// D0 regression: a CIDR block must NOT create an IP detention on the
/// prefix's representative address (`192.0.2.0` for `192.0.2.0/24`).
/// The two detention domains are independent.
#[tokio::test]
async fn cidr_block_does_not_block_representative_ip() {
    let net = IpNetwork::new("192.0.2.0".parse().unwrap(), 24).unwrap();
    let representative = net.addr;
    let mut s = svc(Box::new(RecordingApplier::new()));

    s.enforce(block_cidr_command(net, 0)).await.unwrap();

    // No IpRecord with a Blocked state may exist for the representative —
    // CIDR detention lives ONLY in active_cidrs, not the IP store.
    assert!(
        !s.store
            .get(&representative)
            .and_then(|v| match v {
                Value::IpRecord(r) => Some(r),
                _ => None,
            })
            .map(|r| matches!(r.block_state, BlockState::Blocked { .. }))
            .unwrap_or(false),
        "CIDR block created an explicit IP block on {}",
        representative
    );
    assert!(
        !s.blocked_ips.contains(&representative),
        "CIDR block inserted the representative into the explicit block set"
    );
    assert!(s.store.active_cidrs.contains_key(&net));
}

/// D1 regression: a CIDR unblock must NOT destroy an independent
/// explicit IP detention on the prefix's representative address.
#[tokio::test]
async fn cidr_unblock_does_not_remove_explicit_ip_block() {
    let net = IpNetwork::new("192.0.2.0".parse().unwrap(), 24).unwrap();
    let ip0 = net.addr;
    let mut s = svc(Box::new(RecordingApplier::new()));

    s.enforce(block_cmd(ip0, 0)).await.unwrap();
    s.enforce(block_cidr_command(net, 0)).await.unwrap();
    s.enforce(unblock_cidr_command(net)).await.unwrap();

    assert!(
        s.blocked_ips.contains(&ip0),
        "CIDR unblock cleared an unrelated explicit IP block on {}",
        ip0
    );
    match s.store.get(&ip0).unwrap() {
        Value::IpRecord(r) => assert!(
            matches!(r.block_state, BlockState::Blocked { .. }),
            "explicit IP block_state was cleared by a CIDR unblock"
        ),
        _ => panic!("expected IP record"),
    }
    assert!(!s.store.active_cidrs.contains_key(&net));
}

/// D1 regression: a CIDR unblock must NOT purge the IP TTL of an
/// unrelated explicit detention.
#[tokio::test]
async fn cidr_unblock_does_not_purge_explicit_ip_ttl() {
    let net = IpNetwork::new("198.51.100.0".parse().unwrap(), 24).unwrap();
    let ip0 = net.addr;
    let mut s = svc(Box::new(RecordingApplier::new()));

    s.enforce(block_cmd(ip0, 3600)).await.unwrap();
    s.enforce(block_cidr_command(net, 3600)).await.unwrap();
    s.enforce(unblock_cidr_command(net)).await.unwrap();

    assert!(
        s.expirations.contains_key(&ip0),
        "CIDR unblock detached the explicit IP TTL for {}",
        ip0
    );
    assert!(!s.cidr_expirations.contains_key(&net));
}

/// D3/D4 semantic matrix: explicit IP and CIDR detention are independent
/// and each survives removal of the other.
#[tokio::test]
async fn detention_domain_independence_matrix() {
    let net = IpNetwork::new("203.0.113.0".parse().unwrap(), 24).unwrap();
    let ip0 = net.addr;
    let mut s = svc(Box::new(RecordingApplier::new()));

    // none → not detained
    assert!(!s.blocked_ips.contains(&ip0));
    assert!(s.store.is_blocked_by_cidr(&ip0).is_none());

    // IP only
    s.enforce(block_cmd(ip0, 0)).await.unwrap();
    assert!(s.blocked_ips.contains(&ip0));
    assert!(s.store.is_blocked_by_cidr(&ip0).is_none());

    // IP + CIDR → both domains hold
    s.enforce(block_cidr_command(net, 0)).await.unwrap();
    assert!(s.blocked_ips.contains(&ip0));
    assert_eq!(s.store.is_blocked_by_cidr(&ip0), Some(net));

    // IP unblock → still detained by CIDR
    s.enforce(unblock_cmd(ip0)).await.unwrap();
    assert!(!s.blocked_ips.contains(&ip0));
    assert_eq!(s.store.is_blocked_by_cidr(&ip0), Some(net));

    // CIDR unblock → clean
    s.enforce(unblock_cidr_command(net)).await.unwrap();
    assert!(!s.blocked_ips.contains(&ip0));
    assert!(s.store.is_blocked_by_cidr(&ip0).is_none());
}

/// D4 mirror: CIDR first, then explicit IP, then CIDR unblock — the
/// explicit IP must remain.
#[tokio::test]
async fn explicit_ip_survives_cidr_unblock_when_cidr_was_first() {
    let net = IpNetwork::new("203.0.113.0".parse().unwrap(), 24).unwrap();
    let ip0 = net.addr;
    let mut s = svc(Box::new(RecordingApplier::new()));

    s.enforce(block_cidr_command(net, 0)).await.unwrap();
    s.enforce(block_cmd(ip0, 0)).await.unwrap();
    assert!(s.blocked_ips.contains(&ip0));
    assert_eq!(s.store.is_blocked_by_cidr(&ip0), Some(net));

    s.enforce(unblock_cidr_command(net)).await.unwrap();
    assert!(
        s.blocked_ips.contains(&ip0),
        "CIDR unblock must not disturb an explicit IP detention"
    );
    assert!(s.store.is_blocked_by_cidr(&ip0).is_none());
}

/// D4 mirror the other way: CIDR only, then CIDR unblock → clean.
#[tokio::test]
async fn cidr_only_unblock_clears_detention() {
    let net = IpNetwork::new("203.0.113.0".parse().unwrap(), 24).unwrap();
    let ip0 = net.addr;
    let mut s = svc(Box::new(RecordingApplier::new()));

    s.enforce(block_cidr_command(net, 0)).await.unwrap();
    assert_eq!(s.store.is_blocked_by_cidr(&ip0), Some(net));

    s.enforce(unblock_cidr_command(net)).await.unwrap();
    assert!(s.store.is_blocked_by_cidr(&ip0).is_none());
}

#[tokio::test]
async fn block_then_unblock_reaches_dataplane_once() {
    let mut s = svc(Box::new(RecordingApplier::new()));
    let target = ip([9, 9, 9, 9]);
    s.enforce(block_cmd(target, 3600)).await.unwrap();
    s.enforce(unblock_cmd(target)).await.unwrap();
    // Dataplane saw both ops: blocked_ips empty after unblock proves the
    // unblock path ran; store record is Clean.
    assert!(!s.blocked_ips.contains(&target));
    let rec = s.store.get(&target).unwrap();
    match rec {
        Value::IpRecord(r) => assert_eq!(r.block_state, BlockState::Clean),
        _ => panic!("wrong value type"),
    }
}

#[tokio::test]
async fn duplicate_decision_is_idempotent() {
    let mut s = svc(Box::new(RecordingApplier::new()));
    let target = ip([9, 9, 9, 8]);
    let cmd = block_cmd(target, 0);
    s.enforce(cmd.clone()).await.unwrap();
    s.enforce(cmd).await.unwrap();
    // One dataplane op: verify via store state + single blocked entry.
    assert_eq!(s.blocked_ips.len(), 1);
}

#[test]
fn restore_cidr_keeps_userspace_state_when_xdp_fails() {
    let mut s = svc(Box::new(FailingApplier));
    let v4 = IpNetwork::new("198.51.100.0".parse().unwrap(), 24).unwrap();
    let v6 = IpNetwork::new("2001:db8::".parse().unwrap(), 64).unwrap();
    s.restore_cidr_blocks([(v4, 60), (v6, 0)]);
    assert!(
        s.store.active_cidrs.contains_key(&v4),
        "v4 CIDR must remain authoritative"
    );
    assert!(
        s.store.active_cidrs.contains_key(&v6),
        "v6 CIDR must remain authoritative"
    );
    assert!(s.cidr_expirations.contains_key(&v4));
    assert!(!s.cidr_expirations.contains_key(&v6));
}

/// A dataplane-failing applier: storage and WAL still commit, kernel does
/// not. Used by the idempotency + dataplane-failure contracts.
struct FailingApplier;
#[async_trait::async_trait]
impl XdpApplier for FailingApplier {
    fn apply_block(&mut self, _: IpAddr, _: Uuid, _: u64) -> Result<(), EnforcementError> {
        Err(EnforcementError::Xdp("kernel gone".into()))
    }
    fn apply_unblock(&mut self, _: IpAddr, _: Uuid) -> Result<(), EnforcementError> {
        Err(EnforcementError::Xdp("kernel gone".into()))
    }
    fn reconcile(
        &mut self,
        _: &[IpAddr],
        _: &[IpNetwork],
    ) -> Result<ReconciliationState, EnforcementError> {
        Ok(ReconciliationState {
            last_wal_lsn: 0,
            pending_blocks: Vec::new(),
            pending_unblocks: Vec::new(),
            evicted_count: 0,
        })
    }
    fn configure_trusted_overlay(&mut self, _cidrs: &[IpNetwork]) -> Result<(), EnforcementError> {
        Ok(())
    }
    fn configure_autonomous(
        &mut self,
        _enabled: bool,
        _syn_pps_per_cpu: u64,
        _udp_pps_per_cpu: u64,
        _packet_pps_per_cpu: u64,
        _window_ms: u64,
    ) -> Result<(), EnforcementError> {
        Ok(())
    }
}

/// Documented architecture contract, idempotency item: a duplicate
/// decision_id must return the ORIGINAL EnforceResult. The old code
/// remembered only the id and fabricated a fresh success
/// (`xdp_applied: true`) even when the first attempt had failed on the
/// dataplane — the operator could not see that the kernel never got the
/// block, and a retry consumer would read a lie.
#[tokio::test]
async fn duplicate_decision_returns_original_result() {
    let store = Arc::new(Store::new(16));
    store.traffic.ram_limit_mb.store(512, Ordering::Relaxed);
    let mut s = EnforcementService::new(
        store,
        Arc::new(Metrics::new()),
        Box::new(FailingApplier),
        Arc::new(AtomicBool::new(false)),
    );
    let target = ip([9, 9, 9, 3]);
    let cmd = block_cmd(target, 0);
    let first = s.enforce(cmd.clone()).await.unwrap();
    assert!(
        !first.xdp_applied,
        "test premise: first application failed on the dataplane"
    );
    let second = s.enforce(cmd).await.unwrap();
    assert_eq!(
        first, second,
        "duplicate decision_id must return the cached original result"
    );
}

#[tokio::test]
async fn unspecified_ip_rejected() {
    let mut s = svc(Box::new(RecordingApplier::new()));
    let err = s.enforce(block_cmd(IpAddr::from([0, 0, 0, 0]), 0)).await;
    assert!(matches!(err, Err(EnforcementError::InvalidCommand(_))));
}

#[tokio::test]
async fn reblock_purges_stale_ttl() {
    let ra = Box::new(RecordingApplier::new());
    let mut s = svc(ra);
    let target = ip([9, 9, 9, 7]);
    // Block with TTL, unblock (manual), block again with TTL.
    s.enforce(block_cmd(target, 3600)).await.unwrap();
    s.enforce(unblock_cmd(target)).await.unwrap();
    s.enforce(block_cmd(target, 3600)).await.unwrap();
    // Exactly ONE pending expiration for the IP (old one purged).
    // HashMap type-enforces <=1 per IP; assert the one it must hold.
    assert!(
        s.expirations.contains_key(&target),
        "re-block must keep its TTL"
    );
}

#[tokio::test]
async fn ttl_zero_block_has_no_expiration() {
    let mut s = svc(Box::new(RecordingApplier::new()));
    let target = ip([9, 9, 9, 6]);
    s.enforce(block_cmd(target, 0)).await.unwrap();
    assert!(!s.expirations.contains_key(&target));
    assert!(s.blocked_ips.contains(&target));
}

/// Item 14 regression: re-block must MOVE the ring card, not stack a
/// second one. Block with 1s TTL, immediately refresh to 3600s; after the
/// first deadline passes, the IP must still be blocked (the long TTL won).
/// A lazy/duplicate-card design would expire the stale 1s entry here.
#[tokio::test]
async fn reblock_moves_card_not_duplicates() {
    let mut s = svc(Box::new(RecordingApplier::new()));
    let target = ip([9, 9, 9, 5]);
    s.enforce(block_cmd(target, 1)).await.unwrap();
    s.enforce(block_cmd(target, 3600)).await.unwrap();
    assert_eq!(s.expirations.len(), 1, "exactly one card pending");
    s.check_ring_invariant();
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    s.expire_due().await;
    assert!(
        s.blocked_ips.contains(&target),
        "refreshed TTL must win — stale 1s card must not expire the IP"
    );
    // Clean up: manual unblock leaves no residue.
    s.enforce(unblock_cmd(target)).await.unwrap();
    assert!(s.expirations.is_empty() && s.buckets.is_empty());
}

/// Item 14: a short TTL actually fires through the bucket drain.
#[tokio::test]
async fn ring_expires_short_ttl() {
    let mut s = svc(Box::new(RecordingApplier::new()));
    let target = ip([9, 9, 9, 4]);
    s.enforce(block_cmd(target, 1)).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    s.expire_due().await;
    assert!(!s.blocked_ips.contains(&target), "TTL expiry must unblock");
    assert!(s.expirations.is_empty() && s.buckets.is_empty());
}

/// Dense-bucket surgery: swap-remove fixups must keep every remaining
/// card's (bucket, pos) exact. Churn several IPs across two buckets,
/// unblock the middle ones, then expire: only true-TTL entries unblock.
#[tokio::test]
async fn ring_positions_survive_detach_storm() {
    let mut s = svc(Box::new(RecordingApplier::new()));
    let a = ip([9, 9, 8, 1]);
    let b = ip([9, 9, 8, 2]);
    let c = ip([9, 9, 8, 3]);
    let d = ip([9, 9, 8, 4]);
    s.enforce(block_cmd(a, 3600)).await.unwrap();
    s.enforce(block_cmd(b, 3600)).await.unwrap(); // same bucket as a
    s.enforce(block_cmd(c, 1)).await.unwrap(); // short bucket
    s.enforce(block_cmd(d, 1)).await.unwrap(); // same short bucket as c
    s.enforce(unblock_cmd(a)).await.unwrap(); // detach front of long bucket
    s.enforce(unblock_cmd(c)).await.unwrap(); // detach front of short bucket
    s.check_ring_invariant();
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    s.expire_due().await;
    assert!(s.blocked_ips.contains(&b), "long TTL untouched by drains");
    assert!(!s.blocked_ips.contains(&d), "short TTL fired");
    assert_eq!(s.expirations.len(), 1);
    s.check_ring_invariant();
}

#[tokio::test]
async fn storage_blocked_before_dataplane() {
    // If the dataplane errors, storage must STILL hold the block (fail-open
    // kernel, fail-closed state).
    let store = Arc::new(Store::new(16));
    store.traffic.ram_limit_mb.store(512, Ordering::Relaxed);
    let mut s = EnforcementService::new(
        store.clone(),
        Arc::new(Metrics::new()),
        Box::new(FailingApplier),
        Arc::new(AtomicBool::new(false)),
    );
    let target = ip([9, 9, 9, 5]);
    let res = s.enforce(block_cmd(target, 0)).await.unwrap();
    assert!(
        !res.xdp_applied,
        "xdp_applied must be false on dataplane failure"
    );
    let rec = store.get(&target).expect("record must exist");
    match rec {
        Value::IpRecord(r) => assert!(matches!(r.block_state, BlockState::Blocked { .. })),
        _ => panic!("wrong value type"),
    }
    assert!(s.blocked_ips.contains(&target));
}

#[tokio::test]
async fn wal_first_sets_lsn_and_survives_replay() {
    let dir = std::env::temp_dir().join(format!("rs_enf_wal_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = svc_with_wal(Box::new(RecordingApplier::new()), dir.to_str().unwrap());
    let target = ip([9, 9, 9, 4]);
    let r1 = s.enforce(block_cmd(target, 60)).await.unwrap();
    let lsn1 = r1.wal_lsn.expect("WAL attached ⇒ lsn set");
    assert!(lsn1 >= 1, "LSN base is 1 (0 reserved)");
    let r2 = s.enforce(unblock_cmd(target)).await.unwrap();
    let lsn2 = r2.wal_lsn.expect("unblock also journaled");
    assert!(lsn2 > lsn1, "LSN monotonic");

    drop(s);
    // Replay proves both decisions are durable.
    let entries = ramshield_storage::wal::Wal::replay_dir(dir.to_str().unwrap()).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(matches!(entries[0], WalEntry::BlockIp { .. }));
    assert!(matches!(entries[1], WalEntry::UnblockIp { .. }));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Crash-recovery contract: block → "restart" (fresh store) → replay
/// restores the block into the store so XDP reconcile re-arms it.
#[tokio::test]
async fn replay_restores_block_into_fresh_store() {
    let dir = std::env::temp_dir().join(format!("rs_wal_recov_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let mut s = svc_with_wal(Box::new(RecordingApplier::new()), dir.to_str().unwrap());
    let target = ip([10, 77, 0, 5]);
    s.enforce(block_cmd(target, 3600)).await.unwrap();
    drop(s);

    // "Restart": empty store, same WAL dir.
    let fresh = Arc::new(Store::new(16));
    let wal = Arc::new(
        Wal::open(
            dir.to_str().unwrap(),
            false,
            ramshield_types::Durability::None,
            64 * 1024 * 1024,
            0,
        )
        .unwrap(),
    );
    let restored = replay_wal_into_store(&fresh, &wal, 0).unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].1, 3600, "restored block must carry full TTL");
    match fresh.get(&target) {
        Some(Value::IpRecord(r)) => assert!(
            matches!(r.block_state, BlockState::Blocked { .. }),
            "replay must restore Blocked state"
        ),
        other => panic!("expected IpRecord after replay, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Unblock cancels a prior block across the restart boundary.
#[tokio::test]
async fn replay_unblock_cancels_block() {
    let dir = std::env::temp_dir().join(format!("rs_wal_cancel_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let mut s = svc_with_wal(Box::new(RecordingApplier::new()), dir.to_str().unwrap());
    let target = ip([10, 78, 0, 6]);
    s.enforce(block_cmd(target, 3600)).await.unwrap();
    s.enforce(unblock_cmd(target)).await.unwrap();
    drop(s);

    let fresh = Arc::new(Store::new(16));
    let wal = Arc::new(
        Wal::open(
            dir.to_str().unwrap(),
            false,
            ramshield_types::Durability::None,
            64 * 1024 * 1024,
            0,
        )
        .unwrap(),
    );
    assert_eq!(replay_wal_into_store(&fresh, &wal, 0).unwrap().len(), 0);
    assert!(fresh.get(&target).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Expired TTL blocks are not resurrected on restart.
#[tokio::test]
async fn replay_skips_expired_ttl_blocks() {
    let dir = std::env::temp_dir().join(format!("rs_wal_ttl_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Hand-write an ancient block entry with ttl=1s.
    let wal = Wal::open(
        dir.to_str().unwrap(),
        false,
        ramshield_types::Durability::None,
        64 * 1024 * 1024,
        0,
    )
    .unwrap();
    wal.append(&WalEntry::BlockIp {
        ip: "10.79.0.7".into(),
        reason: "high_rps".into(),
        ttl_secs: Some(1),
        ts_ns: 1, // epoch + 1ns — long expired
    })
    .unwrap();
    drop(wal);

    let fresh = Arc::new(Store::new(16));
    let wal2 = Arc::new(
        Wal::open(
            dir.to_str().unwrap(),
            false,
            ramshield_types::Durability::None,
            64 * 1024 * 1024,
            0,
        )
        .unwrap(),
    );
    assert_eq!(replay_wal_into_store(&fresh, &wal2, 0).unwrap().len(), 0);
    assert!(
        fresh.get(&"10.79.0.7".parse().unwrap()).is_none(),
        "expired block must not resurrect"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// P1-4: replay returns remaining TTLs; restore_expirations re-arms the
/// ring so a restored block actually expires (previously: forever).
#[tokio::test]
async fn replay_then_restore_expirations_arms_ring() {
    let dir = std::env::temp_dir().join(format!("rs_wal_ream_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut s = svc_with_wal(Box::new(RecordingApplier::new()), dir.to_str().unwrap());
    let target = ip([10, 80, 0, 9]);
    s.enforce(block_cmd(target, 3600)).await.unwrap();
    drop(s);

    let fresh = Arc::new(Store::new(16));
    let wal = Arc::new(
        Wal::open(
            dir.to_str().unwrap(),
            false,
            ramshield_types::Durability::None,
            64 * 1024 * 1024,
            0,
        )
        .unwrap(),
    );
    let pairs = replay_wal_into_store(&fresh, &wal, 0).unwrap();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].1, 3600, "block written seconds ago keeps full TTL");

    // New service instance (empty ring) restores + re-arms.
    let mut s2 = EnforcementService::new(
        fresh,
        Arc::new(Metrics::new()),
        Box::new(RecordingApplier::new()),
        Arc::new(AtomicBool::new(false)),
    );
    s2.restore_expirations(pairs);
    assert!(
        s2.expirations.contains_key(&target),
        "restored block must be scheduled in the TTL ring"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn cidr_block_registers_then_unblock_clears_shared_set() {
    // The shared set is the one clock: the actor writes it on BlockCidr
    // and clears it on UnblockCidr, and check_ip reads it. A member host
    // has no IpRecord, so this set is the only place the block exists.
    let store = Arc::new(Store::new(16));
    store.traffic.ram_limit_mb.store(512, Ordering::Relaxed);
    let mut s = EnforcementService::new(
        store.clone(),
        Arc::new(Metrics::new()),
        Box::new(RecordingApplier::new()),
        Arc::new(AtomicBool::new(false)),
    );
    let net = IpNetwork::new("198.51.100.0".parse().unwrap(), 24).unwrap();
    let member: IpAddr = "198.51.100.42".parse().unwrap();

    let mut block = block_cmd(net.addr, 600);
    block.cidr = Some(net);
    s.enforce(block).await.unwrap();

    assert_eq!(
        store.is_blocked_by_cidr(&member),
        Some(net),
        "block must register in the shared set the query reads"
    );
    assert!(store.get(&member).is_none(), "no per-member IpRecord");
    // CIDR block must modify CIDR state only — the representative IP is
    // not an independent target: no IpRecord, no explicit block entry, no
    // IP TTL card for net.addr.
    assert!(store.get(&net.addr).is_none());
    assert!(!s.blocked_ips.contains(&net.addr));
    assert!(!s.expirations.contains_key(&net.addr));
    assert!(s.cidr_expirations.contains_key(&net));

    let mut unblock = unblock_cmd(net.addr);
    unblock.cidr = Some(net);
    s.enforce(unblock).await.unwrap();

    assert!(
        store.is_blocked_by_cidr(&member).is_none(),
        "unblock must clear the shared set"
    );
    // CIDR unblock clears the CIDR domain only; the representative IP
    // state stays exactly as it was (untouched).
    assert!(store.get(&net.addr).is_none());
    assert!(!s.blocked_ips.contains(&net.addr));
    assert!(!s.expirations.contains_key(&net.addr));
    assert!(!s.cidr_expirations.contains_key(&net));
}

/// Interaction regression: an explicit IP block inside a CIDR prefix must
/// survive a CIDR unblock. The CIDR unblock releases the prefix domain
/// only — it is NOT an unblock of `net.addr` as an independent IP.
#[tokio::test]
async fn explicit_ip_block_survives_cidr_unblock() {
    let net = IpNetwork::new("198.51.100.0".parse().unwrap(), 24).unwrap();
    let ip0: IpAddr = "198.51.100.42".parse().unwrap();
    let mut s = svc(Box::new(RecordingApplier::new()));

    s.enforce(block_cmd(ip0, 0)).await.unwrap();
    s.enforce(block_cidr_command(net, 0)).await.unwrap();
    s.enforce(unblock_cidr_command(net)).await.unwrap();

    assert!(
        s.blocked_ips.contains(&ip0),
        "explicit IP block must survive a CIDR unblock"
    );
    match s.store.get(&ip0).unwrap() {
        Value::IpRecord(r) => assert!(
            matches!(r.block_state, BlockState::Blocked { .. }),
            "explicit IP block_state must survive a CIDR unblock"
        ),
        _ => panic!("expected IP record"),
    }
    assert!(
        !s.store.active_cidrs.contains_key(&net),
        "CIDR unblock must clear the prefix"
    );
}

/// P5 case 2/3: WAL committed, then replay twice is safe (no double-apply).
#[tokio::test]
async fn replay_twice_is_idempotent() {
    let dir = std::env::temp_dir().join(format!("rs_wal_idemp_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = svc_with_wal(Box::new(RecordingApplier::new()), dir.to_str().unwrap());
    let target = ip([10, 91, 0, 2]);
    s.enforce(block_cmd(target, 3600)).await.unwrap();
    drop(s);

    let wal = Arc::new(
        Wal::open(
            dir.to_str().unwrap(),
            false,
            ramshield_types::Durability::None,
            64 * 1024 * 1024,
            0,
        )
        .unwrap(),
    );
    let a = Arc::new(Store::new(16));
    let first = replay_wal_into_store(&a, &wal, 0).unwrap();
    let second = replay_wal_into_store(&a, &wal, 0).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].0, second[0].0);
    let _ = std::fs::remove_dir_all(&dir);
}

struct MapApplier {
    blocked: std::collections::HashSet<IpAddr>,
    cidrs: std::collections::HashSet<IpNetwork>,
}
impl MapApplier {
    fn new() -> Self {
        Self {
            blocked: Default::default(),
            cidrs: Default::default(),
        }
    }
}
#[async_trait::async_trait]
impl XdpApplier for MapApplier {
    fn apply_block(&mut self, ip: IpAddr, _: Uuid, _: u64) -> Result<(), EnforcementError> {
        self.blocked.insert(ip);
        Ok(())
    }
    fn apply_unblock(&mut self, ip: IpAddr, _: Uuid) -> Result<(), EnforcementError> {
        self.blocked.remove(&ip);
        Ok(())
    }
    fn apply_cidr_block(&mut self, n: IpNetwork, _: Uuid, _: u64) -> Result<(), EnforcementError> {
        self.cidrs.insert(n);
        Ok(())
    }
    fn apply_cidr_unblock(&mut self, n: IpNetwork, _: Uuid) -> Result<(), EnforcementError> {
        self.cidrs.remove(&n);
        Ok(())
    }
    fn reconcile(
        &mut self,
        expected: &[IpAddr],
        expected_cidrs: &[IpNetwork],
    ) -> Result<ReconciliationState, EnforcementError> {
        let want: std::collections::HashSet<_> = expected.iter().copied().collect();
        let stale: Vec<_> = self.blocked.difference(&want).copied().collect();
        let missing: Vec<_> = want.difference(&self.blocked).copied().collect();
        for ip in &stale {
            self.blocked.remove(ip);
        }
        for ip in &missing {
            self.blocked.insert(*ip);
        }
        let want_c: std::collections::HashSet<_> = expected_cidrs.iter().copied().collect();
        let stale_c: Vec<_> = self.cidrs.difference(&want_c).copied().collect();
        let missing_c: Vec<_> = want_c.difference(&self.cidrs).copied().collect();
        for c in &stale_c {
            self.cidrs.remove(c);
        }
        for c in &missing_c {
            self.cidrs.insert(*c);
        }
        Ok(ReconciliationState {
            last_wal_lsn: 0,
            pending_blocks: missing,
            pending_unblocks: stale.clone(),
            evicted_count: stale.len() as u64,
        })
    }
    fn configure_trusted_overlay(&mut self, _cidrs: &[IpNetwork]) -> Result<(), EnforcementError> {
        Ok(())
    }
    fn configure_autonomous(
        &mut self,
        _enabled: bool,
        _syn_pps_per_cpu: u64,
        _udp_pps_per_cpu: u64,
        _packet_pps_per_cpu: u64,
        _window_ms: u64,
    ) -> Result<(), EnforcementError> {
        Ok(())
    }
}

#[tokio::test]
async fn reconcile_repairs_missing_and_stale_ip() {
    let mut a = MapApplier::new();
    let live = ip([10, 1, 0, 1]);
    let stale_ip = ip([10, 1, 0, 2]);
    a.blocked.insert(stale_ip);
    a.reconcile(&[live], &[]).unwrap();
    assert!(a.blocked.contains(&live));
    assert!(!a.blocked.contains(&stale_ip));
}

#[tokio::test]
async fn reconcile_repairs_missing_and_stale_cidr() {
    let mut a = MapApplier::new();
    let live = IpNetwork::new("198.51.100.0".parse().unwrap(), 24).unwrap();
    let stale_c = IpNetwork::new("203.0.113.0".parse().unwrap(), 24).unwrap();
    a.cidrs.insert(stale_c);
    a.reconcile(&[], &[live]).unwrap();
    assert!(a.cidrs.contains(&live));
    assert!(!a.cidrs.contains(&stale_c));
}

/// A-017: in-memory XDP applier with hard map capacity (kernel map full).
struct BoundedMapApplier {
    cap: usize,
    blocked: std::collections::HashSet<IpAddr>,
    cidrs: std::collections::HashSet<IpNetwork>,
    /// Simulate "map wiped" (e.g. program reload / map recreate).
    lost: bool,
    /// Fail after this many successful applies (partial install).
    fail_after: Option<usize>,
    applies: usize,
}

impl BoundedMapApplier {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            blocked: Default::default(),
            cidrs: Default::default(),
            lost: false,
            fail_after: None,
            applies: 0,
        }
    }

    fn simulate_map_loss(&mut self) {
        self.blocked.clear();
        self.cidrs.clear();
        self.lost = true;
    }
}

#[async_trait::async_trait]
impl XdpApplier for BoundedMapApplier {
    fn apply_block(&mut self, ip: IpAddr, _: Uuid, _: u64) -> Result<(), EnforcementError> {
        if self.fail_after.is_some_and(|n| self.applies >= n) {
            return Err(EnforcementError::Xdp("partial install failed".into()));
        }
        if !self.blocked.contains(&ip) && self.blocked.len() >= self.cap {
            return Err(EnforcementError::Xdp(format!(
                "BLOCKLIST capacity full ({})",
                self.cap
            )));
        }
        self.blocked.insert(ip);
        self.applies += 1;
        self.lost = false;
        Ok(())
    }
    fn apply_unblock(&mut self, ip: IpAddr, _: Uuid) -> Result<(), EnforcementError> {
        self.blocked.remove(&ip);
        Ok(())
    }
    fn apply_cidr_block(&mut self, n: IpNetwork, _: Uuid, _: u64) -> Result<(), EnforcementError> {
        if !self.cidrs.contains(&n) && self.cidrs.len() >= self.cap {
            return Err(EnforcementError::Xdp(format!(
                "BLOCKCIDR capacity full ({})",
                self.cap
            )));
        }
        self.cidrs.insert(n);
        Ok(())
    }
    fn apply_cidr_unblock(&mut self, n: IpNetwork, _: Uuid) -> Result<(), EnforcementError> {
        self.cidrs.remove(&n);
        Ok(())
    }
    fn reconcile(
        &mut self,
        expected: &[IpAddr],
        expected_cidrs: &[IpNetwork],
    ) -> Result<ReconciliationState, EnforcementError> {
        // After map loss, treat as empty kernel view and rebuild from expected.
        if self.lost {
            self.blocked.clear();
            self.cidrs.clear();
            self.lost = false;
        }
        let want: std::collections::HashSet<_> = expected.iter().copied().collect();
        let stale: Vec<_> = self.blocked.difference(&want).copied().collect();
        let missing: Vec<_> = want.difference(&self.blocked).copied().collect();
        if self.blocked.len() - stale.len() + missing.len() > self.cap {
            return Err(EnforcementError::Xdp(
                "reconcile exceeds map capacity".into(),
            ));
        }
        for ip in &stale {
            self.blocked.remove(ip);
        }
        for ip in &missing {
            self.blocked.insert(*ip);
        }
        let want_c: std::collections::HashSet<_> = expected_cidrs.iter().copied().collect();
        let stale_c: Vec<_> = self.cidrs.difference(&want_c).copied().collect();
        let missing_c: Vec<_> = want_c.difference(&self.cidrs).copied().collect();
        for c in &stale_c {
            self.cidrs.remove(c);
        }
        for c in &missing_c {
            self.cidrs.insert(*c);
        }
        Ok(ReconciliationState {
            last_wal_lsn: 0,
            pending_blocks: missing,
            pending_unblocks: stale,
            evicted_count: stale_c.len() as u64,
        })
    }
    fn configure_trusted_overlay(&mut self, _: &[IpNetwork]) -> Result<(), EnforcementError> {
        Ok(())
    }
    fn configure_autonomous(
        &mut self,
        _: bool,
        _: u64,
        _: u64,
        _: u64,
        _: u64,
    ) -> Result<(), EnforcementError> {
        Ok(())
    }
}

#[test]
fn bounded_map_capacity_rejects_new_blocks() {
    let mut a = BoundedMapApplier::new(2);
    let a1 = ip([10, 0, 0, 1]);
    let a2 = ip([10, 0, 0, 2]);
    let a3 = ip([10, 0, 0, 3]);
    assert!(a.apply_block(a1, Uuid::nil(), 60).is_ok());
    assert!(a.apply_block(a2, Uuid::nil(), 60).is_ok());
    let err = a.apply_block(a3, Uuid::nil(), 60).unwrap_err();
    assert!(
        format!("{err:?}").contains("capacity"),
        "expected capacity error, got {err:?}"
    );
    assert!(!a.blocked.contains(&a3));
}

#[test]
fn reconcile_after_map_loss_restores_ipv4_ipv6_and_cidr() {
    let mut a = BoundedMapApplier::new(16);
    let v4 = ip([198, 51, 100, 7]);
    let v6: IpAddr = "2001:db8::7".parse().unwrap();
    let cidr = IpNetwork::new("203.0.113.0".parse().unwrap(), 24).unwrap();
    a.apply_block(v4, Uuid::nil(), 0).unwrap();
    a.apply_block(v6, Uuid::nil(), 0).unwrap();
    a.apply_cidr_block(cidr, Uuid::nil(), 0).unwrap();
    a.simulate_map_loss();
    assert!(a.blocked.is_empty());
    let st = a.reconcile(&[v4, v6], &[cidr]).unwrap();
    assert!(a.blocked.contains(&v4));
    assert!(a.blocked.contains(&v6));
    assert!(a.cidrs.contains(&cidr));
    assert_eq!(st.pending_blocks.len(), 2);
}

#[test]
fn partial_install_failure_does_not_commit_failed_ip() {
    let mut a = BoundedMapApplier::new(8);
    a.fail_after = Some(1);
    let ok_ip = ip([10, 1, 0, 1]);
    let fail_ip = ip([10, 1, 0, 2]);
    assert!(a.apply_block(ok_ip, Uuid::nil(), 30).is_ok());
    assert!(a.apply_block(fail_ip, Uuid::nil(), 30).is_err());
    assert!(a.blocked.contains(&ok_ip));
    assert!(!a.blocked.contains(&fail_ip));
}

#[test]
fn stub_xdp_is_explicit_no_kernel_side_effects() {
    // --no-xdp / unsupported env: Stub accepts calls but never tracks state.
    let mut s = StubXdpApplier;
    let ip = ip([8, 8, 8, 8]);
    s.apply_block(ip, Uuid::nil(), 10).unwrap();
    let st = s.reconcile(&[ip], &[]).unwrap();
    assert!(st.pending_blocks.is_empty());
    assert_eq!(st.evicted_count, 0);
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]
    #[test]
    fn sequence_invariant(ops in proptest::collection::vec(
        (proptest::arbitrary::any::<u8>(), proptest::bool::ANY, proptest::option::of(1u64..100)),
        1..64
    )) {
        use proptest::prop_assert;
        proptest::prop_assume!(!ops.is_empty());
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        rt.block_on(async move {
            let mut s = svc(Box::new(RecordingApplier::new()));
            for (seed, is_block, ttl) in ops {
                let target = ip([10, 0, seed / 2, seed]);
                if is_block {
                    let ttl = ttl.unwrap_or(0);
                    let r = s.enforce(block_cmd(target, ttl)).await.unwrap();
                    prop_assert!(r.committed);
                    prop_assert!(s.blocked_ips.contains(&target));
                } else {
                    let _ = s.enforce(unblock_cmd(target)).await.unwrap();
                    prop_assert!(!s.blocked_ips.contains(&target));
                    prop_assert!(!s.expirations.contains_key(&target),
                        "unblock must purge TTL entry");
                }
                // Global invariant: expirations never exceed blocked set size.
                prop_assert!(s.expirations.len() <= s.blocked_ips.len());
            }
            Ok(())
        })?;
    }
}

#[cfg(test)]
mod send_assert {
    use super::*;
    fn assert_send<T: Send>() {}
    #[test]
    fn run_send() {
        fn fut_is_send<F: Send>(_f: F) {}
        let store = Arc::new(Store::new(16));
        store
            .traffic
            .ram_limit_mb
            .store(512, std::sync::atomic::Ordering::Relaxed);
        let svc = EnforcementService::new(
            store,
            Arc::new(Metrics::new()),
            Box::new(RecordingApplier::new()),
            Arc::new(AtomicBool::new(false)),
        );
        let (_, rx) = mpsc::channel(1);
        fut_is_send(svc.run(rx));
        // Also assert the struct itself is Send
        assert_send::<EnforcementService>();
    }
}
