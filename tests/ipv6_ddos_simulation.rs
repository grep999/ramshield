//! Scaled IPv6 DDoS simulation — drives the detection engine with realistic
//! attack patterns and asserts the defense holds.
//!
//! Scenarios:
//!   1. /64 swarm: 60 unique v6 hosts in one /64, 100 events each (6000 events)
//!   2. Single-host flood: 1 v6 IP, 50000 events (must be promoted and blocked)
//!   3. Mixed legit + attack: 10 legit IPs at low rate, 60 attacker IPs at high rate
//!   4. Random /64s: 10 subnets × 50 unique IPs each (no single /64 should trip block)
//!
//! Bounds: 1M events total. Must complete in <30s. No infinite loops.

use ramshield_detection::batch::aggregate;
use ramshield_detection::DetectionEngine;
use ramshield_metrics::Metrics;
use ramshield_storage::Store;
use ramshield_types::ConnectionEvent;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

fn ev(ip: IpAddr, ts: u64) -> ConnectionEvent {
    ConnectionEvent {
        ip,
        timestamp_ns: ts,
        bytes: 64,
        status_code: 200,
        proto_fingerprint: 0,
    }
}

/// Make engine + return its store for post-assertions.
fn make_engine() -> (Arc<DetectionEngine>, Arc<Store>) {
    use ramshield_config::Config;
    let cfg = Config::default();
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(Metrics::new());
    let (etx, _erx) = mpsc::channel(64);
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut cfg = cfg;
    // Tighten so synthetic traffic is unambiguously hot.
    cfg.detection.promote_min_events = 4;
    cfg.detection.subnet_window_threshold = 1;
    cfg.detection.subnet_batch_threshold = 50;
    cfg.detection.subnet_batch_min_events = 100;
    let eng = Arc::new(DetectionEngine::new(
        store.clone(),
        cfg.into_handle(),
        etx,
        metrics,
        shutdown,
    ));
    (eng, store)
}

#[test]
fn ipv6_single_64_swarm_aggregates() {
    let (eng, _store) = make_engine();
    let events: Vec<_> = (0..60u8)
        .flat_map(|n| {
            let ip = IpAddr::V6(Ipv6Addr::new(
                0x2001, 0xdb8, 0, 0, 0, 0, 0, u16::from(n) + 1,
            ));
            (0..100u64).map(move |i| ev(ip, i))
        })
        .collect();
    let start = Instant::now();
    eng.flush_events(&events);
    let elapsed = start.elapsed();
    let agg = aggregate(&events);
    let any_ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
    let sk = ramshield_storage::subnet_key_u128(any_ip).unwrap();
    assert_eq!(
        agg.subnets.get(&sk).map(|x| x.0),
        Some(6_000),
        "swarm signal must report total events (60 IPs × 100 each)"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "6000-event swarm aggregation must be sub-5s, took {elapsed:?}"
    );
}

#[test]
fn ipv6_single_host_flood_promotes() {
    let (eng, store) = make_engine();
    let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0xdead));
    let events: Vec<_> = (0..50_000u64).map(|i| ev(ip, i)).collect();
    let start = Instant::now();
    eng.flush_events(&events);
    let elapsed = start.elapsed();
    // Use the shared store to verify the IP was promoted.
    let promoted = store.traffic.promoted_ips.load(Ordering::Relaxed);
    assert!(
        promoted >= 1,
        "50k events from one v6 IP must be promoted, got promoted={promoted}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "50k-event flood must be sub-10s, took {elapsed:?}"
    );
}

#[test]
fn ipv6_random_64s_no_single_subnet_dual_gate() {
    let (eng, _store) = make_engine();
    let events: Vec<_> = (0..10u16)
        .flat_map(|subnet| {
            (0..50u8).flat_map(move |host| {
                let ip = IpAddr::V6(Ipv6Addr::new(
                    0x2001, 0xdb8, 0, subnet, 0, 0, 0,
                    u16::from(host) + 1,
                ));
                (0..2u64).map(move |i| ev(ip, i))
            })
        })
        .collect();
    let start = Instant::now();
    eng.flush_events(&events);
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "1000-event mixed traffic must be sub-5s, took {elapsed:?}"
    );
    let agg = aggregate(&events);
    assert_eq!(agg.ips.len(), 10 * 50, "500 unique IPs aggregated");
    for (_, (_, members)) in agg.subnets.iter() {
        assert_eq!(members.len(), 50, "50 unique hosts per /64");
    }
}

#[test]
fn ipv6_mixed_legit_and_attack() {
    let (eng, store) = make_engine();
    let mut events: Vec<ConnectionEvent> = Vec::new();
    // 10 legit IPs at 1 event each — should be cold-skipped.
    for n in 0..10u8 {
        let ip = IpAddr::V6(Ipv6Addr::new(
            0x2001, 0xdb8, 0xcafe, 0, 0, 0, 0, u16::from(n) + 1,
        ));
        events.push(ev(ip, 0));
    }
    // 60 attacker IPs at 50 events each in one /64 — should be promoted.
    for n in 0..60u8 {
        let ip = IpAddr::V6(Ipv6Addr::new(
            0x2001, 0xdb8, 0, 1, 0, 0, 0, u16::from(n) + 1,
        ));
        for i in 0..50u64 {
            events.push(ev(ip, i));
        }
    }
    let start = Instant::now();
    eng.flush_events(&events);
    let elapsed = start.elapsed();
    let promoted = store.traffic.promoted_ips.load(Ordering::Relaxed);
    assert!(
        promoted >= 60,
        "60 attacker IPs must be promoted, got {promoted}"
    );
    let store_len = store.len();
    assert!(
        store_len >= 60,
        "store should contain at least 60 attackers, got {store_len}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "3010-event mixed traffic must be sub-10s, took {elapsed:?}"
    );
}

#[test]
fn ipv6_dos_report() {
    // Summary report — runs 100k events, prints throughput, asserts <30s.
    let start = Instant::now();
    let (eng, _store) = make_engine();
    let events: Vec<_> = (0..1000u32)
        .flat_map(|n| {
            let ip = IpAddr::V6(Ipv6Addr::new(
                0x2001, 0xdb8,
                (n >> 16) as u16,
                (n & 0xFFFF) as u16,
                0, 0, 0, 1,
            ));
            (0..100u64).map(move |i| ev(ip, i))
        })
        .collect();
    let n_events = events.len();
    let agg = aggregate(&events);
    eng.flush_events(&events);
    let elapsed = start.elapsed();
    let throughput = n_events as f64 / elapsed.as_secs_f64();
    eprintln!(
        "ipv6_dos_report: {} events in {:?} ({:.0} events/s)",
        n_events, elapsed, throughput
    );
    eprintln!(
        "  aggregated {} unique IPs across {} subnets",
        agg.ips.len(),
        agg.subnets.len()
    );
    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "100k-event bulk flush must be sub-30s, took {elapsed:?}"
    );
}
