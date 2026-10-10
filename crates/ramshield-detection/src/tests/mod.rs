use super::*;
use ramshield_config::Config;
use ramshield_storage::Value;
use std::net::Ipv4Addr;

fn engine() -> Arc<DetectionEngine> {
    let cfg = Config::default().into_handle();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, _erx) = mpsc::channel(64);
    let shutdown = Arc::new(AtomicBool::new(false));
    Arc::new(
        DetectionEngine::try_new(store, cfg, etx, metrics, shutdown).expect("detection try_new"),
    )
}

/// Documented architecture contract, admission item: re-emitting a block
/// for the same (ip, reason) every flush window re-did WAL append + store
/// write + ring re-arm + XDP apply per window. Gate: first admission
/// passes, re-admission inside the TTL/2 cooldown is suppressed, a
/// different reason is an independent key, and queue rejection removes
/// the pending key so the next window can retry.
#[test]
fn admission_gate_suppresses_reemit_within_cooldown() {
    let eng = engine();
    let ip: IpAddr = "10.0.0.55".parse().unwrap();
    let t0 = 1_000_000_000_000u64;
    let key = (ip, BlockReason::HighRps);
    assert!(
        eng.admit_mitigation(key, 3600, t0),
        "first admission passes"
    );
    assert!(
        !eng.admit_mitigation(key, 3600, t0 + 1_000_000_000),
        "re-admission inside ttl/2 cooldown must be suppressed"
    );
    assert!(
        eng.admit_mitigation((ip, BlockReason::SubnetBatch), 3600, t0 + 1_000),
        "different reason is an independent admission key"
    );
    // Queue rejection: the pending key must be removed so the next
    // window can retry (a stuck key would suppress the block forever).
    eng.retreat_mitigation(key);
    assert!(
        eng.admit_mitigation(key, 3600, t0 + 1_000_000),
        "after a queue rejection the same key must admit again"
    );
}

/// Admission is time-bounded: past the cooldown the same (ip, reason)
/// admits again — that re-admission is the TTL refresh mechanism.
#[test]
fn admission_gate_rearms_after_cooldown() {
    let eng = engine();
    let ip: IpAddr = "10.0.0.56".parse().unwrap();
    let key = (ip, BlockReason::HighRps);
    let ttl = 3600u64;
    let t0 = 1_000_000_000_000u64;
    assert!(eng.admit_mitigation(key, ttl, t0));
    // cooldown = ttl/2
    let half = ttl * 1_000_000_000 / 2;
    assert!(!eng.admit_mitigation(key, ttl, t0 + half - 1));
    assert!(
        eng.admit_mitigation(key, ttl, t0 + half),
        "past cooldown re-admits"
    );
}

/// Item 3 regression: worker-local merge must equal the old shared-map
/// semantics — same-IP counts/statuses summed, window timestamps min/max,
/// local buffer emptied into shared.
#[test]
fn local_merge_preserves_cross_worker_semantics() {
    let eng = engine();
    let mut a: HashMap<IpAddr, IpAgg> = HashMap::new();
    let mut b: HashMap<IpAddr, IpAgg> = HashMap::new();
    let ip: IpAddr = "1.2.3.4".parse().unwrap();
    a.entry(ip).or_default().absorb(&ConnectionEvent {
        ip,
        timestamp_ns: 500,
        bytes: 10,
        status_code: 404,
        proto_fingerprint: 7,
        l7: None,
    });
    b.entry(ip).or_default().absorb(&ConnectionEvent {
        ip,
        timestamp_ns: 200, // earlier than a's first
        bytes: 15,
        status_code: 200,
        proto_fingerprint: 0,
        l7: None,
    });
    eng.merge_local(&mut a);
    eng.merge_local(&mut b);
    let agg = eng.pre_aggs.get(&ip).unwrap();
    assert_eq!(agg.count, 2);
    assert_eq!(agg.bytes, 25);
    assert_eq!(agg.first_ts_ns, 200, "window min across workers");
    assert_eq!(agg.last_ts_ns, 500, "window max across workers");
    assert_eq!(agg.status_dist[3], 1, "4xx from worker a");
    assert_eq!(agg.status_dist[1], 1, "2xx from worker b");
    assert_eq!(agg.proto_fp, 7, "first non-zero fingerprint wins");
    assert!(a.is_empty(), "local buffer is drained into shared");
}

/// A-020 regression: the shared pre-aggregation map never exceeds its
/// configured distinct-IP limit, even when one worker contributes a batch
/// larger than the remaining capacity. Flushed plus pending counts are exact.
#[test]
fn local_merge_enforces_shared_pre_aggregation_capacity() {
    let mut config = Config::default();
    config.detection.pre_aggs_max_size = 2;
    let cfg = config.into_handle();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, _erx) = mpsc::channel(64);
    let eng = DetectionEngine::try_new(store, cfg, etx, metrics, Arc::new(AtomicBool::new(false)))
        .expect("detection try_new");

    let mut local: HashMap<IpAddr, IpAgg> = HashMap::new();
    for octet in 1..=5 {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, octet));
        local.entry(ip).or_default().absorb(&ConnectionEvent {
            ip,
            timestamp_ns: u64::from(octet),
            bytes: 1,
            status_code: 200,
            proto_fingerprint: 0,
            l7: None,
        });
    }

    eng.merge_local(&mut local);
    assert!(local.is_empty(), "all local aggregates must be transferred");
    assert!(
        eng.pre_aggs.len() <= 2,
        "shared map exceeded configured capacity: {}",
        eng.pre_aggs.len()
    );
    let pending: u64 = eng
        .pre_aggs
        .iter()
        .map(|entry| entry.value().count as u64)
        .sum();
    let ingested = eng.metrics.events_ingested.load(Ordering::Relaxed);
    assert_eq!(
        ingested + pending,
        5,
        "capacity-triggered flush must preserve every event"
    );
}

/// Step 3 fast path: crossing the emergency burst threshold on the
/// per-event path emits a block BEFORE any flush, fires exactly once per
/// window (count only rises, so the threshold is crossed once), and —
/// documented architecture contract — a fresh window's re-crossing is
/// admitted only after the (ip, reason) cooldown (ttl/2, min 1s): the
/// re-admission is the block's TTL refresh, not a per-window re-emit.
#[test]
fn emergency_burst_fires_once_before_flush() {
    let mut config = Config::default();
    config.detection.emergency_burst_threshold = 5;
    // 2s block TTL -> 1s admission cooldown (floor), so the re-fire
    // assertion below can wait a real wall-clock second.
    config.detection.block_ttl_secs = 2;
    let cfg = config.into_handle();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, mut erx) = mpsc::channel(64);
    let eng = Arc::new(
        DetectionEngine::try_new(
            store,
            cfg,
            etx,
            metrics.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("detection try_new"),
    );

    let ip: IpAddr = "9.9.9.9".parse().unwrap();
    let ev = |n: u64| ConnectionEvent {
        ip,
        timestamp_ns: n,
        bytes: 10,
        status_code: 200,
        proto_fingerprint: 0,
        l7: None,
    };
    let mut local: HashMap<IpAddr, IpAgg> = HashMap::new();

    // Window 1: count climbs 1..5; crossing at 5 emits the block.
    for n in 0..5 {
        eng.absorb_or_emergency(&mut local, &ev(n), 5);
    }
    let cmd = erx
        .try_recv()
        .expect("emergency block must be emitted pre-flush");
    assert_eq!(cmd.ip, ip);
    assert!(
        matches!(cmd.action, EnforceAction::Block),
        "must be a Block"
    );
    // Regression: the pre-flush fast path must count into the applied
    // detection-block total, otherwise the dashboard's blocks_total and
    // the enforcement stage under-report every emergency-path block.
    assert_eq!(
        metrics
            .blocks_detection
            .load(std::sync::atomic::Ordering::Relaxed),
        1,
        "emergency-path block must increment blocks_detection"
    );

    // Still hot in the same window: no re-emit (crossing is one-shot).
    for n in 5..30 {
        eng.absorb_or_emergency(&mut local, &ev(n), 5);
    }
    assert!(
        erx.try_recv().is_err(),
        "crossing must fire exactly once per window"
    );

    // Drain (simulates flush) -> fresh window re-crosses, but the
    // (ip, reason) is inside its 1s admission cooldown: suppressed.
    // The enforcement layer keeps the block alive; re-emission is the
    // TTL refresh and only happens at the cooldown boundary.
    local.clear();
    for n in 0..5 {
        eng.absorb_or_emergency(&mut local, &ev(n), 5);
    }
    assert!(
        erx.try_recv().is_err(),
        "re-crossing inside the admission cooldown must be suppressed"
    );

    // Cooldown elapses -> the same crossing admits again (TTL refresh).
    std::thread::sleep(std::time::Duration::from_millis(1050));
    local.clear();
    for n in 0..5 {
        eng.absorb_or_emergency(&mut local, &ev(n), 5);
    }
    assert!(
        erx.try_recv().is_ok(),
        "a crossing past the admission cooldown must re-fire (TTL refresh)"
    );
}

/// P1 regression (F1): with N workers, the old iter_mut+take+clear flush
/// silently erased events inserted during the walk. Invariant:
/// ingested + left-in-pre_aggs == sent, exactly, under concurrent flush.
#[test]
fn concurrent_flush_never_loses_events() {
    let cfg = Config::default().into_handle();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, _erx) = mpsc::channel(64);
    let eng = Arc::new(
        DetectionEngine::try_new(
            store,
            cfg,
            etx,
            metrics.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("detection try_new"),
    );

    const SENDERS: u64 = 8;
    const PER_SENDER: u64 = 5_000;
    let stop = Arc::new(AtomicBool::new(false));

    let flushers: Vec<_> = (0..4)
        .map(|_| {
            let e = eng.clone();
            let s = stop.clone();
            std::thread::spawn(move || {
                while !s.load(Ordering::Relaxed) {
                    e.flush_pre_aggs_to_store();
                    std::thread::sleep(std::time::Duration::from_micros(200));
                }
            })
        })
        .collect();

    let feeds: Vec<_> = (0..SENDERS)
        .map(|w| {
            let e = eng.clone();
            std::thread::spawn(move || {
                for i in 0..PER_SENDER {
                    let ip: IpAddr = format!("10.{}.0.{}", w, i % 251).parse().unwrap();
                    e.absorb_shared(ConnectionEvent {
                        ip,
                        timestamp_ns: i * 1_000_000,
                        bytes: 100,
                        status_code: 200,
                        proto_fingerprint: 0,
                        l7: None,
                    });
                }
            })
        })
        .collect();
    for f in feeds {
        f.join().unwrap();
    }
    stop.store(true, Ordering::Relaxed);
    for f in flushers {
        f.join().unwrap();
    }
    // Final drain (single-flusher now uncontended).
    while !eng.pre_aggs.is_empty() {
        eng.flush_pre_aggs_to_store();
    }

    let sent = SENDERS * PER_SENDER;
    let ingested = metrics.events_ingested.load(Ordering::Relaxed);
    let left: u64 = eng.pre_aggs.iter().map(|a| a.value().count as u64).sum();
    assert_eq!(
        ingested + left,
        sent,
        "F1 loss: ingested={ingested} left={left} sent={sent}"
    );
}

/// Saturation guard: the sizing rule is bloom_bits ≈ 20 × (promoted IPs
/// per 8 s epoch) for ~1% FP on the 2-probe filter. At 2n/b ≥ 10%
/// (n ≥ b/20) the gate has stopped cold-skipping — the guard must fire
/// so the epoch clears early instead of silently thrashing the store.
#[test]
fn bloom_saturated_at_one_tenth_fill() {
    use tokio::sync::mpsc;
    let mut cfg = Config::default();
    cfg.detection.bloom_bits = 1_000_000;
    let metrics = Arc::new(Metrics::new());
    let (etx, _erx) = mpsc::channel(64);
    let eng = DetectionEngine::try_new(
        Arc::new(Store::new(16)),
        cfg.into_handle(),
        etx,
        metrics.clone(),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("detection try_new");
    metrics.set_bloom_bits(1_000_000);
    // b/20 = 50k promotions → 10% (2n/b). Guard fires at that line.
    metrics.record_bloom_inserts(49_999);
    assert!(!eng.bloom_saturated(), "just under the 10% line");
    metrics.record_bloom_inserts(1);
    assert!(eng.bloom_saturated(), "at the 10% line the gate is dead");
    // Fresh epoch (guard clears) → unsaturated again.
    metrics.bloom_epoch_clear();
    assert!(!eng.bloom_saturated());
}

/// Patch A RED: the bloom is documented as an advisory revisit cache for
/// PROMOTED IPs, but insert ran over `blocks` — so a host promoted
/// without being blocked (the common case) left the cache cold, and
/// cold-skip fired on hosts seen one flush earlier. Asserts the promoted
/// set is what lands in the filter.
#[test]
fn bloom_caches_promoted_ips_not_just_blocked() {
    use tokio::sync::mpsc;
    let mut cfg = Config::default();
    // Promotes freely, but the RPS threshold sits far above this traffic
    // so nothing blocks. Promote-without-block is exactly the case the
    // old insert set missed (blocks was empty, so nothing was cached).
    cfg.detection.promote_min_events = 8;
    cfg.detection.rps_threshold = 1_000_000;
    let cfg = cfg.into_handle();
    let metrics = Arc::new(Metrics::new());
    let (etx, _erx) = mpsc::channel(64);
    let eng = Arc::new(
        DetectionEngine::try_new(
            Arc::new(Store::new(16)),
            cfg,
            etx,
            metrics.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("detection try_new"),
    );

    let ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 77));
    let events: Vec<_> = (0..12).map(|i| ev_at(ip, i)).collect();
    eng.flush_events(&events);

    assert!(
        eng.store.get(&ip).is_some(),
        "IP with 12 events must be promoted"
    );
    let (a, b) = BloomFilter::slots(&ip);
    assert!(
        eng.bloom.load().contains_hashed(a, b),
        "promoted-without-block IP must be in the bloom revisit cache"
    );
    assert!(
        metrics.bloom_inserts_epoch.load(Ordering::Relaxed) >= 1,
        "bloom_inserts_epoch must count promoted IPs"
    );
    assert_eq!(
        metrics.blocks_total.load(Ordering::Relaxed),
        0,
        "test premise: no blocks issued, so the old insert set was empty"
    );
}

/// Patch A RED, second half: capacity must be published at construction,
/// or fp_ppm has no denominator and silently reads 0 forever.
#[test]
fn bloom_capacity_published_at_construction() {
    use tokio::sync::mpsc;
    let mut cfg = Config::default();
    cfg.detection.bloom_bits = 1_234_567;
    let metrics = Arc::new(Metrics::new());
    let (etx, _erx) = mpsc::channel(64);
    let _eng = Arc::new(
        DetectionEngine::try_new(
            Arc::new(Store::new(16)),
            cfg.into_handle(),
            etx,
            metrics.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("detection try_new"),
    );
    assert_eq!(
        metrics.bloom_bits.load(Ordering::Relaxed),
        1_234_567,
        "constructor must publish bloom capacity for the FP estimate"
    );
}

#[test]
fn flush_promotes_hot_ip() {
    let eng = engine();
    let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
    let events: Vec<_> = (0..20)
        .map(|i| ConnectionEvent {
            ip,
            timestamp_ns: i,
            bytes: 64,
            status_code: 200,
            proto_fingerprint: 0,
            l7: None,
        })
        .collect();
    eng.flush_events(&events);
    assert!(eng.store.get(&ip).is_some());
}

#[test]
fn cold_ip_not_stored() {
    let eng = engine();
    let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
    eng.flush_events(&[ConnectionEvent {
        ip,
        timestamp_ns: 1,
        bytes: 1,
        status_code: 200,
        proto_fingerprint: 0,
        l7: None,
    }]);
    assert!(eng.store.get(&ip).is_none());
}

#[test]
fn flush_preserves_status_dist() {
    // The old reconstruct-events path zeroed status_code, so 5xx never
    // reached threat scoring. One assert that the real distribution survives.
    let eng = engine();
    let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 9, 9));
    let events: Vec<_> = (0..20)
        .map(|i| ConnectionEvent {
            ip,
            timestamp_ns: i,
            bytes: 64,
            status_code: 500,
            proto_fingerprint: 0,
            l7: None,
        })
        .collect();
    eng.flush_events(&events);
    match eng.store.get(&ip) {
        Some(Value::IpRecord(r)) => assert!(
            r.status_dist[4] >= 20,
            "5xx bucket lost: {:?}",
            r.status_dist
        ),
        other => panic!("expected IpRecord, got {other:?}"),
    }
}

#[test]
fn v6_events_aggregate_and_promote() {
    let eng = engine();
    let ip: IpAddr = "2001:db8::1".parse().unwrap();
    let events: Vec<_> = (0..20)
        .map(|i| ConnectionEvent {
            ip,
            timestamp_ns: i,
            bytes: 64,
            status_code: 200,
            proto_fingerprint: 0,
            l7: None,
        })
        .collect();
    eng.flush_events(&events);
    assert!(eng.store.get(&ip).is_some());
    // v6 /64 landed in subnet table
    let sk = subnet_key_u128(ip).unwrap();
    assert!(eng.store.subnet_table().contains_key(&sk));
}

/// IPv6 plan Task 2 (G1): the dual gate must fire for a v6 /64 swarm.
/// v4 counts uniques via the 256-bit host bitmap; a /64 has no bitmap —
/// exactness lives in `subnet_index` cardinality (plan D1).
#[test]
fn v6_subnet_swarm_blocks_via_index_cardinality() {
    use tokio::sync::mpsc;
    let mut cfg = Config::default();
    // Default window threshold (500 ev) exceeds this synthetic swarm;
    // without promotion no host reaches the store, so the reverse index
    // the gate reads stays empty. Same knob v4 tests tune implicitly.
    cfg.detection.subnet_window_threshold = 1;
    let cfg = cfg.into_handle();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, mut erx) = mpsc::channel(64);
    let eng = Arc::new(
        DetectionEngine::try_new(store, cfg, etx, metrics, Arc::new(AtomicBool::new(false)))
            .expect("detection try_new"),
    );
    // 65 distinct hosts in 2001:db8:abcd::/64, 800 events each =
    // 65 uniq / 52,000 events — crosses the deterministic classifier
    // gate (hosts > 64 && rate > 50_000) → one CIDR block decision.
    let hosts: Vec<IpAddr> = (1..=65u16)
        .map(|o| {
            IpAddr::V6(std::net::Ipv6Addr::new(
                0x2001, 0xdb8, 0xabcd, 0, 0, 0, 0, o,
            ))
        })
        .collect();
    let events: Vec<_> = hosts
        .iter()
        .flat_map(|ip| {
            let base = now_ns();
            (0..800u64).map(move |i| ev_at(*ip, base + i))
        })
        .collect();
    eng.flush_events(&events);
    // Drain the flush's per-IP blocks (inst_rps is huge in synthetic
    // tests) so what remains is only the scan's subnet_burst output.
    while erx.try_recv().is_ok() {}
    eng.subnet_batch_scan();
    let cmds: Vec<_> = {
        let mut v = Vec::new();
        while let Ok(c) = erx.try_recv() {
            v.push(c);
        }
        v
    };
    let v6_blocks: Vec<_> = cmds
        .iter()
        .filter(|c| c.ip.is_ipv6() && c.reason == "subnet_burst")
        .collect();
    assert_eq!(v6_blocks.len(), 1, "v6 /64 swarm emits one CIDR decision");
    assert_eq!(v6_blocks[0].cidr.map(|n| n.prefix_len), Some(64));
    assert_eq!(
        v6_blocks[0].cidr.map(|n| n.addr),
        Some("2001:db8:abcd::".parse().unwrap()),
    );
}

/// P1-2 regression: a cooled-off /64 (60 historical members in the
/// lifetime reverse index) plus a small fresh burst (5 hosts, 15 events)
/// must NOT pass the dual gate and batch-block ~55 innocent hosts.
/// Pre-fix: v6 gate read lifetime cardinality (60 >= 50) and the block
/// leg swept the same stale index. Post-fix: both are window-scoped by
/// `last_seen_ns`, so only the 5 fresh hosts count and get blocked.
#[test]
fn v6_cooled_subnet_does_not_block_stale_members() {
    use tokio::sync::mpsc;
    let mut cfg = Config::default();
    cfg.detection.subnet_window_threshold = 1;
    let cfg = cfg.into_handle();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, mut erx) = mpsc::channel(64);
    let eng = Arc::new(
        DetectionEngine::try_new(
            store.clone(),
            cfg,
            etx,
            metrics,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("detection try_new"),
    );
    let v6 = |o: u16| {
        IpAddr::V6(std::net::Ipv6Addr::new(
            0x2001, 0xdb8, 0xbeef, 0, 0, 0, 0, o,
        ))
    };
    // Historical members: promoted with near-zero timestamps, so their
    // last_seen_ns is a long time before `now_ns()`.
    let old: Vec<IpAddr> = (1..=60u16).map(v6).collect();
    let old_events: Vec<_> = old
        .iter()
        .flat_map(|ip| (0..2u64).map(move |i| ev_at(*ip, i)))
        .collect();
    eng.flush_events(&old_events);
    while erx.try_recv().is_ok() {}
    // Assert they actually landed in the reverse index (else the test
    // is not exercising the cooling path).
    assert!(
        store
            .subnet_table()
            .iter()
            .map(|e| e.value().total_rps)
            .sum::<u64>()
            > 0,
        "historical burst must populate the subnet table"
    );
    // Fresh burst: 5 hosts just now, 3 events each = 15 events total.
    let fresh: Vec<IpAddr> = (101..=105u16).map(v6).collect();
    let base = now_ns();
    let fresh_events: Vec<_> = fresh
        .iter()
        .flat_map(|ip| (0..3u64).map(move |i| ev_at(*ip, base + i)))
        .collect();
    eng.flush_events(&fresh_events);
    while erx.try_recv().is_ok() {}
    eng.subnet_batch_scan();
    let cmds: Vec<_> = {
        let mut v = Vec::new();
        while let Ok(c) = erx.try_recv() {
            v.push(c);
        }
        v
    };
    let v6_blocks: Vec<_> = cmds
        .iter()
        .filter(|c| c.ip.is_ipv6() && c.reason == "subnet_burst")
        .collect();
    // Exactly the fresh hosts may be blocked — never the 60 stale ones.
    assert!(
        v6_blocks.len() <= 5,
        "stale members must not be batch-blocked, got {} blocks: {:?}",
        v6_blocks.len(),
        v6_blocks
    );
    assert!(
        v6_blocks.iter().all(|c| fresh.contains(&c.ip)),
        "only window-fresh hosts may be blocked, got {:?}",
        v6_blocks
    );
    // And the gate must not have fired on the lifetime count alone.
    assert!(
        v6_blocks.len() < 50,
        "dual gate must not pass on 60 lifetime members + 5 fresh",
    );
}

/// Regression: TIER_BLOCK's 50k rate floor must be reachable from a burst
/// spread across the 4 s subnet window. The per-scan window reset wiped
/// total_rps on every 500 ms tick for ANY hot subnet, so the classifier
/// only ever saw a single ~500 ms slice — a burst slower than 100k/s
/// (e.g. 55k events over 3 s) could never block the /24.
#[test]
fn subnet_rate_accumulates_across_scans_until_block() {
    let cfg = Config::default().into_handle();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, mut erx) = mpsc::channel(64);
    let eng = Arc::new(
        DetectionEngine::try_new(
            store.clone(),
            cfg,
            etx,
            metrics.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("detection try_new"),
    );

    let any: IpAddr = "192.0.2.9".parse().unwrap();
    let net = crate::IpNetwork::of_ip(any);
    let sk = subnet_key_u128(any).unwrap();
    let hosts: Vec<IpAddr> = (1..=250u8)
        .map(|o| IpAddr::V4(Ipv4Addr::new(192, 0, 2, o)))
        .collect();
    // 3 x 18k events, 1 s apart — all inside SUBNET_WINDOW_NS (4 s),
    // 54k total: crosses the 50k TIER_BLOCK floor only in full.
    let t0 = 5_000_000_000_000u64;
    for i in 0..3u64 {
        store.merge_subnet_window(sk, net, 18_000, Some(&hosts), t0 + i * 1_000_000_000);
        eng.subnet_batch_scan();
    }

    let cmds: Vec<_> = {
        let mut v = Vec::new();
        while let Ok(c) = erx.try_recv() {
            v.push(c);
        }
        v
    };
    let subnet_blocks: Vec<_> = cmds.iter().filter(|c| c.reason == "subnet_burst").collect();
    assert_eq!(
        subnet_blocks.len(),
        1,
        "one /24 decision for a 54k burst spread over the 4 s window"
    );
    assert_eq!(
        subnet_blocks[0].cidr,
        Some(net),
        "decision must be the /24 prefix, not a member host"
    );
    assert_eq!(metrics.blocks_subnet.load(Ordering::Relaxed), 1);
}

/// A block command that cannot reach enforcement must be COUNTED. The
/// CIDR shape is taken from `subnet_decision_is_one_block_not_one_per_member`
/// (proven to emit exactly one command with a live receiver); here the
/// receiver is dropped, so that same emit fails — the exact end state of
/// the review's burst-vs-small-queue scenario.
#[test]
fn enforcement_drop_is_counted_not_silent() {
    use tokio::sync::mpsc;
    let cfg = Config::default().into_handle();
    let metrics = Arc::new(Metrics::new());
    let (etx, erx) = mpsc::channel(64);
    drop(erx); // permanently saturated: every try_send is a hard drop
    let eng = Arc::new(
        DetectionEngine::try_new(
            Arc::new(Store::new(16)),
            cfg,
            etx,
            metrics.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("detection try_new"),
    );

    // CGNAT_TIER_BLOCK needs hosts > 64 AND rate > 50_000: 65 x 800.
    let t = now_ns();
    let events: Vec<_> = (1..=65u8)
        .flat_map(|h| {
            let ip = IpAddr::V4(Ipv4Addr::new(172, 16, 30, h));
            (0..800).map(move |i| ev_at(ip, t + i))
        })
        .collect();
    eng.flush_events(&events);
    eng.subnet_batch_scan();

    assert!(
        metrics.enforcement_dropped.load(Ordering::Relaxed) > 0,
        "a block command that never reached enforcement must be counted — \
             otherwise the operator sees a healthy engine while the attacker floods"
    );
}

fn ev_at(ip: IpAddr, ts: u64) -> ConnectionEvent {
    ConnectionEvent {
        ip,
        timestamp_ns: ts,
        bytes: 64,
        status_code: 200,
        proto_fingerprint: 0,
        l7: None,
    }
}

/// F1 regression: a single IP bursting 10 events must NOT batch-block its /24.
#[test]
fn single_ip_burst_does_not_block_subnet() {
    let eng = engine();
    let ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7));
    let events: Vec<_> = (0..10).map(|i| ev_at(ip, i)).collect();
    eng.flush_events(&events);
    let sk = subnet_key_u128(ip).unwrap();
    // DashMap guard must not be held across flush_events (write to same shard deadlocks)
    let uniq = {
        let rec = eng.store.subnet_table().get(&sk).unwrap();
        rec.unique_ips()
    };
    assert_eq!(uniq, 1, "50 re-reports of ONE ip stay one distinct host");
    // window counters accumulate, but the loop's dual gate (50 IPs AND 100
    // events) can't fire from one IP no matter how hard it hammers.
    for _ in 0..50 {
        eng.flush_events(&(0..200).map(|i| ev_at(ip, i)).collect::<Vec<_>>());
    }
    eprintln!("loop done");
    let r = eng.store.subnet_table().get(&sk).unwrap();
    assert_eq!(
        r.unique_ips(),
        1,
        "single-IP traffic never satisfies the unique gate no matter the volume"
    );
}

/// F1: volume alone is insufficient — one flood IP at high event count stays unblocked.
#[test]
fn raw_volume_alone_insufficient_for_batch_block() {
    let eng = engine();
    let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));
    // 5000 events, ONE unique IP — old code would block this /24 instantly.
    eng.flush_events(&(0..5000).map(|i| ev_at(ip, i)).collect::<Vec<_>>());
    let sk = subnet_key_u128(ip).unwrap();
    let rec = eng.store.subnet_table().get(&sk).unwrap();
    assert_eq!(
        rec.unique_ips(),
        1,
        "one IP must count as one distinct source"
    );
    assert_eq!(rec.total_rps, 5000); // volume tracked but gated by uniqueness too
}

/// F1: swarm signal — many unique IPs × moderate volume satisfies both gates.
#[test]
fn distributed_swarm_satisfies_dual_gate() {
    let eng = engine();
    // 60 IPs in same /24, 3 events each = 60 uniq / 180 events ≥ (50, 100)
    let events: Vec<_> = (0..60)
        .flat_map(|o| {
            let ip = IpAddr::V4(Ipv4Addr::new(45, 148, 10, o as u8 + 1));
            (0..3).map(move |i| ev_at(ip, i))
        })
        .collect();
    eng.flush_events(&events);
    let any_ip = IpAddr::V4(Ipv4Addr::new(45, 148, 10, 5));
    let sk = subnet_key_u128(any_ip).unwrap();
    let rec = eng.store.subnet_table().get(&sk).unwrap();
    assert_eq!(rec.unique_ips(), 60, "60 distinct hosts in bitmap");
    assert_eq!(rec.total_rps, 180);
    // dual-gate predicate (same as subnet_batch_loop) now true:
    assert!(rec.unique_ips() >= 50 && rec.total_rps >= 100);
}

/// P0 regression (sparse swarm deadlock): 60 hosts x 1 event in one /24.
/// `agg.count` (1) is below `promote_min_events` (8) for every host, so the
/// old gate (`count < min && !subnet_hot && !bloom_hit`) cold-skipped all
/// of them BEFORE `merge_subnet_window` could accumulate host cardinality —
/// subnet_hot could never become true. `swarm_hint` reads in-batch host
/// cardinality, so the same batch now promotes every participant.
#[test]
fn swarm_hint_promotes_sparse_hosts_in_one_slash24() {
    let eng = engine();
    let events: Vec<_> = (1..=60u8)
        .map(|h| ev_at(IpAddr::V4(Ipv4Addr::new(203, 0, 113, h)), 1))
        .collect();
    eng.flush_events(&events);
    let stored = (1..=60u8)
        .filter(|h| {
            eng.store
                .get(&IpAddr::V4(Ipv4Addr::new(203, 0, 113, *h)))
                .is_some()
        })
        .count();
    assert_eq!(
        stored, 60,
        "swarm_hint must promote every host once in-batch hosts >= threshold"
    );
    // And the subnet window must actually carry the swarm signal.
    let sk = subnet_key_u128(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1))).unwrap();
    let rec = eng.store.subnet_table().get(&sk).unwrap();
    assert_eq!(rec.unique_ips(), 60, "60 distinct hosts in the /24 bitmap");
}

/// One CIDR decision, one owner: a /24 swarm is ONE subnet block, not N
/// member blocks. The member loop used to publish an identical SHM rule,
/// count a CGNAT classification, write a phantom block-history row and
/// bump `blocks_subnet` for every host in the subnet — so a single
/// decision reported 60 blocks and 60 history rows for hosts that were
/// never individually blocked (check_ip correctly reads them clean).
#[test]
fn subnet_decision_is_one_block_not_one_per_member() {
    // Hand-built engine: the shared `engine()` helper drops its receiver,
    // so `try_send` fails there and the decision path can never commit.
    // This test must own a LIVE receiver to exercise a real decision.
    let cfg = Config::default().into_handle();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, mut erx) = mpsc::channel(64);
    let shutdown = Arc::new(AtomicBool::new(false));
    let eng = Arc::new(
        DetectionEngine::try_new(store, cfg, etx, metrics, shutdown).expect("detection try_new"),
    );
    // CGNAT_TIER_BLOCK needs hosts > 64 AND rate > 50_000, so the
    // enforcement command is actually emitted: 65 hosts x 800 = 52_000.
    let t = now_ns();
    let events: Vec<_> = (1..=65u8)
        .flat_map(|h| {
            let ip = IpAddr::V4(Ipv4Addr::new(172, 16, 30, h));
            (0..800).map(move |i| ev_at(ip, t + i))
        })
        .collect();
    eng.flush_events(&events);
    eng.subnet_batch_scan();

    // Exactly one enforcement command for the whole /24.
    let cmd = erx.try_recv().expect("one CIDR block command");
    assert!(cmd.cidr.is_some(), "the decision is a CIDR block");
    assert!(
        erx.try_recv().is_err(),
        "a single /24 must not emit one command per member host"
    );

    let blocks = eng.metrics.blocks_subnet.load(Ordering::Relaxed);
    assert_eq!(
        blocks, 1,
        "one CIDR decision must be one block, not one per member host"
    );
    let history = eng.metrics.get_block_log();
    assert_eq!(
        history.len(),
        1,
        "block history must hold the decision, not a phantom row per member"
    );
    assert_eq!(
        eng.metrics.cgnat_classify_ticks.load(Ordering::Relaxed),
        1,
        "classification is a subnet-level property — once per decision"
    );
    assert_eq!(
        eng.metrics.shm_publish_count.load(Ordering::Relaxed),
        1,
        "one SHM rule for the subnet, not one per member"
    );
}

/// Same swarm shape, but spread so each host sends ONE event in its own
/// flush: in-batch cardinality is 1 per batch, so the promote gate must
/// fall through to the cumulative store dual gate (leg 3) — the store was
/// seeded by earlier flushes of the same /24.
#[test]
fn swarm_hint_reads_store_dual_gate_across_flushes() {
    let eng = engine();
    let net_ip = |h: u8| IpAddr::V4(Ipv4Addr::new(198, 18, 7, h));
    // First flush: 55 hosts x 2 events = 110 events — passes the store
    // dual gate (>=50 uniq, >=100 events) on its own in-batch legs.
    let seed: Vec<_> = (1..=55u8)
        .flat_map(|h| (0..2u64).map(move |i| ev_at(net_ip(h), i)))
        .collect();
    eng.flush_events(&seed);
    let sk = subnet_key_u128(net_ip(1)).unwrap();
    // Scope the DashMap guard: holding it across `flush_events` would
    // self-deadlock on the same shard (Phase A takes the write lock).
    let (uniq, rps) = {
        let rec = eng.store.subnet_table().get(&sk).unwrap();
        (rec.unique_ips(), rec.total_rps)
    };
    assert!(
        uniq >= 50 && rps >= 100,
        "seed must arm the store dual gate"
    );
    // Second flush: one brand-new host, one event — below promote_min, but
    // the /24 already satisfies the dual gate in the store.
    let late = ev_at(net_ip(200), 3);
    eng.flush_events(&[late]);
    assert!(
        eng.store.get(&net_ip(200)).is_some(),
        "store dual gate must promote a late sparse host into a hot /24"
    );
}

/// Patch B RED (detection seam): a swarm pacing its pulses ~3s apart must
/// still satisfy the store dual gate. Under the old 2s window each pulse
/// landed after the previous counters were zeroed, so `store_dual_gate_met`
/// stayed false and the swarm never promoted.
#[test]
fn pulsed_swarm_meets_store_dual_gate_across_window() {
    let store = Store::new(16);
    let t0 = 1_000_000_000u64;
    let mk = |o: u8| IpAddr::V4(Ipv4Addr::new(198, 18, 0, o));
    let any = mk(1);
    let net = crate::IpNetwork::of_ip(any);
    let sk = subnet_key_u128(any).unwrap();
    // Gate thresholds used by the assertions below.
    let host_threshold = 50u64;
    let event_min = 100u64;

    // Pulse 1: 30 hosts / 60 events.
    let p1: Vec<IpAddr> = (1..=30u8).map(mk).collect();
    store.merge_subnet_window(sk, net, 60, Some(&p1), t0);
    let after_p1 = store.subnet_table().get(&sk).unwrap().unique_ips();
    assert!(
        after_p1 < host_threshold,
        "pulse 1 alone must be below the host gate (got {after_p1})"
    );

    // Pulse 2, 3s later — inside a 4s window, past a 2s one.
    let p2: Vec<IpAddr> = (31..=60u8).map(mk).collect();
    store.merge_subnet_window(sk, net, 60, Some(&p2), t0 + 3_000_000_000);

    let rec = store.subnet_table().get(&sk).unwrap();
    assert!(
        rec.unique_ips() >= host_threshold && rec.total_rps >= event_min,
        "pulsed swarm must hold the dual gate across the window \
             (hosts={}, events={})",
        rec.unique_ips(),
        rec.total_rps
    );
}

/// F1: window rollover de-arms — quiet subnet resets both counters.
#[test]
fn window_rollover_resets_counters() {
    let store = Store::new(16);
    let t0 = 1_000_000_000;
    let mk = |o: u8| IpAddr::V4(Ipv4Addr::new(192, 0, 2, o));
    let any = mk(9);
    let net = crate::IpNetwork::of_ip(any);
    let sk = subnet_key_u128(any).unwrap();
    // 60 distinct hosts in one window
    let hosts: Vec<std::net::IpAddr> = (1..=60u8).map(mk).collect();
    store.merge_subnet_window(sk, net, 480, Some(&hosts), t0);
    assert_eq!(store.subnet_table().get(&sk).unwrap().unique_ips(), 60);
    // next window (past SUBNET_WINDOW_NS): fresh attacker or benign traffic
    // starts clean. The gap must exceed the window or the merge lands
    // inside it and accumulates instead of resetting.
    store.merge_subnet_window(
        sk,
        net,
        3,
        Some(&[mk(200)]),
        t0 + SUBNET_WINDOW_NS + 1_000_000_000,
    );
    let rec = store.subnet_table().get(&sk).unwrap();
    assert_eq!(
        rec.unique_ips(),
        1,
        "stale swarm signal must not survive rollover"
    );
    assert_eq!(rec.total_rps, 3);
}

/// P0 regression: bloom must clear periodically — without clear, every bit
/// becomes set within hours and cold-skip stops skipping anything,
/// ballooning the store to O(total_ips_ever_seen).
#[test]
fn bloom_clear_resets_all_bits() {
    let mut bf = BloomFilter::new(1024);
    let ip: IpAddr = "192.168.0.1".parse().unwrap();
    bf.insert(ip);
    assert!(bf.contains(ip));
    bf.clear();
    assert!(!bf.contains(ip), "clear() must reset the filter to empty");
}

/// P0 regression: detection flush must record the per-flush promoted count
/// (not the total store size) in the metrics counter. Without this fix the
/// dashboard shows the cumulative store size, which is unrelated to
/// per-window throughput and makes the metric meaningless.
#[test]
fn flush_records_per_flush_promoted_count() {
    use ramshield_config::Config;
    let cfg = Config::default();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, _erx) = mpsc::channel(64);
    let shutdown = Arc::new(AtomicBool::new(false));
    // Tighten thresholds so the synthetic traffic is hot enough to
    // promote without flapping into blocks.
    let mut cfg = cfg;
    cfg.detection.promote_min_events = 2;
    cfg.detection.subnet_window_threshold = 1;
    let eng = Arc::new(
        DetectionEngine::try_new(store.clone(), cfg.into_handle(), etx, metrics, shutdown)
            .expect("detection try_new"),
    );
    // 3 distinct /24s, each with a hot IP, in a SINGLE flush.
    let events: Vec<_> = (0..3u8)
        .flat_map(|n| {
            let ip = IpAddr::V4(Ipv4Addr::new(10, 20, 30, n + 1));
            (0..5).map(move |i| ConnectionEvent {
                ip,
                timestamp_ns: i,
                bytes: 64,
                status_code: 200,
                proto_fingerprint: 0,
                l7: None,
            })
        })
        .collect();
    eng.flush_events(&events);
    let promoted = eng.store.traffic.promoted_ips.load(Ordering::Relaxed);
    // promote_min_events=2, 3 IPs each with 5 events, all get promoted.
    // The metric should report 3, not store.len() which could be larger
    // or smaller depending on prior state.
    assert_eq!(
        promoted, 3,
        "promoted_ips counter must reflect this flush's promoted count, not store.len()"
    );
}

/// RED: Wenn update_ip wegen erschöpfter RAM-Budget ein net-new Key
/// ablehnt (stored=false), darf die IP weder als promoted gezählt noch
/// in den subnet_index aufgenommen werden — und der Vorfall muss in
/// Metrics sichtbar sein. Bisher: stillschweigendes Verschwinden.
#[test]
fn capacity_exceeded_is_tracked_and_not_promoted() {
    let cfg = Config::default().into_handle();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, _erx) = mpsc::channel(64);
    let eng = Arc::new(
        DetectionEngine::try_new(
            store.clone(),
            cfg,
            etx,
            metrics.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("detection try_new"),
    );
    // Budget künstlich erschöpfen: ram_bytes weit über jedes Limit heben.
    store.set_ram_bytes_for_testing(usize::MAX / 2);
    let ip: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 42, 0, 7));
    eng.flush_events(&(0..20).map(|i| ev_at(ip, i)).collect::<Vec<_>>());
    assert_eq!(
        metrics.capacity_exceeded_count.load(Ordering::Relaxed),
        1,
        "capacity exceeded muss in Metrics sichtbar sein"
    );
    assert_eq!(
        metrics.capacity_exceeded_ips.load(Ordering::Relaxed),
        1,
        "betroffene IP muss gezählt werden"
    );
    assert!(
        store.get(&ip).is_none(),
        "abgelehnte IP darf keinen Store-Eintrag erzeugen"
    );
    assert_eq!(
        store.traffic.promoted_ips.load(Ordering::Relaxed),
        0,
        "abgelehnte IP darf nicht als promoted gezählt werden"
    );
}

#[test]
fn relative_gate_uses_prior_baseline_and_requires_streak() {
    let mut cfg = Config::default();
    cfg.detection.rps_threshold = 1_000_000;
    cfg.detection.promote_min_events = 1;
    cfg.detection.relative_enabled = true;
    cfg.detection.relative_factor = 2.0;
    cfg.detection.relative_floor_rps = 1.0;
    cfg.detection.relative_min_samples = 3;
    cfg.detection.relative_min_breaches = 2;
    let handle = cfg.into_handle();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, mut erx) = mpsc::channel(64);
    let eng = Arc::new(
        DetectionEngine::try_new(
            store.clone(),
            handle,
            etx,
            metrics,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("detection try_new"),
    );
    let ip: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 55, 3, 3));

    // Warm baseline at ~12 rps without crossing any absolute detector.
    // The first observation seeds the slow baseline from inst_rps, not the cold-start fast EWMA.
    for round in 0..8u64 {
        let base = round * 1_000_000_000;
        let events: Vec<_> = (0..10)
            .map(|i| ConnectionEvent {
                ip,
                timestamp_ns: base + i * 90_000_000,
                bytes: 64,
                status_code: 200,
                proto_fingerprint: 0,
                l7: None,
            })
            .collect();
        eng.flush_events(&events);
    }
    let mut n = 0;
    while erx.try_recv().is_ok() {
        n += 1;
    }
    assert_eq!(n, 0);
    let baseline_before_attack = if let Value::IpRecord(r) = store.get(&ip).unwrap() {
        r.baseline_rps
    } else {
        panic!("expected IpRecord, got something else");
    };

    // One breach must not fire: hysteresis requires two consecutive breaches.
    let base = 8_000_000_000;
    let events: Vec<_> = (0..25)
        .map(|i| ConnectionEvent {
            ip,
            timestamp_ns: base + i * 40_000_000,
            bytes: 64,
            status_code: 200,
            proto_fingerprint: 0,
            l7: None,
        })
        .collect();
    eng.flush_events(&events);
    let mut n = 0;
    while erx.try_recv().is_ok() {
        n += 1;
    }
    assert_eq!(n, 0);
    let baseline_after_first_breach = if let Value::IpRecord(r) = store.get(&ip).unwrap() {
        r.baseline_rps
    } else {
        panic!("expected IpRecord, got something else");
    };
    assert_eq!(
        baseline_after_first_breach.to_bits(),
        baseline_before_attack.to_bits(),
        "relative baseline must stay frozen during a breach streak"
    );

    // Second consecutive breach fires. Absolute threshold remains unreachable.
    let base = 9_000_000_000;
    let events: Vec<_> = (0..25)
        .map(|i| ConnectionEvent {
            ip,
            timestamp_ns: base + i * 40_000_000,
            bytes: 64,
            status_code: 200,
            proto_fingerprint: 0,
            l7: None,
        })
        .collect();
    eng.flush_events(&events);
    let mut n = 0;
    while erx.try_recv().is_ok() {
        n += 1;
    }
    assert!(n >= 1);
}

#[test]
fn relative_gate_resets_streak_on_non_breach() {
    let mut cfg = Config::default();
    cfg.detection.rps_threshold = 1_000_000;
    cfg.detection.promote_min_events = 1;
    cfg.detection.relative_enabled = true;
    cfg.detection.relative_factor = 2.0;
    cfg.detection.relative_floor_rps = 1.0;
    cfg.detection.relative_min_samples = 2;
    cfg.detection.relative_min_breaches = 2;
    let handle = cfg.into_handle();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, mut erx) = mpsc::channel(64);
    let eng = Arc::new(
        DetectionEngine::try_new(
            store.clone(),
            handle,
            etx,
            metrics,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("detection try_new"),
    );
    let ip: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 55, 4, 4));

    for round in 0..6u64 {
        let base = round * 1_000_000_000;
        let events: Vec<_> = (0..10)
            .map(|i| ConnectionEvent {
                ip,
                timestamp_ns: base + i * 90_000_000,
                bytes: 64,
                status_code: 200,
                proto_fingerprint: 0,
                l7: None,
            })
            .collect();
        eng.flush_events(&events);
    }
    let mut n = 0;
    while erx.try_recv().is_ok() {
        n += 1;
    }
    assert_eq!(n, 0);

    // breach / clear / breach: no fire because the streak must be consecutive.
    for (base, n) in [
        (6_000_000_000u64, 25usize),
        (7_000_000_000, 10),
        (8_000_000_000, 25),
    ] {
        let events: Vec<_> = (0..n)
            .map(|i| ConnectionEvent {
                ip,
                timestamp_ns: base + i as u64 * (1_000_000_000 / n as u64),
                bytes: 64,
                status_code: 200,
                proto_fingerprint: 0,
                l7: None,
            })
            .collect();
        eng.flush_events(&events);
    }
    let mut n = 0;
    while erx.try_recv().is_ok() {
        n += 1;
    }
    assert_eq!(n, 0);
}

#[test]
fn try_new_propagates_shm_open_failure() {
    use std::fs;
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, _erx) = mpsc::channel(8);
    let shutdown = Arc::new(AtomicBool::new(false));
    let cfg = Config::default().into_handle();
    // Open path that cannot be a writable SHM file: an existing directory.
    let dir = std::env::temp_dir().join(format!(
        "ramshield-det-shm-fail-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("temp dir");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        DetectionEngine::try_new_with_shm_path(store, cfg, etx, metrics, shutdown, &dir)
    }));
    assert!(
        result.is_ok(),
        "fallible SHM initialization must return Err rather than panic"
    );
    assert!(
        result.expect("panic checked above").is_err(),
        "opening a directory as SHM file must return an error"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn pending_mitigations_hard_capped_under_flood() {
    use ramshield_types::BlockReason;
    let eng = engine();
    let now = 10_000_000_000u64;

    // Simulate one batch containing many more unique decisions than the
    // test cap. The map must remain bounded after EVERY admission, not only
    // after a later flush has a chance to clean it up.
    for i in 0..200u32 {
        let ip = IpAddr::from([
            10,
            ((i >> 16) & 0xff) as u8,
            ((i >> 8) & 0xff) as u8,
            (i & 0xff) as u8,
        ]);
        eng.admit_mitigation((ip, BlockReason::HighRps), 60, now + u64::from(i));
        assert!(
            eng.pending_mitigations_len() <= PENDING_MITIGATION_CAP,
            "admission {i} exceeded cap: {}",
            eng.pending_mitigations_len()
        );
    }
    assert_eq!(eng.pending_mitigations_len(), PENDING_MITIGATION_CAP);

    // Exercise simultaneous normal/emergency-style admissions: no pair of
    // callers may race through a len-check and overshoot the strict cap.
    let workers: Vec<_> = (0..8u32)
        .map(|worker| {
            let eng = eng.clone();
            std::thread::spawn(move || {
                for i in 0..200u32 {
                    let ip =
                        IpAddr::from([11, worker as u8, ((i >> 8) & 0xff) as u8, (i & 0xff) as u8]);
                    eng.admit_mitigation(
                        (ip, BlockReason::HighRps),
                        60,
                        now + 1_000 + u64::from(worker * 200 + i),
                    );
                    assert!(
                        eng.pending_mitigations_len() <= PENDING_MITIGATION_CAP,
                        "concurrent admission exceeded cap: {}",
                        eng.pending_mitigations_len()
                    );
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("admission worker");
    }
    assert!(
        eng.pending_mitigations_len() <= PENDING_MITIGATION_CAP,
        "concurrent admissions exceeded cap: {}",
        eng.pending_mitigations_len()
    );
}
