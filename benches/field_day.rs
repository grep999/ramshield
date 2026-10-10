//! Field-day benchmarks: drive the DETECTION decision pipeline and the
//! ENFORCEMENT actor path end-to-end (hot_paths.rs only covers storage
//! primitives + isolated math steps).
//!
//! Each bench builds a fresh subsystem, warms up unmeasured, then times n
//! steady-state iterations. Per-flush granularity: one iteration = one full
//! flush (or one enforce() call), NOT one event — event-level cost is the
//! division.

use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

fn print_line(label: &str, ns: f64) {
    println!(
        "  {:<52} {:>10.1} ns/op  ({:>10.0} ops/s)",
        label,
        ns,
        1_000_000_000.0 / ns
    );
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn ip(i: u32) -> IpAddr {
    format!("10.{}.{}.{}", (i >> 16) & 0xFF, (i >> 8) & 0xFF, i & 0xFF)
        .parse()
        .unwrap()
}

fn events_for(
    ips: &[IpAddr],
    per_ip: u64,
    base_ts_ns: u64,
) -> Vec<ramshield_types::ConnectionEvent> {
    let mut out = Vec::with_capacity(ips.len() * per_ip as usize);
    for (idx, ev_i) in (0..)
        .step_by(ips.len().max(1))
        .take(ips.len() * per_ip as usize)
        .enumerate()
    {
        let ip_idx = idx % ips.len();
        out.push(ramshield_types::ConnectionEvent {
            ip: ips[ip_idx],
            timestamp_ns: base_ts_ns + ev_i as u64 * 1_000_000,
            bytes: 256,
            status_code: 200,
            proto_fingerprint: 1,
            l7: None,
        });
    }
    out
}

fn det_engine(rps_threshold: u64) -> ramshield_detection::DetectionEngine {
    use ramshield_config::Config;
    use ramshield_metrics::Metrics;
    use ramshield_storage::Store;
    let mut cfg = Config::default();
    cfg.detection.rps_threshold = rps_threshold;
    let store = Arc::new(Store::new(64));
    store
        .traffic
        .ram_limit_mb
        .store(512, std::sync::atomic::Ordering::Relaxed);
    let (tx, _rx) = tokio::sync::mpsc::channel(8192);
    ramshield_detection::DetectionEngine::try_new(
        store,
        cfg.into_handle(),
        tx,
        Arc::new(Metrics::new()),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("SHM init")
}

// ── Detection: full flush pipeline ──────────────────────────────────────────

/// 4096 events / 128 IPs (32 events/IP > promote_min 8): every IP passes the
/// cold gate and runs merge_record (store RMW + EWMA + CUSUM + pulse + gate).
fn det_flush_promoted(n_flushes: usize) -> f64 {
    let ips: Vec<IpAddr> = (0..128).map(ip).collect();
    let events = events_for(&ips, 32, 1_000_000_000);
    let eng = det_engine(1_000);
    for _ in 0..8 {
        eng.flush_events(&events);
    }
    let t0 = Instant::now();
    for _ in 0..n_flushes {
        eng.flush_events(&events);
    }
    t0.elapsed().as_nanos() as f64 / n_flushes as f64
}

/// 4096 events / 4096 IPs (1 event/IP): cold-skip path — subnet key, bloom
/// slots, two map lookups, then skip. The steady-state bulk of real traffic.
fn det_flush_cold(n_flushes: usize) -> f64 {
    let ips: Vec<IpAddr> = (0..4096).map(ip).collect();
    let events = events_for(&ips, 1, 1_000_000_000);
    let eng = det_engine(1_000);
    for _ in 0..8 {
        eng.flush_events(&events);
    }
    let t0 = Instant::now();
    for _ in 0..n_flushes {
        eng.flush_events(&events);
    }
    t0.elapsed().as_nanos() as f64 / n_flushes as f64
}

/// 4096 events / 10 IPs at rps_threshold=50: sustained attackers that cross
/// the block threshold. 3 priming flushes (debounce/pulse need 2 samples),
/// then steady state: blocked IPs still get merge_record (pulse tracker must
/// keep running) and the admission gate suppresses re-emission.
fn det_flush_sustained_blocked(n_flushes: usize) -> f64 {
    let ips: Vec<IpAddr> = (0..10).map(ip).collect();
    let events = events_for(&ips, 410, 1_000_000_000);
    let eng = det_engine(50);
    for _ in 0..3 {
        eng.flush_events(&events);
    }
    let t0 = Instant::now();
    for _ in 0..n_flushes {
        eng.flush_events(&events);
    }
    t0.elapsed().as_nanos() as f64 / n_flushes as f64
}

/// 4096 events / 512 IPs, all high enough to promote + emit a fresh block:
/// the full emit path — admission gate, EnforceCommand build, try_send,
/// record_block_ip, bloom ArcSwap clone+insert+store.
fn det_flush_first_attack(n_flushes: usize) -> f64 {
    // Fresh engine per call would re-run SHM init; one engine, fresh IP pool
    // per flush rotates through 4096 distinct IPs so admission never sees a
    // repeat key within the cooldown.
    let ips: Vec<IpAddr> = (0..4096).map(ip).collect();
    let base = 1_000_000_000u64;
    let mut chunk = 0usize;
    let eng = det_engine(50);
    // Prime the detectors once.
    eng.flush_events(&events_for(&ips[0..10], 410, base));
    let t0 = Instant::now();
    for _ in 0..n_flushes {
        let lo = chunk % 1024;
        let hi = lo + 512;
        let events = events_for(&ips[lo..hi], 8, base + (chunk as u64) * 1_000_000_000);
        eng.flush_events(&events);
        chunk += 512;
    }
    t0.elapsed().as_nanos() as f64 / n_flushes as f64
}

/// Subnet-scale: 32768 events spread over 512 subnets — merge_subnet_window
/// store RMW ×512 + swarm gates per IP.
/// Pure aggregation of 4096 raw events into IP + subnet maps — the per-event
/// cost paid once per event at the worker boundary (everything in the flush
/// path is per-distinct-IP, this is per-event).
fn det_aggregate_4096(n: usize) -> f64 {
    let ips: Vec<IpAddr> = (0..4096).map(ip).collect();
    let events = events_for(&ips, 1, 1_000_000_000);
    for _ in 0..8 {
        std::hint::black_box(ramshield_detection::batch::aggregate(&events));
    }
    let t0 = Instant::now();
    for _ in 0..n {
        std::hint::black_box(ramshield_detection::batch::aggregate(&events));
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn det_flush_subnet_scale(n_flushes: usize) -> f64 {
    // 512 subnets × 64 addresses
    let ips: Vec<IpAddr> = (0..32768).map(ip).collect();
    let events = events_for(&ips, 1, 1_000_000_000);
    let eng = det_engine(1_000);
    for _ in 0..4 {
        eng.flush_events(&events);
    }
    let t0 = Instant::now();
    for _ in 0..n_flushes {
        eng.flush_events(&events);
    }
    t0.elapsed().as_nanos() as f64 / n_flushes as f64
}

// ── Enforcement: actor path ─────────────────────────────────────────────────

struct NullApplier;
#[async_trait::async_trait]
impl ramshield_enforcement::XdpApplier for NullApplier {
    fn apply_block(
        &mut self,
        _: IpAddr,
        _: uuid::Uuid,
        _: u64,
    ) -> Result<(), ramshield_types::EnforcementError> {
        Ok(())
    }
    fn apply_unblock(
        &mut self,
        _: IpAddr,
        _: uuid::Uuid,
    ) -> Result<(), ramshield_types::EnforcementError> {
        Ok(())
    }
    fn reconcile(
        &mut self,
        _: &[IpAddr],
        _: &[ramshield_types::IpNetwork],
    ) -> Result<ramshield_enforcement::ReconciliationState, ramshield_types::EnforcementError> {
        Ok(ramshield_enforcement::ReconciliationState {
            last_wal_lsn: 0,
            pending_blocks: Vec::new(),
            pending_unblocks: Vec::new(),
            evicted_count: 0,
        })
    }
    fn configure_trusted_overlay(
        &mut self,
        _: &[ramshield_types::IpNetwork],
    ) -> Result<(), ramshield_types::EnforcementError> {
        Ok(())
    }
    fn configure_autonomous(
        &mut self,
        _: bool,
        _: u64,
        _: u64,
        _: u64,
        _: u64,
    ) -> Result<(), ramshield_types::EnforcementError> {
        Ok(())
    }
}

fn enf_block_cmd(ip: IpAddr, ttl: u64, id: uuid::Uuid) -> ramshield_types::EnforceCommand {
    ramshield_types::EnforceCommand {
        decision_id: id,
        policy_version: 1,
        source: "bench".into(),
        actor: "bench".into(),
        timestamp_utc: 0,
        ttl_seconds: ttl,
        reason: "high_rps".into(),
        ip,
        cidr: None,
        action: ramshield_types::EnforceAction::Block,
        evidence_source: ramshield_types::EvidenceSource::LocalSignals,
    }
}

fn enf_unblock_cmd(ip: IpAddr, id: uuid::Uuid) -> ramshield_types::EnforceCommand {
    ramshield_types::EnforceCommand {
        decision_id: id,
        policy_version: 1,
        source: "bench".into(),
        actor: "bench".into(),
        timestamp_utc: 0,
        ttl_seconds: 0,
        reason: "manual".into(),
        ip,
        cidr: None,
        action: ramshield_types::EnforceAction::Unblock,
        evidence_source: ramshield_types::EvidenceSource::LocalSignals,
    }
}

fn enforcer() -> ramshield_enforcement::EnforcementService {
    let store = Arc::new(ramshield_storage::Store::new(64));
    store
        .traffic
        .ram_limit_mb
        .store(512, std::sync::atomic::Ordering::Relaxed);
    ramshield_enforcement::EnforcementService::new(
        store,
        Arc::new(ramshield_metrics::Metrics::new()),
        Box::new(NullApplier),
        Arc::new(AtomicBool::new(false)),
    )
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
}

/// Full block transition, WAL off: store RMW + ring schedule + applier.
fn enf_block_wal_off(n: usize) -> f64 {
    let mut s = enforcer();
    let r = rt();
    r.block_on(async move {
        for _ in 0..256 {
            s.enforce(enf_block_cmd(ip(1), 60, uuid::Uuid::new_v4()))
                .await
                .unwrap();
        }
        let t0 = Instant::now();
        for i in 0..n {
            s.enforce(enf_block_cmd(
                ip(1 + i as u32 % 50_000),
                60,
                uuid::Uuid::new_v4(),
            ))
            .await
            .unwrap();
        }
        t0.elapsed().as_nanos() as f64 / n as f64
    })
}

/// WAL on, GroupCommit (prod default): JSON serialize + CRC + write, fsync
/// amortized to ≤10 Hz.
fn enf_block_wal_group_commit(n: usize) -> f64 {
    let dir = std::env::temp_dir().join(format!("rs_bench_wal_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = enforcer();
    s = s.with_wal(Arc::new(
        ramshield_storage::wal::Wal::open(
            dir.to_str().unwrap(),
            false,
            ramshield_types::Durability::GroupCommit,
            64 * 1024 * 1024,
            0,
        )
        .unwrap(),
    ));
    let r = rt();
    r.block_on(async move {
        for _ in 0..64 {
            s.enforce(enf_block_cmd(ip(2), 60, uuid::Uuid::new_v4()))
                .await
                .unwrap();
        }
        let t0 = Instant::now();
        for i in 0..n {
            s.enforce(enf_block_cmd(
                ip(2 + i as u32 % 50_000),
                60,
                uuid::Uuid::new_v4(),
            ))
            .await
            .unwrap();
        }
        let ns = t0.elapsed().as_nanos() as f64 / n as f64;
        let _ = std::fs::remove_dir_all(&dir);
        ns
    })
}

/// Idempotent replay: same decision_id N times → cached result path.
fn enf_duplicate_decision(n: usize) -> f64 {
    let mut s = enforcer();
    let r = rt();
    r.block_on(async move {
        let cmd = enf_block_cmd(ip(3), 60, uuid::Uuid::new_v4());
        s.enforce(cmd.clone()).await.unwrap();
        let t0 = Instant::now();
        for _ in 0..n {
            s.enforce(cmd.clone()).await.unwrap();
        }
        t0.elapsed().as_nanos() as f64 / n as f64
    })
}

/// Unblock transition (store RMW back to Clean + ring detach + applier).
fn enf_unblock(n: usize) -> f64 {
    let mut s = enforcer();
    let r = rt();
    r.block_on(async move {
        for i in 0..5_000u32 {
            s.enforce(enf_block_cmd(ip(100_000 + i), 60, uuid::Uuid::new_v4()))
                .await
                .unwrap();
        }
        let t0 = Instant::now();
        for i in 0..n {
            s.enforce(enf_unblock_cmd(
                ip(100_000 + i as u32 % 5_000),
                uuid::Uuid::new_v4(),
            ))
            .await
            .unwrap();
        }
        t0.elapsed().as_nanos() as f64 / n as f64
    })
}

/// Periodic-reconcile userspace cost with 10k blocked: get_all_blocked_ips
/// allocation + iteration is what every 10 s tick pays (the kernel map work
/// is the stub's job, not measured here).
fn storage_get_all_blocked_ips_10k(n: usize) -> f64 {
    use ramshield_storage::{BlockState, IpRecord, Value};
    let store = Arc::new(ramshield_storage::Store::new(64));
    store
        .traffic
        .ram_limit_mb
        .store(512, std::sync::atomic::Ordering::Relaxed);
    for i in 0..10_000u32 {
        let rec = IpRecord {
            ip: ip(400_000 + i),
            request_count: 1,
            ewma_rps: 0.0,
            cusum_s: 0.0,
            baseline_rps: 0.0,
            prev_sample_hot: false,
            sample_count: 0,
            relative_breach_streak: 0,
            pulse_samples_in_window: 0,
            pulse_window_start_ns: 0,
            first_seen_ns: 0,
            last_seen_ns: 0,
            bytes_in: 0,
            status_dist: [0; 5],
            proto_fingerprint: 0,
            threat_score: 0.0,
            block_state: BlockState::Blocked {
                reason: ramshield_types::BlockReason::HighRps,
                since_ns: 0,
            },
        };
        store
            .insert(ip(400_000 + i), Value::IpRecord(rec), None, 1 << 30)
            .unwrap();
    }
    let t0 = Instant::now();
    for _ in 0..n {
        let all = store.get_all_blocked_ips();
        std::hint::black_box(all.len());
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

fn main() {
    println!(
        "═══════════════════════════════════════════════════════════════════\n── FIELD DAY: detection pipeline + enforcement actor ──────────────────"
    );

    // Warm cache + page faults for all involved paths.
    let _ = det_flush_promoted(4);

    print_line(
        "detection flush 4096ev/128ips (all promoted, RMW+detectors)",
        det_flush_promoted(200),
    );
    print_line(
        "detection flush 4096ev/4096ips (cold-skip: gate + bloom)",
        det_flush_cold(100),
    );
    print_line(
        "detection flush 4096ev/10ips SUSTAINED blocked (gate suppress)",
        det_flush_sustained_blocked(200),
    );
    print_line(
        "detection flush 4096ev/512ips FIRST attack (emit+admit+bloom)",
        det_flush_first_attack(100),
    );
    print_line(
        "detection flush 32768ev/512 subnets (subnet window RMW x512)",
        det_flush_subnet_scale(64),
    );
    print_line(
        "detection aggregate(4096 raw events): pure per-event cost",
        det_aggregate_4096(500),
    );

    println!("── Enforcement ──────────────────────────────────────────────────");
    print_line(
        "enforce block (WAL off): full transition",
        enf_block_wal_off(20_000),
    );
    print_line(
        "enforce block (WAL GroupCommit, fsync ≤10Hz)",
        enf_block_wal_group_commit(20_000),
    );
    print_line(
        "enforce duplicate decision_id (idempotent cache)",
        enf_duplicate_decision(50_000),
    );
    print_line("enforce unblock: full transition", enf_unblock(10_000));

    print_line(
        "storage get_all_blocked_ips (10k blocked, every 10 s)",
        storage_get_all_blocked_ips_10k(200),
    );
    println!("═══════════════════════════════════════════════════════════════════");
}
