//! Decaying Count-Min Sketch for heavy-hitter frequency tracking
//!
//! 4 rows × 65,536 columns of AtomicU64 buckets = 512 KiB, allocated
//! directly on the heap via flat Vec (no stack-array boxing that would
//! overflow musl's 128 KiB thread stacks).
//!
//! Single shared instance per daemon — O(1) space w.r.t. stream length.

use std::sync::atomic::{AtomicU64, Ordering};

pub const CMS_ROWS: usize = 4;
pub const CMS_COLS: usize = 65_536;

pub struct DecayingCountMinSketch {
    counts: Vec<AtomicU64>,
    /// Number of items inserted since last decay (for decay scheduling)
    inserted_since_decay: AtomicU64,
}

impl DecayingCountMinSketch {
    pub fn new() -> Self {
        // Flat heap allocation — avoids stack overflow on small stacks
        let counts = (0..CMS_ROWS * CMS_COLS)
            .map(|_| AtomicU64::new(0))
            .collect::<Vec<_>>();
        assert_eq!(counts.len(), CMS_ROWS * CMS_COLS);
        Self {
            counts,
            inserted_since_decay: AtomicU64::new(0),
        }
    }

    /// Hash function per row: mix the key with a per-row salt
    #[inline]
    fn row_hash(row: usize, key: u64) -> usize {
        let salt = (row as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
        (key ^ salt).wrapping_mul(0x9E3779B97F4A7C15) as usize
    }

    pub fn increment(&self, key: u64) {
        for row in 0..CMS_ROWS {
            let col = Self::row_hash(row, key) & (CMS_COLS - 1);
            let idx = row * CMS_COLS + col;
            self.counts[idx].fetch_add(1, Ordering::Relaxed);
        }
        self.inserted_since_decay.fetch_add(1, Ordering::Relaxed);
    }

    /// Frequency estimate: minimum across all rows (count-min property)
    pub fn estimate(&self, key: u64) -> u64 {
        let mut min = u64::MAX;
        for row in 0..CMS_ROWS {
            let col = Self::row_hash(row, key) & (CMS_COLS - 1);
            let idx = row * CMS_COLS + col;
            let v = self.counts[idx].load(Ordering::Relaxed);
            if v < min {
                min = v;
            }
        }
        min
    }

    /// Exponential decay: halve all counters, call when inserted_since_decay
    /// exceeds a threshold (e.g. 1M) to keep counts bounded.
    pub fn decay(&self) {
        for c in &self.counts {
            c.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| Some(v / 2))
                .ok();
        }
        self.inserted_since_decay.store(0, Ordering::Relaxed);
    }

    /// Total insertions since last decay
    pub fn inserted_since_decay(&self) -> u64 {
        self.inserted_since_decay.load(Ordering::Relaxed)
    }
}

impl Default for DecayingCountMinSketch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_allocated_no_stack_overflow() {
        let cms = DecayingCountMinSketch::new();
        cms.increment(42);
        assert_eq!(cms.estimate(42), 1);
        assert_eq!(cms.estimate(43), 0);
    }

    #[test]
    fn decay_halves_counts() {
        let cms = DecayingCountMinSketch::new();
        for _ in 0..4 {
            cms.increment(7);
        }
        assert_eq!(cms.estimate(7), 4);
        cms.decay();
        assert_eq!(cms.estimate(7), 2);
    }
}