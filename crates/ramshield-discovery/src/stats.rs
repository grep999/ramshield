//! Stable online moments and descriptive count-family diagnostics.
//! A family fit is not a calibrated anomaly probability.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CountMoments {
    observations: u64,
    mean: f64,
    m2: f64,
}

impl CountMoments {
    pub fn push(&mut self, value: u64) {
        if self.observations == u64::MAX { return; }
        let value = value as f64;
        self.observations += 1;
        let delta = value - self.mean;
        self.mean += delta / self.observations as f64;
        self.m2 += delta * (value - self.mean);
        if self.m2 < 0.0 && self.m2 > -f64::EPSILON { self.m2 = 0.0; }
    }
    pub fn observations(&self) -> u64 { self.observations }
    pub fn mean(&self) -> f64 { self.mean }
    pub fn is_valid(&self) -> bool {
        self.mean.is_finite() && self.mean >= 0.0 && self.m2.is_finite()
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
    Poisson { mean: f64 },
    NegativeBinomial { mean: f64, dispersion: f64, size: f64, probability: f64 },
    Underdispersed { mean: f64, variance: f64 },
}

/// Method-of-moments family diagnostic using Var(X)=mu+alpha*mu^2.
/// This does not test goodness-of-fit or provide calibrated tail probabilities.
pub fn fit_count_model(n: u64, mean: f64, variance: f64) -> CountModelFit {
    if n < 2 || !mean.is_finite() || !variance.is_finite() || mean < 0.0 || variance < 0.0 {
        return CountModelFit::InsufficientData;
    }
    if mean <= f64::EPSILON {
        return if variance <= f64::EPSILON { CountModelFit::Poisson { mean: 0.0 } }
            else { CountModelFit::Underdispersed { mean, variance } };
    }
    let tolerance = 1e-9 * mean.max(1.0);
    if variance > mean + tolerance {
        let denominator = mean * mean;
        if !denominator.is_finite() || denominator <= 0.0 { return CountModelFit::InsufficientData; }
        let dispersion = (variance - mean) / denominator;
        let size = 1.0 / dispersion;
        if !dispersion.is_finite() || dispersion <= 0.0 || !size.is_finite() { return CountModelFit::InsufficientData; }
        let probability = size / (size + mean);
        CountModelFit::NegativeBinomial { mean, dispersion, size, probability }
    } else if variance + tolerance >= mean {
        CountModelFit::Poisson { mean }
    } else {
        CountModelFit::Underdispersed { mean, variance }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn welford_is_stable_for_large_offsets() {
        let mut m = CountMoments::default();
        for x in [1_000_000_001u64, 1_000_000_002, 1_000_000_003] { m.push(x); }
        assert!((m.mean() - 1_000_000_002.0).abs() < 1e-6);
        assert!((m.sample_variance().unwrap() - 1.0).abs() < 1e-6);
    }
    #[test]
    fn detects_overdispersion_without_claiming_goodness_of_fit() {
        assert!(matches!(fit_count_model(10, 100.0, 2_000.0), CountModelFit::NegativeBinomial { .. }));
    }
    #[test]
    fn keeps_underdispersion_explicit() {
        assert!(matches!(fit_count_model(10, 100.0, 20.0), CountModelFit::Underdispersed { .. }));
    }

    #[test]
    fn rejects_invalid_and_insufficient_model_inputs() {
        assert!(matches!(fit_count_model(1, 100.0, 100.0), CountModelFit::InsufficientData));
        assert!(matches!(fit_count_model(10, f64::NAN, 100.0), CountModelFit::InsufficientData));
        let moments = CountMoments::default();
        assert_eq!(moments.observations(), 0);
        assert!(moments.is_valid());
    }
}