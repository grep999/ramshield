//! Exponentially Weighted Welford estimator for dynamic baseline
//!
//! 32-byte struct — tracks mean + variance with exponential decay.
//! Replaces unbounded windowed statistics in detection.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

#[derive(Clone, Copy)]
struct WelfordState {
    mean: f32,
    variance: f32,
}

impl WelfordState {
    #[inline]
    fn pack(self) -> [u32; 2] {
        [self.mean.to_bits(), self.variance.to_bits()]
    }

    #[inline]
    fn unpack(packed: &[u32; 2]) -> Self {
        Self {
            mean: f32::from_bits(packed[0]),
            variance: f32::from_bits(packed[1]),
        }
    }
}

pub struct Welford {
    mean: AtomicU32,
    variance: AtomicU32,
    count: AtomicU64,
    alpha: f32,
}

impl Welford {
    pub fn new(alpha: f32) -> Self {
        let packed = WelfordState { mean: 0.0, variance: 0.0 }.pack();
        Self {
            mean: AtomicU32::new(packed[0]),
            variance: AtomicU32::new(packed[1]),
            count: AtomicU64::new(0),
            alpha,
        }
    }

    pub fn update(&self, value: f32) {
        self.count.fetch_add(1, Ordering::Relaxed);
        let mut current_mean = self.mean.load(Ordering::Relaxed);
        let mut current_variance = self.variance.load(Ordering::Relaxed);

        loop {
            let unpacked = WelfordState {
                mean: f32::from_bits(current_mean),
                variance: f32::from_bits(current_variance),
            };
            let delta = value - unpacked.mean;
            let new_mean = unpacked.mean + self.alpha * delta;
            let new_variance = ((1.0 - self.alpha) * (unpacked.variance + self.alpha * delta * delta)).max(0.0);

            let next_mean = new_mean.to_bits();
            let next_variance = new_variance.to_bits();
            match self.mean.compare_exchange_weak(current_mean, next_mean, Ordering::Release, Ordering::Relaxed) {
                Ok(_) => {
                    self.variance.store(next_variance, Ordering::Relaxed);
                    break;
                }
                Err(actual) => {
                    current_mean = actual;
                    current_variance = self.variance.load(Ordering::Relaxed);
                }
            }
        }
    }

    #[inline]
    pub fn mean(&self) -> f32 {
        f32::from_bits(self.mean.load(Ordering::Acquire))
    }

    #[inline]
    pub fn variance(&self) -> f32 {
        f32::from_bits(self.variance.load(Ordering::Acquire))
    }

    #[inline]
    pub fn stddev(&self) -> f32 {
        self.variance().sqrt()
    }

    #[inline]
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn z_score(&self, value: f32) -> f32 {
        let unpacked = WelfordState {
            mean: self.mean(),
            variance: self.variance(),
        };
        let std_dev = unpacked.variance.sqrt();
        if std_dev < 1e-6 {
            0.0
        } else {
            (value - unpacked.mean) / std_dev
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
        assert!((w.z_score(10.0) - 0.0).abs() < 0.5, "z={}", w.z_score(10.0));
    }

    #[test]
    fn z_score_outlier() {
        let w = Welford::new(0.3);
        for i in 0..50 {
            w.update(if i % 2 == 0 { 95.0 } else { 105.0 });
        }
        assert!(w.z_score(1000.0) > 5.0, "z={}", w.z_score(1000.0));
    }
}
