//! HyperLogLog for tracking unique IP cardinality per subnet in 1024 bytes
//!
//! Dense register array — 1024 bytes, zero heap allocations.
//! Use for IPv6 /64 subnets where 2^64 addresses make bitmaps impossible.
//! For IPv4 /24, prefer host_bitmap in storage (32 bytes exact).

pub struct SubnetHll {
    registers: [u8; 1024],
}

impl SubnetHll {
    pub const fn new() -> Self {
        Self { registers: [0; 1024] }
    }

    pub fn insert(&mut self, hash: u64) {
        let index = (hash & 0x3FF) as usize; // 10 bits: 0..1023
        let remainder = (hash >> 10) | (1 << 53); // remaining bits + implicit 1
        let leading_zeros = (remainder.trailing_zeros() + 1).min(63) as u8;

        if leading_zeros > self.registers[index] {
            self.registers[index] = leading_zeros;
        }
    }

    pub fn estimate(&self) -> f64 {
        let mut sum = 0.0;
        let mut zero_count = 0;

        for &val in &self.registers {
            sum += 2.0f64.powi(-(val as i32));
            if val == 0 {
                zero_count += 1;
            }
        }

        let m = 1024.0;
        let alpha = 0.7213 / (1.0 + 1.079 / m);
        let raw_estimate = alpha * m * m / sum;

        // Small range correction (Linear Counting)
        if raw_estimate <= 2.5 * m && zero_count > 0 {
            m * (m / zero_count as f64).ln()
        } else {
            raw_estimate
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hll_bounded_memory() {
        let mut hll = SubnetHll::new();
        let initial_size = std::mem::size_of_val(&hll);
        assert_eq!(initial_size, 1024, "HLL must be exactly 1024 bytes on stack");

        // splitmix64: proper uniform hash — avoids the v2(i) correlation that
        // i*wrapping_mul(C) has with the low-10-bit index (register values
        // collapse to ~1, estimate collapses to ~n/1024).
        fn splitmix64(mut x: u64) -> u64 {
            x = x.wrapping_add(0x9E3779B97F4A7C15);
            x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
            x ^ (x >> 31)
        }

        // Simulate massive 1,000,000 unique IP spoofing attack
        for i in 0..1_000_000 {
            hll.insert(splitmix64(i as u64));
        }

        let estimate = hll.estimate();
        let error = (estimate - 1_000_000.0).abs() / 1_000_000.0;
        assert!(error < 0.05, "HLL error must be < 5%, got: {} (est={})", error, estimate);

        // Memory must remain strictly identical
        assert_eq!(std::mem::size_of_val(&hll), 1024, "Memory must not grow on heap");
    }
}