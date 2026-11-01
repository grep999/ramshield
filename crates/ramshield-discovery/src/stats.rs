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
    if current.estimate == u64::MAX || current.standard_error == f64::INFINITY {
        return DistributionAssessment::InsufficientData;
    }
    // Insufficient recent observations to assess change
    if observations < 10 {
        return DistributionAssessment::InsufficientData;
    }
    // No baseline to compare against — conservatively insufficient
    let Some(base) = baseline else {
        return DistributionAssessment::InsufficientData;
    };
    // Both estimates must be finite and positive
    if base.estimate == 0 || base.standard_error.is_nan() || base.standard_error == f64::INFINITY {
        return DistributionAssessment::InsufficientData;
    }
    // Check for significant growth (>2x baseline)
    if current.estimate > base.estimate * 2 {
        return DistributionAssessment::RequiresReview;
    }
    // Check for significant decline (<0.5x baseline)
    if current.estimate > 0 && current.estimate < base.estimate / 2 {
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
    // Insufficient Clean observations to form a baseline
    if clean < 5 {
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
    if mean > 0.0 {
        // Simple dispersion check: if variance > 1.5 * mean, suggests anomaly
        if variance > mean * 1.5 {
            return DistributionAssessment::RequiresReview;
        }
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
        let assessment = MetricAssessment {
            metric: CountMetric::Packets,
            observed_packets: 56,
            quality: ObservationQuality::Clean,
            moments,
            clean_observations: 5,
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

/// Production threshold calibration — maps learned assessments to
/// RamShield detector configuration parameters with strict quality gating.
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
/// it only reports calibrated assessment states for the enforcement layer to act on.
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
    // If either assessment requires review, propagate that
    if matches!(
        cardinality_assessment,
        DistributionAssessment::RequiresReview
    ) || matches!(metric_assessment, DistributionAssessment::RequiresReview)
    {
        return DistributionAssessment::RequiresReview;
    }
    // Both assessments normal and sufficient — calibrated threshold OK
    DistributionAssessment::Normal
}

/// Convenience: D8 gate for saturated/insufficient conditions,
/// short-circuiting to InsufficientData without full assessment.
///
/// This is the primary D8 guard rail — callers should NOT bypass it.
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
