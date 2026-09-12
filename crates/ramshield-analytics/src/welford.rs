//! Exponentially Weighted Welford estimator for dynamic baseline
//!
//! 32-byte struct — tracks mean + variance with exponential decay.
//! Replaces unbounded windowed statistics in detection.

use std::sync::atomic::{AtomicU64, Ordering};

pub struct Welford {
    /// Current smoothed mean (f64 bits)
    mean: AtomicU64,
    /// Current smoothed variance (f64 bits)
    variance: AtomicU64,
    /// Number of observations seen
    count: AtomicU64,
    /// Decay factor alpha ∈ (0,1): higher = faster adaptation
    alpha: f64,
}

impl Welford {
    pub fn new(alpha: f64) -> Self {
        Self {
            mean: AtomicU64::new((0.0f64).to_bits()),
            variance: AtomicU64::new((0.0f64).to_bits()),
            count: AtomicU64::new(0),
            alpha,
        }
    }

    pub fn update(&self, value: f64) {
        let n = self.count.fetch_add(1, Ordering::Relaxed) + 1;
        let old_mean = f64::from_bits(self.mean.load(Ordering::Relaxed));

        // Exponentially weighted mean update
        let new_mean = if n == 1 {
            value
        } else {
            old_mean + self.alpha * (value - old_mean)
        };
        self.mean.store(new_mean.to_bits(), Ordering::Relaxed);

        // Welford variance update
        let old_var = f64::from_bits(self.variance.load(Ordering::Relaxed));
        let new_var = if n < 3 {
            0.0
        } else {
            let delta = value - old_mean;
            let delta2 = value - new_mean;
            (1.0 - self.alpha) * old_var + self.alpha * (delta * delta2)
        };
        self.variance.store(new_var.to_bits(), Ordering::Relaxed);
    }

    pub fn mean(&self) -> f64 {
        f64::from_bits(self.mean.load(Ordering::Relaxed))
    }

    pub fn variance(&self) -> f64 {
        f64::from_bits(self.variance.load(Ordering::Relaxed))
    }

    pub fn stddev(&self) -> f64 {
        self.variance().sqrt()
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Z-score: how many standard deviations away from mean
    pub fn z_score(&self, value: f64) -> f64 {
        let sd = self.stddev();
        if sd < 1e-9 {
            0.0
        } else {
            (value - self.mean()) / sd
        }
    }
}

impl Default for Welford {
    fn default() -> Self {
        Self::new(0.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converge_to_mean() {
        let w = Welford::new(0.3);
        for _ in 0..100 {
            w.update(10.0);
        }
        assert!((w.mean() - 10.0).abs() < 0.5, "mean={}", w.mean());
    }

    #[test]
    fn z_score_outlier() {
        let w = Welford::new(0.3);
        // Alternate values so variance is nonzero (constant input → stddev 0
        // → z_score's sd<1e-9 guard returns 0.0).
        for i in 0..50 {
            w.update(if i % 2 == 0 { 95.0 } else { 105.0 });
        }
        // Sudden spike should have high z-score
        assert!(w.z_score(1000.0) > 5.0, "z={}", w.z_score(1000.0));
    }
}