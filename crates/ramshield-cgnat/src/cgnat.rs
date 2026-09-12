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
    fn fingerprint_counts(fingerprint: &[u8]) -> Vec<u64> {
        let mut counts = vec![0u64; 256];
        for &b in fingerprint {
            counts[b as usize] += 1;
        }
        counts
    }

    pub fn classify(&self, fingerprint: &[u8]) -> u8 {
        let counts = Self::fingerprint_counts(fingerprint);
        let entropy = shannon_entropy(&counts, fingerprint.len() as u64);
        let slot = self.rules.get_slot((fingerprint.as_ptr() as u64 & 0xFFFF_FFFF) as usize % 65536);

        if slot.client_hash.load(std::sync::atomic::Ordering::Relaxed) == 0 {
            CGNAT_TIER_CHALLENGE
        } else if entropy < self.entropy_threshold {
            CGNAT_TIER_XDP_DROP
        } else {
            slot.tier.load(std::sync::atomic::Ordering::Relaxed)
        }
    }
}