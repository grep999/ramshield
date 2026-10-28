//! Month-3 gate: deterministic replay budgets. Fixed synthetic event streams;
//! the only real-clock quantity is wall latency, which is the thing being
//! budgeted. Budgets:
//!   - detection latency: a 600-event single-IP burst (emergency threshold
//!     500) must produce a block decision within 2s end-to-end.
//!   - false positives: 300 distinct benign IPs at 1 event each must produce
//!     ZERO block decisions (below every gate: promote gate needs 4,
//!     subnet dual gate needs 64 hosts AND 500 events).

use ramshield_config::Config;
use ramshield_storage::Store;
use ramshield_types::ConnectionEvent;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

fn make_engine(
    cfg: Config,
) -> (
    Arc<ramshield_detection::DetectionEngine>,
    mpsc::Receiver<ramshield_types::EnforceCommand>,
    Arc<ramshield_metrics::Metrics>,
    Arc<Store>,
) {
    let store = Arc::new(Store::new(16));
    let store_ret = store.clone();
    let metrics = Arc::new(ramshield_metrics::Metrics::new());
    let (etx, erx) = mpsc::channel(4096);
    let engine = Arc::new(
        ramshield_detection::DetectionEngine::try_new(
            store,
            Arc::new(arc_swap::ArcSwap::from_pointee(cfg)),
            etx,
            metrics.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("detection try_new"),
    );
    (engine, erx, metrics, store_ret)
}

fn ev(ip: IpAddr, ts: u64) -> ConnectionEvent {
    ConnectionEvent {
        ip,
        timestamp_ns: ts,
        bytes: 512,
        status_code: 200,
        proto_fingerprint: 0,
        l7: None,
    }
}

#[test]
fn emergency_burst_block_latency_within_budget() {
    let (engine, mut erx, _metrics, _store) = make_engine(Config::default());
    engine.clone().spawn_workers(1);
    let _event_tx = engine.event_sender();

    let target: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 9, 9, 9));
    let events: Vec<ConnectionEvent> = (0..600u64).map(|i| ev(target, i)).collect();
    let sender = engine.event_sender();

    let start = Instant::now();
    for e in &events {
        sender.send(e.clone()).expect("event channel");
    }

    let cmd = erx
        .blocking_recv()
        .expect("emergency block must be emitted");
    let latency = start.elapsed();
    println!("emergency block latency: {latency:?} (budget 2s)");
    assert!(matches!(cmd.action, ramshield_types::EnforceAction::Block));
    assert_eq!(cmd.ip, target);
    assert!(
        latency < Duration::from_secs(2),
        "block decision took {latency:?}; budget is 2s"
    );
}

#[test]
fn benign_traffic_produces_zero_false_positives() {
    let (engine, mut erx, metrics, store) = make_engine(Config::default());
    // 300 distinct benign IPs, one 200-OK event each.
    let events: Vec<ConnectionEvent> = (0..300u64)
        .map(|i| {
            ev(
                IpAddr::V4(Ipv4Addr::new(192, 0, (i / 256) as u8, (i % 256) as u8)),
                i,
            )
        })
        .collect();
    engine.flush_events(&events);

    assert!(
        erx.try_recv().is_err(),
        "benign single-event IPs must never be blocked"
    );
    assert_eq!(
        metrics.blocks_detection.load(Ordering::Relaxed),
        0,
        "blocks_detection must stay at zero for benign traffic"
    );
    // The events still land: the IP is tracked in the store as an IpRecord.
    let sample = IpAddr::V4(Ipv4Addr::new(192, 0, 0, 7));
    assert!(
        store.get(&sample).is_some(),
        "benign events must be ingested, not dropped"
    );
}
