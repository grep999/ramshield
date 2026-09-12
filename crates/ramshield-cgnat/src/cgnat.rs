//! Shannon-Entropy Analyzer for CGNAT & Shared IP Protection
//!
//! Reuses ramshield_forecasting::shannon_entropy for JA4 fingerprint
//! cardinality analysis.

use ramshield_forecasting::shannon_entropy;
use std::collections::HashMap;

pub struct CgnatGuard {
    entropy_threshold: f64,
}

impl CgnatGuard {
    pub fn new(entropy_threshold: f64) -> Self {
        Self { entropy_threshold }
    }

    /// Evaluates if an IP exhibits multi-client entropy (e.g., thousands of sessions on NAT)
    pub fn is_shared(&self, fingerprint_counts: &HashMap<u64, u32>, total_samples: u32) -> bool {
        if total_samples < 50 {
            return false; // Insufficient statistics
        }

        let counts: Vec<u64> = fingerprint_counts.values().map(|&c| c as u64).collect();
        let entropy = shannon_entropy(&counts, total_samples as u64);

        entropy >= self.entropy_threshold
    }

    /// Prevents dropping shared infrastructure at Layer 3
    pub fn resolve_tier(&self, candidate_tier: u8, is_shared: bool) -> u8 {
        if is_shared && candidate_tier >= 3 {
            2 // Clamp to Tier 2 (Interactive Challenge)
        } else {
            candidate_tier
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_high_entropy() {
        let guard = CgnatGuard::new(2.5);
        let mut ja4_map = HashMap::new();
        // Simulate high client diversity (typical mobile carrier NAT)
        for i in 0..20 {
            ja4_map.insert(i, 5); // 20 unique fingerprints, 5 hits each = 100 total
        }

        let is_shared = guard.is_shared(&ja4_map, 100);
        assert!(is_shared, "High entropy must be flagged as Shared Infrastructure");

        let resolved_tier = guard.resolve_tier(3, is_shared);
        assert_eq!(resolved_tier, 2, "Tier 3 (XDP Drop) must be downgraded to Tier 2 (Challenge)");
    }

    #[test]
    fn test_dedicated_low_entropy() {
        let guard = CgnatGuard::new(2.5);
        let mut ja4_map = HashMap::new();
        // Simulate single client: 1 fingerprint, 100 hits = 0 entropy
        ja4_map.insert(1, 100);

        let is_shared = guard.is_shared(&ja4_map, 100);
        assert!(!is_shared, "Single fingerprint must NOT be flagged as shared");

        let resolved_tier = guard.resolve_tier(3, is_shared);
        assert_eq!(resolved_tier, 3, "Dedicated IP Tier 3 must remain Tier 3");
    }
}