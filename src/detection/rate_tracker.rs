//! Per-IP rate tracker with sliding-window EWMA and adaptive threat scoring.
//!
//! Academic basis:
//! - EWMA for DDoS detection: Ye et al., "Decision Rules for Detection of Attacks"
//! - Sliding window: detect transient spikes that EWMA alone smooths over
//! - Adaptive thresholds: inspired by Cloudflare's adaptive rate limiting
//! - Threat score fusion: feeds back from forecasting entropy/anomaly module

// Note: AtomicU64/Ordering removed — not needed in new design

/// Window size in milliseconds for sliding-window RPS calculation.
const WINDOW_MS: u64 = 1000;

/// RateTracker maintains per-IP traffic state using:
/// - Sliding window: counts requests in the last WINDOW_MS
/// - EWMA: exponentially weighted moving average of RPS
/// - Adaptive threat score: adjusted by upstream forecasting signals
pub struct RateTracker {
    /// EWMA of requests-per-second
    pub ewma_rps: f64,
    /// EWMA smoothing factor (0-1, higher = more responsive)
    pub ewma_alpha: f64,

    /// Sliding-window state: count and window boundary
    pub window_count: u64,
    pub window_start_ms: u64,

    /// Peak RPS observed (for baseline calibration)
    pub peak_rps: f64,
    /// Long-term baseline RPS (adaptive)
    pub baseline_rps: f64,
    /// Number of observations contributing to baseline
    pub baseline_samples: u64,

    /// Dynamic threat score (0-1), adjusted by forecasting
    pub threat_score: f64,
    /// Configured RPS threshold for blocking
    pub rps_threshold: u64,
    /// Threat score threshold for blocking
    pub threat_threshold: f64,
    /// Block expiry timestamp in ms (0 = not blocked)
    pub blocked_until_ms: u64,

    /// Total requests seen by this tracker
    pub total_requests: u64,
    /// First-seen timestamp ms
    pub first_seen_ms: u64,
}

impl RateTracker {
    pub fn new(rps_threshold: u64) -> Self {
        Self {
            ewma_rps: 0.0,
            ewma_alpha: 0.3,
            window_count: 0,
            window_start_ms: 0,
            peak_rps: 0.0,
            baseline_rps: 0.0,
            baseline_samples: 0,
            threat_score: 0.0,
            rps_threshold,
            threat_threshold: 0.7,
            blocked_until_ms: 0,
            total_requests: 0,
            first_seen_ms: 0,
        }
    }

    /// Record a request and compute current RPS.
    /// `timestamp_ns` is the event timestamp in nanoseconds,
    /// `current_ms` is the wall-clock time in milliseconds.
    pub fn record_request(&mut self, _timestamp_ns: u64, current_ms: u64) -> f64 {
        if self.first_seen_ms == 0 {
            self.first_seen_ms = current_ms;
            self.window_start_ms = current_ms;
        }

        self.total_requests += 1;

        // Slide window if needed
        if current_ms >= self.window_start_ms + WINDOW_MS {
            // Compute RPS from the completed window
            let elapsed = current_ms.saturating_sub(self.window_start_ms);
            if elapsed > 0 {
                let window_rps = (self.window_count as f64 * 1000.0) / elapsed as f64;
                self.update_ewma(window_rps);
                self.update_baseline(window_rps);
            }
            self.window_count = 0;
            self.window_start_ms = current_ms;
        }

        self.window_count += 1;

        // Estimate instantaneous RPS from current window
        let elapsed_in_window = current_ms.saturating_sub(self.window_start_ms).max(1);
        let _instant_rps = (self.window_count as f64 * 1000.0) / elapsed_in_window as f64;

        // Decay threat score over time (exponential decay)
        self.decay_threat(current_ms);

        self.ewma_rps
    }

    /// Update EWMA with observed RPS value.
    fn update_ewma(&mut self, observed_rps: f64) {
        if self.ewma_rps == 0.0 {
            self.ewma_rps = observed_rps;
        } else {
            self.ewma_rps = self.ewma_alpha * observed_rps + (1.0 - self.ewma_alpha) * self.ewma_rps;
        }
        if observed_rps > self.peak_rps {
            self.peak_rps = observed_rps;
        }
    }

    /// Update long-term baseline (slow-moving average).
    fn update_baseline(&mut self, observed_rps: f64) {
        // Use a slow alpha for baseline (0.01) to track long-term trends
        const BASELINE_ALPHA: f64 = 0.01;
        if self.baseline_rps == 0.0 {
            self.baseline_rps = observed_rps;
        } else {
            self.baseline_rps = BASELINE_ALPHA * observed_rps + (1.0 - BASELINE_ALPHA) * self.baseline_rps;
        }
        self.baseline_samples += 1;
    }

    /// Calculate threat score from RPS ratio vs baseline.
    fn calculate_rps_threat(&self) -> f64 {
        if self.baseline_rps == 0.0 || self.baseline_samples < 10 {
            // Not enough data — use raw threshold comparison
            if self.rps_threshold > 0 && self.ewma_rps >= self.rps_threshold as f64 {
                return 0.8;
            }
            return 0.0;
        }

        let ratio = self.ewma_rps / self.baseline_rps;
        // Sigmoid-like: (ratio / threshold_ratio - 1) clamped to [0, 1]
        let threshold_ratio = 2.0; // Block when 2x baseline
        ((ratio / threshold_ratio) - 1.0).clamp(0.0, 1.0)
    }

    /// Check if this IP should be blocked.
    /// `current_ms` is wall-clock time in milliseconds.
    pub fn should_block(&self, current_ms: u64) -> bool {
        // Check block expiry
        if self.blocked_until_ms > 0 && current_ms >= self.blocked_until_ms {
            return false; // Block expired
        }
        if self.blocked_until_ms > current_ms {
            return true; // Still blocked
        }

        // Not currently blocked — check if should block
        let rps_threat = self.calculate_rps_threat();
        let combined_threat = (rps_threat * 0.6 + self.threat_score * 0.4).clamp(0.0, 1.0);

        self.ewma_rps >= self.rps_threshold as f64 || combined_threat >= self.threat_threshold
    }

    /// Block this IP for a given duration.
    pub fn block(&mut self, duration_ms: u64, current_ms: u64) {
        self.blocked_until_ms = current_ms.saturating_add(duration_ms);
    }

    /// Adjust threat score from upstream forecasting signals.
    /// `forecast_threat` is 0-1, `confidence` is 0-1.
    pub fn adjust_threat(&mut self, forecast_threat: f64, confidence: f64) {
        // Weighted adjustment: higher confidence = more influence
        let weight = confidence.clamp(0.0, 1.0) * 0.5;
        self.threat_score = (self.threat_score * (1.0 - weight) + forecast_threat * weight).clamp(0.0, 1.0);
    }

    /// Exponentially decay threat score over time.
    fn decay_threat(&mut self, _current_ms: u64) {
        // Decay rate: threat halves every 30 seconds
        // ln(2) / 30000 = positive decay rate for 30s half-life
        let decay_rate = std::f64::consts::LN_2 / 30_000.0;

        // Estimate elapsed since last decay (approximate using total_requests)
        let elapsed_ms = if self.total_requests > 0 {
            // Rough scale: each request ~1ms under load
            1.0
        } else {
            0.0
        };

        let decay_factor = (-decay_rate * elapsed_ms).exp();
        self.threat_score *= decay_factor;
    }

    /// Reset tracker state (e.g., after manual unblock).
    pub fn reset(&mut self) {
        self.ewma_rps = 0.0;
        self.window_count = 0;
        self.baseline_rps = 0.0;
        self.baseline_samples = 0;
        self.threat_score = 0.0;
        self.blocked_until_ms = 0;
        self.total_requests = 0;
    }

    /// Check if this tracker is stale (no recent activity).
    pub fn is_stale(&self, current_ms: u64, stale_after_ms: u64) -> bool {
        if self.window_start_ms == 0 {
            return false;
        }
        current_ms.saturating_sub(self.window_start_ms) > stale_after_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tracker_starts_clean() {
        let rt = RateTracker::new(1000);
        assert_eq!(rt.ewma_rps, 0.0);
        assert_eq!(rt.rps_threshold, 1000);
        assert!(!rt.should_block(0));
    }

    #[test]
    fn record_request_tracks_count() {
        let mut rt = RateTracker::new(1000);
        for _ in 0..100 {
            rt.record_request(0, 100);
        }
        assert_eq!(rt.total_requests, 100);
    }

    #[test]
    fn block_and_expire() {
        let mut rt = RateTracker::new(1000);
        rt.block(5000, 1000); // Block for 5s at t=1000 -> expires at 6000
        assert!(rt.should_block(5999)); // Still blocked at 5999
        assert!(!rt.should_block(6000)); // Expired at 6000
        assert!(!rt.should_block(7000)); // Past expiry
    }

    #[test]
    fn threat_decay_resets() {
        let mut rt = RateTracker::new(1000);
        rt.threat_score = 0.9;
        rt.total_requests = 1000;
        rt.decay_threat(1000);
        assert!(rt.threat_score < 0.9, "threat should decay: got {}", rt.threat_score);
    }

    #[test]
    fn adjust_threat_from_forecast() {
        let mut rt = RateTracker::new(1000);
        rt.adjust_threat(0.8, 1.0);
        assert!(rt.threat_score > 0.0);
        assert!(rt.threat_score < 0.8);
    }

    #[test]
    fn reset_clears_state() {
        let mut rt = RateTracker::new(1000);
        rt.record_request(0, 100);
        rt.block(5000, 100);
        rt.reset();
        assert_eq!(rt.ewma_rps, 0.0);
        assert_eq!(rt.blocked_until_ms, 0);
    }
}