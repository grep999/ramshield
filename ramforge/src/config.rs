// ── Detection config ────────────────────────────────────────────────────────
// Baseline tuned against NIST SP 800-63B online-guessing guidance: per-source
// throttling with escalating penalties under sustained pressure.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionConfig {
    /// Failures within window_secs that trigger the first block.
    pub max_failures: u64,
    /// Sliding window length in seconds.
    pub window_secs: u64,
    /// Block duration in seconds (cooldown before counter re-arms).
    pub block_ttl_secs: u64,
    /// Threshold floor: escalation never lowers tolerance below this.
    pub adaptive_floor: u64,
    /// Evict idle per-IP entries older than this many seconds.
    pub evict_idle_secs: u64,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            max_failures: 5,
            window_secs: 60,
            block_ttl_secs: 3600,
            adaptive_floor: 3,
            evict_idle_secs: 86_400,
        }
    }
}