//! BACKLOG #14 — engine startup integration tests.
//! Lives in-tree (not `tests/`) so it can reach crate-private engine
//! internals; rides `cargo test --lib`.
use super::*;
use crate::Config;
use crate::metrics::Metrics;
use crate::storage::Store;
use std::sync::Arc;

#[tokio::test]
async fn startup_runtime_failure_is_reported_to_waiter() {
    let engine = Engine::new(
        Config::default(),
        Arc::new(Store::new(16)),
        Arc::new(Metrics::new()),
    );

    engine.fail_startup(std::io::Error::other("runtime initialization failed"));

    assert!(
        engine.pipeline_failed.load(std::sync::atomic::Ordering::Acquire),
        "startup failure must mark the pipeline failed"
    );
    let err = engine
        .wait_startup()
        .await
        .expect_err("startup waiter must receive the runtime initialization failure");
    assert!(err.to_string().contains("runtime initialization failed"));
}

#[test]
fn engine_constructs_with_default_config() {
    let _engine = Engine::new(
        Config::default(),
        Arc::new(Store::new(16)),
        Arc::new(Metrics::new()),
    );
}

#[test]
fn engine_start_then_snapshot_default_state() {
    let engine = Engine::new(
        Config::default(),
        Arc::new(Store::new(16)),
        Arc::new(Metrics::new()),
    );
    #[allow(deprecated)]
    engine.start();
    engine.mark_pipeline_ready_for_test();
    let snap = engine.dashboard_snapshot();
    assert!(snap.is_healthy);
    assert_eq!(snap.ips_tracked, 0);
    assert_eq!(snap.blocked_total, 0);
    assert_eq!(snap.events_ingested, 0);
}

#[test]
fn engine_module_stats_have_four_canonical_rows() {
    let engine = Engine::new(
        Config::default(),
        Arc::new(Store::new(16)),
        Arc::new(Metrics::new()),
    );
    #[allow(deprecated)]
    engine.start();
    let stats = engine.get_module_stats();
    assert_eq!(stats.len(), 7);
    let labels: Vec<&str> = stats.iter().map(|m| m.label.as_str()).collect();
    assert!(labels.contains(&"IPC"));
    assert!(labels.contains(&"Detection"));
    assert!(labels.contains(&"Forecasting"));
    assert!(labels.contains(&"Storage"));
}

#[test]
fn engine_snapshot_unhealthy_when_shutting_down() {
    let engine = Engine::new(
        Config::default(),
        Arc::new(Store::new(16)),
        Arc::new(Metrics::new()),
    );
    engine.shutdown();
    let snap = engine.dashboard_snapshot();
    assert!(!snap.is_healthy);
    assert_eq!(snap.health_reason, "shutting down");
}

#[test]
fn engine_snapshot_unhealthy_until_pipeline_ready() {
    let engine = Engine::new(
        Config::default(),
        Arc::new(Store::new(16)),
        Arc::new(Metrics::new()),
    );
    let snap = engine.dashboard_snapshot();
    assert!(!snap.is_healthy);
    assert_eq!(snap.health_reason, "starting");
    engine.mark_pipeline_ready_for_test();
    let snap = engine.dashboard_snapshot();
    assert!(snap.is_healthy);
    assert_eq!(snap.health_reason, "running");
    engine.pipeline_failed.store(true, Ordering::Release);
    let snap = engine.dashboard_snapshot();
    assert!(!snap.is_healthy);
    assert_eq!(snap.health_reason, "pipeline failed");
}

#[test]
fn engine_snapshot_marks_stale_active_xdp_projection_unhealthy() {
    let mut cfg = Config::default();
    cfg.xdp.enabled = true;
    cfg.xdp.allow_inband_fallback = true;
    let engine = Engine::new(cfg, Arc::new(Store::new(16)), Arc::new(Metrics::new()));
    engine.mark_pipeline_ready_for_test();
    engine.xdp_active.store(true, Ordering::Release);
    engine.metrics.set_xdp_projection_active(true);

    // No successful reconciliation has occurred: active configured XDP
    // therefore has an unknown/stale userspace→kernel projection.
    let snap = engine.dashboard_snapshot();
    assert!(snap.xdp_projection_stale);
    assert!(!snap.is_healthy);
    assert_eq!(
        snap.protection_state,
        crate::metrics::ProtectionState::Degraded
    );
    assert_eq!(snap.health_reason, "xdp reconciliation stale");

    let prom = engine.metrics.render_prometheus();
    assert!(prom.contains("ramshield_xdp_projection_stale 1"));
}

#[test]
fn engine_snapshot_unhealthy_when_ram_pressure() {
    // RED: set ram_limit_mb=1 MB and ram_bytes = 1.5 MB → ram_pct > 95%.
    // Broken code (8c159cc): is_healthy stays true. Fixed code: flips to false.
    let store = Arc::new(Store::new(16));
    store.set_ram_limit_mb_for_testing(1);
    store.set_ram_bytes_for_testing(1_572_864); // 1.5 MB > 1 MB → ram_pct = 100.0
    let engine = Engine::new(Config::default(), store, Arc::new(Metrics::new()));
    engine.mark_pipeline_ready_for_test();
    let snap = engine.dashboard_snapshot();
    assert!(
        !snap.is_healthy,
        "is_healthy should flip false at ram_pct=100%"
    );
    assert_eq!(snap.health_reason, "ram pressure");
}

fn blocked_record(ip: std::net::IpAddr) -> crate::storage::IpRecord {
    use crate::BlockReason;
    use crate::storage::{BlockState, IpRecord};
    IpRecord {
        ip,
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
            reason: BlockReason::HighRps,
            since_ns: 0,
        },
    }
}

#[test]
fn active_blocks_emit_canonical_reason_tokens() {
    // RED: format!("{reason:?}") emitted "HighRps" from /status while every
    // other boundary emits the canonical token "high_rps" (BlockReason::as_str).
    let store = Arc::new(Store::new(16));
    store
        .insert(
            "10.9.9.9".parse().unwrap(),
            crate::storage::Value::IpRecord(blocked_record("10.9.9.9".parse().unwrap())),
            None,
            1 << 30,
        )
        .unwrap();
    let engine = Engine::new(Config::default(), store, Arc::new(Metrics::new()));
    let blocks = engine.get_active_blocks();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].reason, "high_rps");
}
