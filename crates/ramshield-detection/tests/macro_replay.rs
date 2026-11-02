//! Labeled macro-replay scenarios for the adaptive per-IP rate detector.
//!
//! These deterministic traces verify the state machine's acceptance behavior;
//! they are not a substitute for production packet-capture replay or performance
//! measurement.

use ramshield_detection::rate_tracker::{
    DetectorMode, DetectorProfile, PerIpRecord, TrafficContext,
};
use ramshield_forecasting::Hypothesis;

const NS_PER_SECOND: u64 = 1_000_000_000;

fn tick(record: &mut PerIpRecord, index: u64, rate: f64, hypothesis: Option<Hypothesis>) -> bool {
    let timestamp_ns = index * NS_PER_SECOND;
    let context = hypothesis.map(|hypothesis| TrafficContext {
        hypothesis,
        observed_at_ns: timestamp_ns,
        confidence: 0.95,
    });
    record
        .update(rate, timestamp_ns, context.as_ref())
        .expect("valid macro-replay sample")
        .is_breached
}

fn warmed(rate: f64) -> PerIpRecord {
    let mut record = PerIpRecord::new(rate, DetectorProfile::default()).unwrap();
    for index in 1..=5 {
        assert!(!tick(&mut record, index, rate, None));
    }
    record
}

#[test]
fn benign_steady_state_has_no_false_breaches() {
    let mut record = warmed(100.0);
    for index in 6..=300 {
        let rate = match index % 5 {
            0 => 90.0,
            1 => 95.0,
            2 => 100.0,
            3 => 105.0,
            _ => 110.0,
        };
        assert!(
            !tick(&mut record, index, rate, None),
            "benign trace breached at second {index}, rate={rate}"
        );
    }
}

#[test]
fn classified_flash_crowd_does_not_trigger_adaptive_breach() {
    let mut record = warmed(100.0);
    for (index, rate) in [(6, 5_000.0), (7, 15_000.0), (8, 15_000.0)] {
        assert!(
            !tick(&mut record, index, rate, Some(Hypothesis::FlashCrowd)),
            "fresh FlashCrowd classification breached at second {index}"
        );
    }
    assert_ne!(record.mode, DetectorMode::Breached);
}

#[test]
fn unclassified_volumetric_flood_breaches_immediately_after_warmup() {
    let mut record = warmed(100.0);
    assert!(tick(&mut record, 6, 50_000.0, None));
    assert_eq!(record.mode, DetectorMode::Breached);
}

#[test]
fn slow_ramp_breaches_before_baseline_poisoning_exceeds_twenty_five_percent() {
    let mut record = warmed(100.0);
    let mut detected_at = None;
    for index in 6..=65 {
        let rate = 100.0 + (index - 5) as f64 * 5.0;
        if tick(&mut record, index, rate, Some(Hypothesis::SlowRampDoS)) {
            detected_at = Some((index, rate));
            break;
        }
    }
    let (index, rate) = detected_at.expect("slow ramp should be detected");
    assert!(
        record.baseline_rate <= 125.0,
        "baseline poisoned to {} by second {index} at rate {rate}",
        record.baseline_rate
    );
}

#[test]
fn pulsing_attack_requires_sustained_recovery_and_rebreaches() {
    let mut record = warmed(100.0);
    for index in 6..=10 {
        assert!(tick(&mut record, index, 500.0, None));
    }
    assert_eq!(record.mode, DetectorMode::Breached);

    for index in 11..=13 {
        assert!(tick(&mut record, index, 20.0, None));
        assert_eq!(
            record.mode,
            DetectorMode::Breached,
            "three low samples must not clear while CUSUM remains above the clear threshold"
        );
    }
    for index in 14..=16 {
        tick(&mut record, index, 20.0, None);
    }
    assert_eq!(record.mode, DetectorMode::Monitoring);

    assert!(tick(&mut record, 17, 500.0, None));
    assert_eq!(record.mode, DetectorMode::Breached);
}
