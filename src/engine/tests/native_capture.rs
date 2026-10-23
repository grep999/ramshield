//! The native capture worker lifecycle.
//!
//! F-2. The capture workers were created inside the supervisor thread,
//! so their spawn results were swallowed with
//! `handles.into_iter().flatten()` and the daemon reached ready state
//! with zero capture workers running. A worker that failed logged a
//! `warn!` and vanished; a worker that panicked was dropped with
//! `let _ = handle.join()`.
//!
//! Every test here is deterministic: the spawner and the capture attempt
//! are parameters, so a partial startup failure, a total failure, a
//! runtime exit, a shutdown and a retry exhaustion are all produced
//! without a NIC, a capability or a race.

use super::Engine;
use super::native::{MAX_CAPTURE_ATTEMPTS, SlotOutcome, capture_slot, join_workers, start_workers};
use crate::Config;
use crate::metrics::{Metrics, ProtectionState};
use crate::storage::Store;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// A worker that does nothing and exits cleanly.
fn idle_worker() -> std::io::Result<std::thread::JoinHandle<()>> {
    Ok(std::thread::spawn(|| {}))
}

#[test]
fn a_partial_worker_startup_failure_is_counted_not_swallowed() {
    let result = start_workers(4, |worker| {
        if worker == 2 {
            Err(std::io::Error::other("thread creation refused"))
        } else {
            idle_worker()
        }
    });
    assert_eq!(
        result.started, 3,
        "three of the four workers must have started"
    );
    assert_eq!(
        result.failed, 1,
        "the failed spawn must be counted, not dropped"
    );
    assert_eq!(
        result.handles.len(),
        3,
        "only the started workers have a handle"
    );
    for handle in result.handles {
        handle.join().unwrap();
    }
}

#[test]
fn a_total_worker_startup_failure_is_reported() {
    let result = start_workers(4, |_worker| {
        Err(std::io::Error::other("thread creation refused"))
    });
    assert_eq!(result.started, 0, "no worker started");
    assert_eq!(result.failed, 4, "every failed spawn is counted");
    assert!(result.handles.is_empty(), "a failed spawn has no handle");
}

#[test]
fn a_panicking_worker_is_reported_not_swallowed() {
    let handles = vec![
        std::thread::spawn(|| {}),
        std::thread::spawn(|| panic!("worker died")),
    ];
    let panicked = join_workers(handles);
    assert_eq!(panicked, 1, "the panicking worker must be counted");
}

#[test]
fn a_runtime_worker_failure_is_retried_up_to_the_limit() {
    let shutdown = AtomicBool::new(false);
    let outcome = capture_slot(
        MAX_CAPTURE_ATTEMPTS,
        Duration::from_millis(0),
        &shutdown,
        |_| Err(std::io::Error::other("capture setup failed")),
    );
    assert_eq!(outcome.attempts, MAX_CAPTURE_ATTEMPTS);
    assert_eq!(outcome.failures, MAX_CAPTURE_ATTEMPTS);
    assert!(!outcome.stopped_by_shutdown);
}

#[test]
fn a_transient_worker_failure_is_retried() {
    let shutdown = AtomicBool::new(false);
    let mut calls = 0u32;
    let outcome = capture_slot(
        MAX_CAPTURE_ATTEMPTS,
        Duration::from_millis(0),
        &shutdown,
        |_| {
            calls += 1;
            if calls == 1 {
                Err(std::io::Error::other("capture setup failed"))
            } else {
                Ok(())
            }
        },
    );
    assert_eq!(
        outcome.attempts, 2,
        "the slot retries once and then succeeds"
    );
    assert_eq!(outcome.failures, 1);
    assert!(!outcome.stopped_by_shutdown);
}

#[test]
fn a_clean_capture_end_stops_the_slot() {
    let shutdown = AtomicBool::new(false);
    let outcome = capture_slot(
        MAX_CAPTURE_ATTEMPTS,
        Duration::from_millis(0),
        &shutdown,
        |_| Ok(()),
    );
    assert_eq!(outcome.attempts, 1, "a clean end is not retried");
    assert_eq!(outcome.failures, 0);
}

#[test]
fn shutdown_stops_the_retry_loop() {
    let shutdown = AtomicBool::new(false);
    let outcome = capture_slot(
        MAX_CAPTURE_ATTEMPTS,
        Duration::from_millis(0),
        &shutdown,
        |_| {
            // The daemon asked to stop while the attempt was failing.
            shutdown.store(true, Ordering::Release);
            Err(std::io::Error::other("capture setup failed"))
        },
    );
    assert_eq!(outcome.attempts, 1, "no second attempt after the shutdown");
    assert_eq!(outcome.failures, 1);
    assert!(outcome.stopped_by_shutdown);
}

#[test]
fn shutdown_before_the_first_attempt_stops_the_slot() {
    let shutdown = AtomicBool::new(true);
    let outcome = capture_slot(
        MAX_CAPTURE_ATTEMPTS,
        Duration::from_millis(0),
        &shutdown,
        |_| panic!("the capture must not be attempted after the shutdown"),
    );
    assert_eq!(outcome.attempts, 0);
    assert_eq!(outcome.failures, 0);
    assert!(outcome.stopped_by_shutdown);
}

#[test]
fn the_slot_outcome_is_a_plain_value() {
    // The outcome is what the production path reports, so it has to be
    // comparable without a lock.
    let a = SlotOutcome {
        attempts: 1,
        failures: 0,
        stopped_by_shutdown: false,
    };
    let b = a;
    assert_eq!(a, b);
}

/// The owner's decision: a dead native capture degrades the service and
/// says so; it does not refuse readiness. Capture is observation, not
/// enforcement - detection has a second event source and XDP/netfilter
/// remain the enforcement layers - so a telemetry fault must not become
/// an enforcement outage.
#[test]
fn a_dead_native_capture_degrades_the_service_without_refusing_readiness() {
    let mut cfg = Config::default();
    cfg.native_ingest.enabled = true;
    let metrics = Arc::new(Metrics::new());
    // The capture path started and every slot is gone.
    metrics.set_native_capture_workers_expected(8);
    metrics.set_native_capture_workers_started(8);
    let engine = Engine::new(cfg, Arc::new(Store::new(16)), metrics);
    engine.mark_pipeline_ready_for_test();

    let snapshot = engine.dashboard_snapshot();
    assert_eq!(
        snapshot.protection_state,
        ProtectionState::Degraded,
        "a dead capture must show as degraded"
    );
    assert_eq!(snapshot.health_reason, "native capture stopped");
    assert!(
        snapshot.is_healthy,
        "a dead capture must not take the daemon out of service"
    );
    assert_eq!(snapshot.native_capture_workers_running, 0);
}

/// Before the capture path starts, the daemon is not "degraded": the
/// expected count is zero, so there is nothing to be missing yet.
#[test]
fn a_capture_path_that_has_not_started_is_not_degraded() {
    let mut cfg = Config::default();
    cfg.native_ingest.enabled = true;
    let engine = Engine::new(cfg, Arc::new(Store::new(16)), Arc::new(Metrics::new()));
    engine.mark_pipeline_ready_for_test();

    let snapshot = engine.dashboard_snapshot();
    assert_ne!(
        snapshot.protection_state,
        ProtectionState::Degraded,
        "the daemon must not read as degraded before the capture path starts"
    );
}
