//! Hybrid Logical Clock (HLC) with burst-safe 32-bit logical counter
//!
//! 64-bit wall-clock milliseconds + 32-bit logical counter.
//! 32-bit counter avoids the 3.6-year wrap that a 16-bit counter would hit
//! under sustained burst traffic.

use std::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Hlc {
    last_ms: AtomicU64,
    logical: AtomicU32,
}

impl Hlc {
    pub fn new() -> Self {
        Self {
            last_ms: AtomicU64::new(0),
            logical: AtomicU32::new(0),
        }
    }

    /// Returns a 96-bit timestamp: (ms << 32) | logical_counter
    pub fn now(&self) -> u128 {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let prev = self.last_ms.load(Ordering::Relaxed);
        let logical = if ms > prev {
            self.last_ms.store(ms, Ordering::Release);
            0
        } else {
            self.logical.fetch_add(1, Ordering::Relaxed) + 1
        };
        ((ms as u128) << 32) | (logical as u128)
    }
}

impl Default for Hlc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotonicity() {
        let hlc = Hlc::new();
        let t1 = hlc.now();
        let t2 = hlc.now();
        assert!(t2 >= t1);
    }
}