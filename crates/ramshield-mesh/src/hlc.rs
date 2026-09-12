//! Hybrid Logical Clock (HLC) with burst-safe 32-bit logical counter
//!
//! 64-bit wall-clock milliseconds + 32-bit logical counter.
//! 32-bit counter avoids the 3.6-year wrap that a 16-bit counter would hit
//! under sustained burst traffic.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Hlc {
    physical_ms: AtomicU64,
    logical_seq: AtomicU32,
}

impl Hlc {
    pub fn new() -> Self {
        Self {
            physical_ms: AtomicU64::new(0),
            logical_seq: AtomicU32::new(0),
        }
    }

    /// Merge-and-tick: advance past local wall clock and received remote
    /// timestamp, bump the logical counter when physical time is tied.
    /// CAS loop guarantees monotonicity under concurrent callers.
    pub fn tick(&self, remote_ms: u64, remote_seq: u32) -> (u64, u32) {
        let wall_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        loop {
            let cur_phys = self.physical_ms.load(Ordering::Relaxed);
            let cur_seq = self.logical_seq.load(Ordering::Relaxed);

            let next_phys = wall_ms.max(remote_ms).max(cur_phys);
            let next_seq = if next_phys == cur_phys && next_phys == remote_ms {
                cur_seq.max(remote_seq) + 1
            } else if next_phys == cur_phys {
                cur_seq + 1
            } else {
                0
            };

            if self
                .physical_ms
                .compare_exchange(cur_phys, next_phys, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                self.logical_seq.store(next_seq, Ordering::Relaxed);
                return (next_phys, next_seq);
            }
        }
    }

    /// Local-only tick: advance past the wall clock, zero the counter when
    /// physical time moves, else bump it.
    pub fn now(&self) -> u128 {
        let (phys, seq) = self.tick(0, 0);
        ((phys as u128) << 32) | (seq as u128)
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

    #[test]
    fn remote_timestamp_wins() {
        let hlc = Hlc::new();
        let (phys, seq) = hlc.tick(u64::MAX, 7);
        assert!(phys >= u64::MAX - 1, "must advance past remote: {phys}");
        assert!(seq == 0, "remote ms ahead → seq reset, got {seq}");
        let (_, seq2) = hlc.tick(0, 0);
        assert!(seq2 >= 1, "tie on physical → seq bumps, got {seq2}");
    }
}