//! Macro replay evaluation suite — P7/P8 adaptive CUSUM detector.
//!
//! Five synthetic traffic scenarios run against PerIpRecord directly:
//!   1. Benign steady state           → 0 breaches, FPR = 0.0%.
//!   2. Legitimate flash crowd        → 0 breaches (FlashCrowd suppression).
//!   3. Volumetric flood              → breach ≤ 2 bucket ticks (2.0 s).
//!   4. Low-and-slow creeping attack  → breach before baseline rises >25%.
//!   5. Pulsing attack + recovery     → baseline frozen, recovers after 3 clean ticks.

use super::*;

use ramshield_forecasting::Hypothesis;
use rate_tracker::{DetectorError, DetectorMode, DetectorProfile, PerIpRecord, TrafficContext};

const SECOND_NS: u64 = 1_000_000_000;

/// Default detector profile for replay scenarios.
fn default_profile() -> DetectorProfile {
    DetectorProfile {
        min_threshold_h: 1000.0,
        // min_slack_k must be well below the pulsing attack's 500pps delta so the
        // CUSUM can accumulate; the h-floor (1000) still guards false positives.
        min_slack_k: 50.0,
        min_variance: 0.1,
        alpha: 0.3,
        clear_ratio: 0.35,
        warmup_samples: 6,
        recovery_samples: 3,
        max_gap_tolerance_ns: 5_000_000_000,
    }
}

/// Build a fresh TrafficContext tagged with a Hypothesis and observed timestamp.
fn fresh_ctx(hypothesis: Hypothesis, t: u64) -> TrafficContext {
    TrafficContext {
        hypothesis,
        observed_at_ns: t,
        confidence: 0.8,
    }
}

/// P7/P8 adaptive CUSUM detector replay tests.

#[test]
fn replay_benign_steady_state_zero_false_positives() {
    let profile = default_profile();
    let mut record = PerIpRecord::new(100.0, profile).unwrap();
    let t0 = SECOND_NS;

    // 100 pps ± 10% Gaussian-like noise for 300 seconds.
    for i in 0..300u64 {
        let rate = if i % 3 == 0 {
            90.0
        } else if i % 3 == 1 {
            100.0
        } else {
            110.0
        };
        let outcome = record.update(rate, t0 + i * SECOND_NS, None).unwrap();
        assert!(
            !outcome.is_breached,
            "benign steady state triggered breach at sample {i} (rate {rate})"
        );
    }
    assert_eq!(record.mode, DetectorMode::Monitoring);
}

/// Legitimate Flash Crowd: traffic surges with FlashCrowd hypothesis —
/// adaptive threshold scales up (h ← 2h), no breach should occur.
#[test]
fn replay_legitimate_flash_crowd_suppressed() {
    let profile = default_profile();
    let mut record = PerIpRecord::new(100.0, profile).unwrap();
    let t0 = SECOND_NS;

    // Warm up with benign traffic.
    for i in 0..10u64 {
        record.update(100.0, t0 + i * SECOND_NS, None).unwrap();
    }

    // Surge to 15,000 pps tagged FlashCrowd — adaptive threshold should scale h ← 2h.
    for i in 10..30u64 {
        let ctx = fresh_ctx(Hypothesis::FlashCrowd, t0 + i * SECOND_NS);
        let outcome = record
            .update(15_000.0, t0 + i * SECOND_NS, Some(&ctx))
            .unwrap();
        assert!(
            !outcome.is_breached,
            "flash crowd triggered breach at sample {i}"
        );
    }

    // Follow-up: same surge WITHOUT context should breach (baseline hasn't adapted yet).
    let outcome = record.update(15_000.0, t0 + 30 * SECOND_NS, None).unwrap();
    assert!(
        outcome.is_breached,
        "flash crowd should breach without context"
    );
}

/// Volumetric flood: sudden 100→50,000 pps volumetric attack — breach within 2 ticks.
#[test]
fn replay_volumetric_flood_detects_fast() {
    let profile = default_profile();
    let mut record = PerIpRecord::new(100.0, profile).unwrap();
    let t0 = SECOND_NS;

    // Warm up at benign rate.
    for i in 0..10u64 {
        record.update(100.0, t0 + i * SECOND_NS, None).unwrap();
    }

    // Sudden 50,000 pps flood tagged VolumetricDoS — breach within 2 ticks.
    for i in 10..15u64 {
        let ctx = fresh_ctx(Hypothesis::VolumetricDoS, t0 + i * SECOND_NS);
        let outcome = record
            .update(50_000.0, t0 + i * SECOND_NS, Some(&ctx))
            .unwrap();
        if outcome.is_breached {
            // Breach must occur within 2 bucket ticks (2 seconds).
            assert!(
                i <= 12,
                "volumetric flood breach took {i} ticks (> 2 tick budget)"
            );
            return;
        }
    }
    panic!("volumetric flood did not breach within 5 samples");
}

/// Low-and-slow creeping attack: baseline adapts slowly; breach must trigger
/// before baseline rises >25%.
#[test]
fn replay_slow_ramp_detects_before_baseline_poisoning() {
    let profile = default_profile();
    let mut record = PerIpRecord::new(100.0, profile).unwrap();
    let t0 = SECOND_NS;

    // Warm up.
    for i in 0..10u64 {
        record.update(100.0, t0 + i * SECOND_NS, None).unwrap();
    }

    // Creep up 5 pps per second for 60 seconds, tagged SlowRampDoS.
    let mut breached = false;
    for i in 10..70u64 {
        let rate = 100.0 + (i - 10) as f64 * 5.0;
        let ctx = fresh_ctx(Hypothesis::SlowRampDoS, t0 + i * SECOND_NS);
        let outcome = record.update(rate, t0 + i * SECOND_NS, Some(&ctx)).unwrap();
        if outcome.is_breached {
            breached = true;
            // Baseline must not have risen > 25% above 100 pps.
            assert!(
                record.baseline_rate <= 125.0,
                "baseline rose to {} (>25% of 100) before breach at sample {i}",
                record.baseline_rate
            );
            break;
        }
    }
    assert!(breached, "slow ramp never triggered breach");
}

/// Pulsing attack recovery: breach triggers on burst, baseline stays frozen,
/// recovery requires 3 consecutive clean ticks.
#[test]
fn replay_pulsing_attack_recovers_with_hysteresis() {
    let profile = default_profile();
    let mut record = PerIpRecord::new(100.0, profile).unwrap();
    let t0 = SECOND_NS;

    // Warm up.
    for i in 0..10u64 {
        record.update(100.0, t0 + i * SECOND_NS, None).unwrap();
    }

    // 500 pps burst for 5 seconds → breach.
    for i in 10..15u64 {
        record.update(500.0, t0 + i * SECOND_NS, None).unwrap();
    }
    assert_eq!(record.mode, DetectorMode::Breached);

    // Baseline must remain frozen during burst.
    let frozen_baseline = record.baseline_rate;

    // Drop to 20 pps → recovery. cusum peaks at 1750 (5 burst ticks × 350) and
    // decays 130/tick, so the clear gate (350 = 0.35 × h) is reached at tick 11,
    // then 3 hysteresis ticks → recovery at ~tick 13.
    let mut recovered = false;
    for i in 15..40u64 {
        record.update(20.0, t0 + i * SECOND_NS, None).unwrap();
        if record.mode == DetectorMode::Monitoring {
            recovered = true;
            assert_eq!(record.cusum_s, 0.0, "cusum not reset on recovery");
            // Baseline resumes adapting after recovery.
            assert!(
                record.baseline_rate < frozen_baseline,
                "baseline did not resume adapting after recovery"
            );
            break;
        }
    }
    assert!(
        recovered,
        "pulsing attack did not recover after 10 clean seconds"
    );
}

/// Out-of-order timestamp rejected without mutating state (atomicity under error).
#[test]
fn replay_out_of_order_timestamp_leaves_record_unchanged() {
    let profile = default_profile();
    let mut record = PerIpRecord::new(100.0, profile).unwrap();
    let t0 = SECOND_NS;

    record.update(100.0, t0 + SECOND_NS, None).unwrap();
    let before = record.clone();

    // Non-monotonic timestamp must fail and leave state identical.
    let err = record.update(50.0, t0, None).unwrap_err();
    assert!(matches!(err, DetectorError::NonMonotonicTimestamp));
    assert_eq!(record, before);
}

/// Numerical overflow (f64::MAX) rejected without mutating state.
#[test]
fn replay_numerical_overflow_leaves_record_unchanged() {
    let profile = default_profile();
    let mut record = PerIpRecord::new(100.0, profile).unwrap();
    let t0 = SECOND_NS;

    record.update(100.0, t0 + SECOND_NS, None).unwrap();
    let before = record.clone();

    let err = record
        .update(f64::MAX, t0 + 2 * SECOND_NS, None)
        .unwrap_err();
    assert!(
        matches!(err, DetectorError::NumericalCalculationFailure),
        "expected NumericalCalculationFailure, got {err:?}"
    );
    assert_eq!(record, before);
}
