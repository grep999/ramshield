//! Stable online moments and descriptive count-family diagnostics.
//! A family fit is not a calibrated anomaly probability.
use crate::metric::{CardinalityEstimate, MetricAssessment};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CountMoments {
    observations: u64,
    mean: f64,
    m2: f64,
}

impl CountMoments {
    pub fn push(&mut self, value: u64) {
        if self.observations == u64::MAX {
            return;
        }
        let value = value as f64;
        self.observations += 1;
        let delta = value - self.mean;
        self.mean += delta / self.observations as f64;
        self.m2 += delta * (value - self.mean);
        if self.m2 < 0.0 && self.m2 > -f64::EPSILON {
            self.m2 = 0.0;
        }
    }
    pub fn observations(&self) -> u64 {
        self.observations
    }
    pub fn mean(&self) -> f64 {
        self.mean
    }
    pub fn is_valid(&self) -> bool {
        self.mean.is_finite()
            && self.mean >= 0.0
            && self.m2.is_finite()
            && self.m2 >= -f64::EPSILON
            && (self.observations != 0 || (self.mean == 0.0 && self.m2 == 0.0))
            && (self.mean != 0.0 || self.m2.abs() <= f64::EPSILON)
    }
    pub fn sample_variance(&self) -> Option<f64> {
        (self.observations > 1).then(|| (self.m2 / (self.observations - 1) as f64).max(0.0))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(tag = "family", rename_all = "snake_case")]
pub enum CountModelFit {
    InsufficientData,
    Poisson {
        mean: f64,
    },
    NegativeBinomial {
        mean: f64,
        dispersion: f64,
        size: f64,
        probability: f64,
    },
    Underdispersed {
        mean: f64,
        variance: f64,
    },
}

/// Assessment of whether a distribution profile shows anomalous behaviour.
/// Consumes only quality-qualified evidence (Clean observations) from D1-D6.
/// Distinguishes expected activity from unexpected deviations without making
/// enforcement decisions — remains observational.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DistributionAssessment {
    /// Distribution behaviour is within expected bounds (no anomaly).
    Normal,
    /// Distribution deviates from baseline; requires investigation.
    RequiresReview,
    /// Insufficient quality-qualified evidence to make assessment.
    InsufficientData,
}

/// Assess whether a CardinalityEstimate indicates anomalous distinct-identity growth.
/// Only considers Clean-observation estimates; Missing/Degraded/Invalid return InsufficientData.
/// A significantly growing estimate (more than 2x baseline) triggers RequiresReview.
/// Saturated estimators (u64::MAX) always produce InsufficientData — unreliable evidence.
/// Baseline must be provided from prior observation windows; without it, returns InsufficientData.
pub fn assess_cardinality_anomaly(
    current: &CardinalityEstimate,
    baseline: Option<&CardinalityEstimate>,
    observations: u64,
) -> DistributionAssessment {
    // Saturated estimator — unreliable evidence, never automatic recommendation
    if current.estimate == u64::MAX
        || !current.standard_error.is_finite()
        || current.standard_error < 0.0
    {
        return DistributionAssessment::InsufficientData;
    }
    // Require enough evidence in both the explicit gate and the estimator itself.
    if observations < 10 || current.observations < 10 {
        return DistributionAssessment::InsufficientData;
    }
    // No baseline to compare against — conservatively insufficient
    let Some(base) = baseline else {
        return DistributionAssessment::InsufficientData;
    };
    // Both estimates must be finite and positive
    if base.estimate == 0
        || base.observations < 10
        || !base.standard_error.is_finite()
        || base.standard_error < 0.0
    {
        return DistributionAssessment::InsufficientData;
    }
    // Saturating arithmetic prevents a large but finite baseline from wrapping.
    if current.estimate > base.estimate.saturating_mul(2) {
        return DistributionAssessment::RequiresReview;
    }
    // Check for significant decline (<0.5x baseline)
    if current.estimate.saturating_mul(2) < base.estimate {
        return DistributionAssessment::RequiresReview;
    }
    DistributionAssessment::Normal
}

/// Assess whether metric observations show distributional anomalies.
/// Gates on observation quality: only Clean observations update trusted state.
/// Degraded/Missing/Invalid observations are counted but not trusted for assessment.
/// Returns InsufficientData when not enough Clean observations exist.
pub fn assess_metric_distribution(assessment: &MetricAssessment) -> DistributionAssessment {
    let clean = assessment.clean_observations;
    // The latest observation must be clean, and the count must describe the
    // exact moments state being assessed. A stale/mismatched counter is unsafe.
    if assessment.quality != crate::ObservationQuality::Clean
        || assessment.metric == crate::CountMetric::Flows
        || assessment.degraded_observations != 0
        || clean < 10
        || assessment.moments.observations() != clean
    {
        return DistributionAssessment::InsufficientData;
    }
    // Moments must be valid
    if !assessment.moments.is_valid() {
        return DistributionAssessment::InsufficientData;
    }
    // With sufficient Clean observations, check variance against mean
    // (dispersion test: variance vs mean + alpha*mu^2)
    let Some(variance) = assessment.moments.sample_variance() else {
        return DistributionAssessment::InsufficientData;
    };
    let mean = assessment.moments.mean();
    // If variance is within expected Poisson range, Normal
    // Otherwise RequiresReview for over/under-dispersion
    if mean == 0.0 {
        return if variance == 0.0 {
            DistributionAssessment::Normal
        } else {
            DistributionAssessment::RequiresReview
        };
    }
    // Flag material over- and under-dispersion for human review. This is a
    // descriptive heuristic, not a calibrated statistical test.
    if variance > mean * 1.5 || variance < mean * 0.5 {
        return DistributionAssessment::RequiresReview;
    }
    DistributionAssessment::Normal
}

/// Method-of-moments family diagnostic using Var(X)=mu+alpha*mu^2.
/// This does not test goodness-of-fit or provide calibrated tail probabilities.
pub fn fit_count_model(n: u64, mean: f64, variance: f64) -> CountModelFit {
    if n < 2 || !mean.is_finite() || !variance.is_finite() || mean < 0.0 || variance < 0.0 {
        return CountModelFit::InsufficientData;
    }
    if mean <= f64::EPSILON {
        return if variance <= f64::EPSILON {
            CountModelFit::Poisson { mean: 0.0 }
        } else {
            CountModelFit::Underdispersed { mean, variance }
        };
    }
    let tolerance = 1e-9 * mean.max(1.0);
    if variance > mean + tolerance {
        let denominator = mean * mean;
        if !denominator.is_finite() || denominator <= 0.0 {
            return CountModelFit::InsufficientData;
        }
        let dispersion = (variance - mean) / denominator;
        let size = 1.0 / dispersion;
        if !dispersion.is_finite() || dispersion <= 0.0 || !size.is_finite() {
            return CountModelFit::InsufficientData;
        }
        let probability = size / (size + mean);
        CountModelFit::NegativeBinomial {
            mean,
            dispersion,
            size,
            probability,
        }
    } else if variance + tolerance >= mean {
        CountModelFit::Poisson { mean }
    } else {
        CountModelFit::Underdispersed { mean, variance }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CountMetric, ObservationQuality};
    #[test]
    fn welford_is_stable_for_large_offsets() {
        let mut m = CountMoments::default();
        for x in [1_000_000_001u64, 1_000_000_002, 1_000_000_003] {
            m.push(x);
        }
        assert!((m.mean() - 1_000_000_002.0).abs() < 1e-6);
        assert!((m.sample_variance().unwrap() - 1.0).abs() < 1e-6);
    }
    #[test]
    fn detects_overdispersion_without_claiming_goodness_of_fit() {
        assert!(matches!(
            fit_count_model(10, 100.0, 2_000.0),
            CountModelFit::NegativeBinomial { .. }
        ));
    }
    #[test]
    fn keeps_underdispersion_explicit() {
        assert!(matches!(
            fit_count_model(10, 100.0, 20.0),
            CountModelFit::Underdispersed { .. }
        ));
    }

    #[test]
    fn rejects_invalid_and_insufficient_model_inputs() {
        assert!(matches!(
            fit_count_model(1, 100.0, 100.0),
            CountModelFit::InsufficientData
        ));
        assert!(matches!(
            fit_count_model(10, f64::NAN, 100.0),
            CountModelFit::InsufficientData
        ));
        let moments = CountMoments::default();
        assert_eq!(moments.observations(), 0);
        assert!(moments.is_valid());
    }

    #[test]
    fn cardinality_anomaly_saturated_estimator_returns_insufficient() {
        use crate::metric::CardinalityEstimate;
        let current = CardinalityEstimate {
            estimate: u64::MAX,
            standard_error: f64::INFINITY,
            observations: 100,
        };
        let assessment = assess_cardinality_anomaly(&current, None, 100);
        assert!(matches!(
            assessment,
            DistributionAssessment::InsufficientData
        ));
    }

    #[test]
    fn cardinality_anomaly_insufficient_observations() {
        use crate::metric::CardinalityEstimate;
        let current = CardinalityEstimate {
            estimate: 50,
            standard_error: 5.0,
            observations: 3,
        };
        let assessment = assess_cardinality_anomaly(&current, None, 3);
        assert!(matches!(
            assessment,
            DistributionAssessment::InsufficientData
        ));
    }

    #[test]
    fn cardinality_anomaly_no_baseline() {
        use crate::metric::CardinalityEstimate;
        let current = CardinalityEstimate {
            estimate: 100,
            standard_error: 5.0,
            observations: 50,
        };
        let assessment = assess_cardinality_anomaly(&current, None, 50);
        assert!(matches!(
            assessment,
            DistributionAssessment::InsufficientData
        ));
    }

    #[test]
    fn cardinality_anomaly_significant_growth_requires_review() {
        use crate::metric::CardinalityEstimate;
        let current = CardinalityEstimate {
            estimate: 200,
            standard_error: 5.0,
            observations: 50,
        };
        let baseline = CardinalityEstimate {
            estimate: 50,
            standard_error: 3.0,
            observations: 50,
        };
        let assessment = assess_cardinality_anomaly(&current, Some(&baseline), 50);
        assert!(matches!(assessment, DistributionAssessment::RequiresReview));
    }

    #[test]
    fn cardinality_anomaly_significant_decline_requires_review() {
        use crate::metric::CardinalityEstimate;
        let current = CardinalityEstimate {
            estimate: 10,
            standard_error: 3.0,
            observations: 50,
        };
        let baseline = CardinalityEstimate {
            estimate: 100,
            standard_error: 5.0,
            observations: 50,
        };
        let assessment = assess_cardinality_anomaly(&current, Some(&baseline), 50);
        assert!(matches!(assessment, DistributionAssessment::RequiresReview));
    }

    #[test]
    fn cardinality_anomaly_within_bounds_normal() {
        use crate::metric::CardinalityEstimate;
        let current = CardinalityEstimate {
            estimate: 60,
            standard_error: 4.0,
            observations: 50,
        };
        let baseline = CardinalityEstimate {
            estimate: 50,
            standard_error: 3.0,
            observations: 50,
        };
        let assessment = assess_cardinality_anomaly(&current, Some(&baseline), 50);
        assert!(matches!(assessment, DistributionAssessment::Normal));
    }

    #[test]
    fn metric_distribution_insufficient_clean_observations() {
        use crate::MetricAssessment;
        let assessment = MetricAssessment {
            metric: CountMetric::Packets,
            observed_packets: 100,
            quality: ObservationQuality::Clean,
            moments: CountMoments::default(),
            clean_observations: 2,
            degraded_observations: 0,
            baseline_source: None,
            last_updated_secs: 0,
        };
        let result = assess_metric_distribution(&assessment);
        assert!(matches!(result, DistributionAssessment::InsufficientData));
    }

    #[test]
    fn metric_distribution_normal_with_sufficient_observations() {
        use crate::MetricAssessment;
        let mut moments = CountMoments::default();
        moments.push(10);
        moments.push(12);
        moments.push(11);
        moments.push(13);
        moments.push(10);
        moments.push(11);
        moments.push(12);
        moments.push(10);
        moments.push(11);
        moments.push(13);
        let assessment = MetricAssessment {
            metric: CountMetric::Packets,
            observed_packets: 113,
            quality: ObservationQuality::Clean,
            moments,
            clean_observations: 10,
            degraded_observations: 0,
            baseline_source: None,
            last_updated_secs: 0,
        };
        let result = assess_metric_distribution(&assessment);
        assert!(
            matches!(
                result,
                DistributionAssessment::Normal | DistributionAssessment::RequiresReview
            ),
            "Expected Normal or RequiresReview, got {:?}",
            result
        );
    }
}

/// Report-only threshold-calibration eligibility assessment with strict quality gating.
///
/// This function does not calculate a numeric threshold, validate a model, or
/// authorize detector/enforcement changes. Normal means only that the supplied
/// assessments passed these gates; it is not a production calibration claim.
///
/// **Rule**: Saturated estimators, insufficient samples, degraded windows,
/// unknown metric semantics, and unvalidated statistical models must never
/// produce automatic threshold recommendations.
///
/// They produce:
/// - `InsufficientData` when evidence is too weak to calibrate
/// - `RequiresReview` when evidence suggests possible anomaly but needs human review
///
/// This function is the D8 boundary: it does not make enforcement decisions,
/// it only reports whether the supplied evidence is sufficient for further
/// offline evaluation. No enforcement action should consume this result directly.
pub fn calibrate_threshold(
    cardinality_assessment: &DistributionAssessment,
    metric_assessment: &DistributionAssessment,
    cardinality_saturated: bool,
    clean_observations: u64,
) -> DistributionAssessment {
    // Saturated cardinality estimator — never auto-recommend
    if cardinality_saturated {
        return DistributionAssessment::InsufficientData;
    }
    // Insufficient Clean observations — cannot calibrate
    if clean_observations < 10 {
        return DistributionAssessment::InsufficientData;
    }
    // Fail closed: insufficient evidence from either assessment must never
    // be promoted to Normal merely because the aggregate sample count is high.
    if matches!(
        cardinality_assessment,
        DistributionAssessment::InsufficientData
    ) || matches!(metric_assessment, DistributionAssessment::InsufficientData)
    {
        return DistributionAssessment::InsufficientData;
    }
    if matches!(
        cardinality_assessment,
        DistributionAssessment::RequiresReview
    ) || matches!(metric_assessment, DistributionAssessment::RequiresReview)
    {
        return DistributionAssessment::RequiresReview;
    }
    DistributionAssessment::Normal
}

/// Convenience: D8 gate for saturated/insufficient conditions,
/// short-circuiting to InsufficientData without full assessment.
///
/// This is only a preliminary sample/saturation gate. Normal means these two
/// checks passed; it is not a statistical assessment or threshold recommendation.
pub fn d8_saturated_or_insufficient_guard(
    saturated: bool,
    clean_observations: u64,
) -> DistributionAssessment {
    if saturated {
        return DistributionAssessment::InsufficientData;
    }
    if clean_observations < 10 {
        return DistributionAssessment::InsufficientData;
    }
    DistributionAssessment::Normal
}

#[test]
fn d8_calibrate_saturated_estimator_insufficient() {
    let cal = calibrate_threshold(
        &DistributionAssessment::Normal,
        &DistributionAssessment::Normal,
        true,
        100,
    );
    assert_eq!(cal, DistributionAssessment::InsufficientData);
}

#[test]
fn d8_calibrate_insufficient_clean_observations() {
    let cal = calibrate_threshold(
        &DistributionAssessment::Normal,
        &DistributionAssessment::Normal,
        false,
        3,
    );
    assert_eq!(cal, DistributionAssessment::InsufficientData);
}

#[test]
fn d8_calibrate_cardinality_requires_review_propagates() {
    let cal = calibrate_threshold(
        &DistributionAssessment::RequiresReview,
        &DistributionAssessment::Normal,
        false,
        100,
    );
    assert_eq!(cal, DistributionAssessment::RequiresReview);
}

#[test]
fn d8_calibrate_metric_requires_review_propagates() {
    let cal = calibrate_threshold(
        &DistributionAssessment::Normal,
        &DistributionAssessment::RequiresReview,
        false,
        100,
    );
    assert_eq!(cal, DistributionAssessment::RequiresReview);
}

#[test]
fn d8_calibrate_both_normal_returns_normal() {
    let cal = calibrate_threshold(
        &DistributionAssessment::Normal,
        &DistributionAssessment::Normal,
        false,
        100,
    );
    assert_eq!(cal, DistributionAssessment::Normal);
}

#[test]
fn d8_guard_saturated_always_insufficient() {
    assert_eq!(
        d8_saturated_or_insufficient_guard(true, 1000),
        DistributionAssessment::InsufficientData
    );
}

#[test]
fn d8_guard_insufficient_observations() {
    assert_eq!(
        d8_saturated_or_insufficient_guard(false, 5),
        DistributionAssessment::InsufficientData
    );
}

#[test]
fn d8_guard_sufficient_passes() {
    assert_eq!(
        d8_saturated_or_insufficient_guard(false, 100),
        DistributionAssessment::Normal
    );
}

#[cfg(test)]
mod d7_d8_regression_tests {
    use super::*;
    use crate::{CountMetric, MetricAssessment, ObservationQuality};

    fn metric_assessment(quality: ObservationQuality, clean: u64) -> MetricAssessment {
        let mut moments = CountMoments::default();
        for value in 0..clean {
            moments.push(value);
        }
        MetricAssessment {
            metric: CountMetric::Packets,
            observed_packets: clean,
            quality,
            moments,
            clean_observations: clean,
            degraded_observations: 0,
            baseline_source: Some("test-baseline".into()),
            last_updated_secs: 0,
        }
    }

    #[test]
    fn d8_does_not_promote_insufficient_cardinality_to_normal() {
        assert_eq!(
            calibrate_threshold(
                &DistributionAssessment::InsufficientData,
                &DistributionAssessment::Normal,
                false,
                100,
            ),
            DistributionAssessment::InsufficientData
        );
    }

    #[test]
    fn d8_does_not_promote_insufficient_metric_to_normal() {
        assert_eq!(
            calibrate_threshold(
                &DistributionAssessment::Normal,
                &DistributionAssessment::InsufficientData,
                false,
                100,
            ),
            DistributionAssessment::InsufficientData
        );
    }

    #[test]
    fn d7_rejects_latest_degraded_metric_observation() {
        let assessment = metric_assessment(ObservationQuality::Degraded, 12);
        assert_eq!(
            assess_metric_distribution(&assessment),
            DistributionAssessment::InsufficientData
        );
    }

    #[test]
    fn d7_rejects_profiles_with_degraded_observations() {
        let mut assessment = metric_assessment(ObservationQuality::Clean, 12);
        assessment.degraded_observations = 1;
        assert_eq!(
            assess_metric_distribution(&assessment),
            DistributionAssessment::InsufficientData
        );
    }

    #[test]
    fn d7_rejects_flow_metric_without_validated_flow_source() {
        let mut assessment = metric_assessment(ObservationQuality::Clean, 12);
        assessment.metric = CountMetric::Flows;
        assert_eq!(
            assess_metric_distribution(&assessment),
            DistributionAssessment::InsufficientData
        );
    }

    #[test]
    fn d7_rejects_mismatched_moment_and_quality_counts() {
        let mut assessment = metric_assessment(ObservationQuality::Clean, 12);
        assessment.clean_observations = 13;
        assert_eq!(
            assess_metric_distribution(&assessment),
            DistributionAssessment::InsufficientData
        );
    }

    #[test]
    fn d7_rejects_invalid_cardinality_standard_errors() {
        let current = CardinalityEstimate {
            estimate: 100,
            standard_error: f64::NAN,
            observations: 100,
        };
        let baseline = CardinalityEstimate {
            estimate: 50,
            standard_error: 1.0,
            observations: 100,
        };
        assert_eq!(
            assess_cardinality_anomaly(&current, Some(&baseline), 100),
            DistributionAssessment::InsufficientData
        );
    }

    #[test]
    fn d7_cardinality_growth_comparison_cannot_overflow() {
        let current = CardinalityEstimate {
            estimate: u64::MAX - 1,
            standard_error: 1.0,
            observations: 100,
        };
        let baseline = CardinalityEstimate {
            estimate: (u64::MAX / 2) + 1,
            standard_error: 1.0,
            observations: 100,
        };
        assert_eq!(
            assess_cardinality_anomaly(&current, Some(&baseline), 100),
            DistributionAssessment::Normal
        );
    }
}
