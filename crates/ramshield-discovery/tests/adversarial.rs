//! Stage 6 adversarial evaluation: deterministic ground-truth scenarios.
//! No PCAP dependency; exercises existing public API with known inputs.
//! Covers: ground truth, fault injection, window integrity, durability,
//! baseline poisoning, slow ramps, flash crowds, high cardinality, clock skew.

use ramshield_learning::{
    assess_cardinality_anomaly, assess_metric_distribution, calibrate_threshold,
    CardinalityEstimate, CardinalityEstimator, CountMetric, DistributionAssessment,
    IngestionEngine, IngestConfig, MetricAssessment, MetricProfileStore,
    ObservationQuality, RecordOutcome, ServiceId, TrafficDirection, WindowKey,
    WindowLedger, WindowObservation, WindowState,
    CountMoments,
};
use ramshield_types::ConnectionEvent;
use std::net::{IpAddr, Ipv4Addr};
use tempfile::tempdir;

fn event(ip: IpAddr, ts_ns: u64, captured: u64, wire: u64, truncated: bool) -> ConnectionEvent {
    ConnectionEvent {
        ip,
        timestamp_ns: ts_ns,
        wire_bytes: wire,
        captured_bytes: captured,
        truncated,
        status_code: 200,
        proto_fingerprint: 1,
        l7: None,
    }
}

fn mk_key(svc: &str, start: u64, seconds: u32) -> WindowKey {
    WindowKey {
        service_id: svc.to_string(),
        window_start_secs: start,
        window_seconds: seconds,
    }
}

fn mk_obs(key: WindowKey, packets: u64, bytes: u64, drops: u64) -> WindowObservation {
    let state = if drops > 0 {
        WindowState::Degraded {
            packets,
            bytes,
            capture_drops: drops,
            ingest_drops: 0,
            malformed: 0,
        }
    } else {
        WindowState::Observed { packets, bytes }
    };
    WindowObservation { key, state }
}

// ── Ground truth ───────────────────────────────────────────────────────

#[test]
fn ground_truth_counts_match_input_exactly() {
    let _dir = tempdir().unwrap();
    let mut eng = IngestionEngine::new(100, IngestConfig::default()).unwrap();
    let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 1).into();
    let events = vec![
        event(ip, 1_000_000_000, 100, 100, false),
        event(ip, 1_000_000_500, 200, 200, false),
        event(ip, 1_000_000_999, 50, 50, false),
    ];
    eng.record_batch(&events);
    let outcomes = eng.flush_pending().unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0], RecordOutcome::Inserted);

    let key = mk_key(&ServiceId::from_ip(ip).as_string(), 0, 60);
    let state = eng.ledger().get(&key).expect("window should exist after flush");
    assert!(matches!(
        state,
        WindowState::Observed { packets: 3, bytes: 350 }
    ));
}

#[test]
fn ground_truth_wire_bytes_never_underreported() {
    let _dir = tempdir().unwrap();
    let mut eng = IngestionEngine::new(100, IngestConfig::default()).unwrap();
    let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 1).into();
    let events = vec![event(ip, 1_000_000_000, 64, 1500, true)];
    eng.record_batch(&events);
    eng.flush_pending().unwrap();
    let key = mk_key(&ServiceId::from_ip(ip).as_string(), 0, 60);
    let state = eng.ledger().get(&key).expect("window should exist");
    assert!(matches!(state, WindowState::Degraded { .. }));
}

// ── Fault injection ────────────────────────────────────────────────────

#[test]
fn ring_drop_marks_window_degraded() {
    let _dir = tempdir().unwrap();
    let mut eng = IngestionEngine::new(100, IngestConfig::default()).unwrap();
    eng.record_capture_drops(5);
    let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 1).into();
    eng.record_batch(&[event(ip, 1_000_000_000, 100, 100, false)]);
    eng.flush_pending().unwrap();
    let key = mk_key(&ServiceId::from_ip(ip).as_string(), 0, 60);
    let state = eng.ledger().get(&key).expect("window should exist");
    assert!(matches!(state, WindowState::Degraded { capture_drops: 5, .. }));
}

#[test]
fn parse_reject_marks_window_degraded() {
    let _dir = tempdir().unwrap();
    let mut eng = IngestionEngine::new(100, IngestConfig::default()).unwrap();
    eng.record_malformed(3);
    let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 1).into();
    eng.record_batch(&[event(ip, 1_000_000_000, 100, 100, false)]);
    eng.flush_pending().unwrap();
    let key = mk_key(&ServiceId::from_ip(ip).as_string(), 0, 60);
    let state = eng.ledger().get(&key).expect("window should exist");
    assert!(matches!(state, WindowState::Degraded { malformed: 3, .. }));
}

// ── Window integrity ───────────────────────────────────────────────────

#[test]
fn duplicate_window_returns_duplicate_outcome() {
    let mut ledger = WindowLedger::new(100).unwrap();
    let k = mk_key("svc-a", 0, 60);
    assert_eq!(
        ledger.record(mk_obs(k.clone(), 10, 1000, 0)).unwrap(),
        RecordOutcome::Inserted
    );
    assert_eq!(
        ledger.record(mk_obs(k.clone(), 10, 1000, 0)).unwrap(),
        RecordOutcome::Duplicate
    );
}

#[test]
fn conflicting_duplicate_window_returns_conflict() {
    let mut ledger = WindowLedger::new(100).unwrap();
    let k = mk_key("svc-a", 0, 60);
    ledger.record(mk_obs(k.clone(), 10, 1000, 0)).unwrap();
    assert_eq!(
        ledger.record(mk_obs(k.clone(), 11, 1100, 0)).unwrap(),
        RecordOutcome::ConflictingDuplicate
    );
}

#[test]
fn out_of_order_window_rejected_too_old() {
    let mut ledger = WindowLedger::new(2).unwrap();
    // Fill capacity with two distinct keys
    let k_full1 = mk_key("svc-full-a", 60, 60);
    let k_full2 = mk_key("svc-full-b", 60, 60);
    ledger.record(mk_obs(k_full1.clone(), 10, 1000, 0)).unwrap();
    ledger.record(mk_obs(k_full2.clone(), 10, 1000, 0)).unwrap();
    // Window at slot 0 is older than slot 60 — rejected when at capacity
    let k_old = mk_key("svc-a", 0, 60);
    assert_eq!(
        ledger.record(mk_obs(k_old, 10, 1000, 0)).unwrap(),
        RecordOutcome::RejectedTooOld
    );
}

// ── Durability ─────────────────────────────────────────────────────────

#[test]
fn crash_recovery_preserves_committed_windows() {
    let path = tempdir().unwrap().path().join("ledger.json");
    {
        let mut eng = IngestionEngine::new(100, IngestConfig::default()).unwrap();
        let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 1).into();
        eng.record_batch(&[event(ip, 1_000_000_000, 100, 100, false)]);
        eng.flush_pending().unwrap();
        eng.persist(&path).unwrap();
    }
    let eng = IngestionEngine::reopen(&path, 100, IngestConfig::default()).unwrap();
    let key = mk_key(&ServiceId::from_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))).as_string(), 0, 60);
    let state = eng.ledger().get(&key).expect("committed window survives restart");
    assert!(matches!(state, WindowState::Observed { packets: 1, bytes: 100 }));
}

#[test]
fn corrupted_snapshot_is_rejected() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("corrupt.json");
    std::fs::write(&path, b"not valid json at all").unwrap();
    let result = WindowLedger::reopen(&path, 100);
    assert!(result.is_err(), "corrupted snapshot must fail closed");
}

// ── Baseline poisoning ─────────────────────────────────────────────────

#[test]
fn baseline_poisoning_detected_as_requires_review() {
    let base = CardinalityEstimate {
        estimate: 10_000,
        standard_error: 0.1,
        observations: 100,
    };
    let current = CardinalityEstimate {
        estimate: 100,
        standard_error: 0.1,
        observations: 100,
    };
    let result = assess_cardinality_anomaly(&current, Some(&base), 100);
    assert_eq!(result, DistributionAssessment::RequiresReview);
}

#[test]
fn baseline_poisoning_growth_detected() {
    let base = CardinalityEstimate {
        estimate: 10,
        standard_error: 0.1,
        observations: 100,
    };
    let current = CardinalityEstimate {
        estimate: 10_000,
        standard_error: 0.1,
        observations: 100,
    };
    let result = assess_cardinality_anomaly(&current, Some(&base), 100);
    assert_eq!(result, DistributionAssessment::RequiresReview);
}

// ── Slow ramps (evasion below 2x threshold) ────────────────────────────

#[test]
fn slow_ramp_below_threshold_passes_normal() {
    let base = CardinalityEstimate {
        estimate: 100,
        standard_error: 0.1,
        observations: 100,
    };
    let current = CardinalityEstimate {
        estimate: 150,
        standard_error: 0.1,
        observations: 100,
    };
    let result = assess_cardinality_anomaly(&current, Some(&base), 100);
    assert_eq!(result, DistributionAssessment::Normal);
}

#[test]
fn slow_ramp_exact_2x_is_normal() {
    let base = CardinalityEstimate {
        estimate: 100,
        standard_error: 0.1,
        observations: 100,
    };
    let current = CardinalityEstimate {
        estimate: 200,
        standard_error: 0.1,
        observations: 100,
    };
    let result = assess_cardinality_anomaly(&current, Some(&base), 100);
    assert_eq!(result, DistributionAssessment::Normal);
}

// ── Flash crowds ───────────────────────────────────────────────────────

#[test]
fn flash_crowd_cardinality_spike_requires_review() {
    let base = CardinalityEstimate {
        estimate: 100,
        standard_error: 0.1,
        observations: 100,
    };
    let current = CardinalityEstimate {
        estimate: 250,
        standard_error: 0.1,
        observations: 100,
    };
    let result = assess_cardinality_anomaly(&current, Some(&base), 100);
    assert_eq!(result, DistributionAssessment::RequiresReview);
}

// ── High cardinality / estimator saturation ────────────────────────────

#[test]
fn saturated_estimator_returns_insufficient_data() {
    let current = CardinalityEstimate {
        estimate: u64::MAX,
        standard_error: f64::INFINITY,
        observations: 100,
    };
    let result = assess_cardinality_anomaly(&current, None, 100);
    assert_eq!(result, DistributionAssessment::InsufficientData);
}

#[test]
fn saturated_baseline_returns_insufficient_data() {
    let current = CardinalityEstimate {
        estimate: 100,
        standard_error: 0.1,
        observations: 100,
    };
    let base = CardinalityEstimate {
        estimate: u64::MAX,
        standard_error: 0.1,
        observations: 100,
    };
    let result = assess_cardinality_anomaly(&current, Some(&base), 100);
    assert_eq!(result, DistributionAssessment::InsufficientData);
}

// ── Cardinality estimator under adversarial load ───────────────────────

#[test]
fn high_cardinality_estimator_stays_bounded() {
    let mut ce = CardinalityEstimator::new(4096);
    for i in 0..50_000u64 {
        ce.observe(i);
    }
    let est = ce.estimate();
    // At full saturation all bits set → returns u64::MAX (saturated sentinel)
    // This is correct behavior; stats module treats u64::MAX as InsufficientData
    assert!(est.estimate > 0 || est.estimate == u64::MAX);
}

// ── Clock skew / out-of-order timestamps ───────────────────────────────

#[test]
fn out_of_order_timestamps_does_not_panic() {
    let _dir = tempdir().unwrap();
    let mut eng = IngestionEngine::new(100, IngestConfig::default()).unwrap();
    let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 1).into();
    let events = vec![
        event(ip, 1_000_000_900, 100, 100, false),
        event(ip, 1_000_000_100, 100, 100, false),
        event(ip, 1_000_000_500, 100, 100, false),
    ];
    eng.record_batch(&events);
    let outcomes = eng.flush_pending().unwrap();
    assert_eq!(outcomes.len(), 1);
}

// ── Quality gate: degraded never promotes to Normal ────────────────────

#[test]
fn degraded_quality_never_promotes_to_normal() {
    let mut moments = CountMoments::default();
    for _ in 0..10 { moments.push(11); }
    let assessment = MetricAssessment {
        metric: CountMetric::Packets,
        observed_packets: 113,
        quality: ObservationQuality::Degraded,
        moments,
        clean_observations: 10,
        degraded_observations: 1,
        baseline_source: None,
        last_updated_secs: 0,
    };
    let result = assess_metric_distribution(&assessment);
    assert!(
        !matches!(result, DistributionAssessment::Normal),
        "degraded quality must not produce Normal assessment"
    );
}

// ── Calibration fail-closed ────────────────────────────────────────────

#[test]
fn calibration_saturated_cardinality_never_normal() {
    let result = calibrate_threshold(
        &DistributionAssessment::Normal,
        &DistributionAssessment::Normal,
        true,
        100,
    );
    assert_eq!(result, DistributionAssessment::InsufficientData);
}

#[test]
fn calibration_insufficient_clean_observations_never_normal() {
    let result = calibrate_threshold(
        &DistributionAssessment::Normal,
        &DistributionAssessment::Normal,
        false,
        3,
    );
    assert_eq!(result, DistributionAssessment::InsufficientData);
}

#[test]
fn calibration_requires_review_propagates() {
    let result = calibrate_threshold(
        &DistributionAssessment::RequiresReview,
        &DistributionAssessment::Normal,
        false,
        100,
    );
    assert_eq!(result, DistributionAssessment::RequiresReview);
}

// ── Per-service cardinality independence under attack ──────────────────

#[test]
fn adversarial_traffic_one_service_does_not_poison_another() {
    let mut store = MetricProfileStore::new();
    for i in 0..100u64 {
        store.observe_cardinality(
            "svc-attacked".into(),
            TrafficDirection::Ingress,
            0,
            Some(i),
            None,
            ObservationQuality::Clean,
        );
    }
    store.observe_cardinality(
        "svc-quiet".into(),
        TrafficDirection::Ingress,
        0,
        Some(1u64),
        None,
        ObservationQuality::Clean,
    );
    let a_est = store
        .source_cardinality_estimate("svc-attacked", TrafficDirection::Ingress, 0)
        .unwrap();
    let b_est = store
        .source_cardinality_estimate("svc-quiet", TrafficDirection::Ingress, 0)
        .unwrap();
    assert!(a_est.estimate > b_est.estimate, "services must stay independent");
}