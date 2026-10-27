//! Month-4 gate: memory bounds.
//!   - Exhausted RAM budget: net-new keys are REFUSED (never evict live
//!     records), and the refusal is observable via capacity_exceeded_count.
//! High-cardinality bound test lives in the detection crate's lib tests
//! (merge_local/pre_aggs are crate-internal by design).
//! Bounded soak: concurrent_flush_never_loses_events churns the same
//! structures; a wall-clock 24h soak is a scheduled job, not a unit test.

use ramshield_config::Config;
use ramshield_detection::DetectionEngine;
use ramshield_storage::Store;
use ramshield_types::ConnectionEvent;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;

#[test]
fn exhausted_ram_budget_refuses_net_new_and_is_observable() {
    let mut cfg = Config::default();
    cfg.engine.ram_limit_mb = 0; // budget exhausted from the first net-new key
    let store = Arc::new(Store::new(16));
    let metrics = Arc::new(ramshield_metrics::Metrics::new());
    let (etx, _erx) = mpsc::channel(4096);
    let det = Arc::new(
        DetectionEngine::try_new(
            store.clone(),
            Arc::new(arc_swap::ArcSwap::from_pointee(cfg)),
            etx,
            metrics.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("detection try_new"),
    );

    let events: Vec<ConnectionEvent> = (0..50u64)
        .map(|i| ConnectionEvent {
            ip: IpAddr::V4(Ipv4Addr::new(10, 77, 0, i as u8)),
            timestamp_ns: i,
            bytes: 64,
            status_code: 200,
            proto_fingerprint: 0,
            l7: None,
        })
        .collect();
    det.flush_events(&events);

    let exceeded = metrics.capacity_exceeded_count.load(Ordering::Relaxed);
    assert!(
        exceeded > 0,
        "RAM-budget exhaustion must surface in capacity_exceeded_count"
    );
    // Budget 0: every net-new key refused, nothing stored.
    assert_eq!(store.len(), 0, "budget 0 must refuse every net-new key");
}
