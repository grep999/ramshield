//! CGNAT Guard with Shannon Entropy & Graduated Mitigation
//!
//! Graduated 4-tier response per-rule: Allow/Challenge/XDP Drop/Block.
//! Uses ramshield_forecasting::shannon_entropy() to fingerprint JA4 etc.
//! and flag shared-infra IPs for extra scrutiny.

use std::sync::Arc;
use crate::shm::ShmTableManager;
use ramshield_forecasting::shannon_entropy;

pub const CGNAT_TIER_ALLOW: u8 = 0;
pub const CGNAT_TIER_CHALLENGE: u8 = 1;
pub const CGNAT_TIER_XDP_DROP: u8 = 2;
pub const CGNAT_TIER_BLOCK: u8 = 3;

pub struct CgnatGuard {
    rules: Arc<ShmTableManager>,
    entropy_threshold: f64,
}

impl CgnatGuard {
    pub fn new(rules: Arc<ShmTableManager>, entropy_threshold: f64) -> Self {
        Self { rules, entropy_threshold }
    }

    /// Convert fingerprint bytes into byte-value frequency counts
    fn fingerprint_counts(fingerprint: &[u8]) -> [u64; 256] {
        let mut counts = [0u64; 256];
        for &b in fingerprint {
            counts[b as usize] += 1;
        }
        counts
    }

    /// Derive the SHM slot from the fingerprint bytes, not from their address.
    /// The address changes between allocations and processes, so pointer-based
    /// indexing could not reliably find a published rule.
    pub fn fingerprint_hash(fingerprint: &[u8]) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for &byte in fingerprint {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    pub fn classify(&self, fingerprint: &[u8]) -> u8 {
        let counts = Self::fingerprint_counts(fingerprint);
        let entropy = shannon_entropy(&counts, fingerprint.len() as u64);
        let slot = self.rules.find_rule(Self::fingerprint_hash(fingerprint));

        match slot {
            // No published rule means this identity has not been classified
            // as shared infrastructure. Preserve the normal hard-block path;
            // Challenge is reserved for an explicitly published shared rule.
            None => CGNAT_TIER_BLOCK,
            Some(_) if entropy < self.entropy_threshold => CGNAT_TIER_XDP_DROP,
            Some(slot) => slot.tier.load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CgnatGuard;

    #[test]
    fn fingerprint_hash_is_stable_across_allocations() {
        let first = String::from("client-fingerprint");
        let second = String::from("client-fingerprint");
        assert_eq!(
            CgnatGuard::fingerprint_hash(first.as_bytes()),
            CgnatGuard::fingerprint_hash(second.as_bytes())
        );
    }
}