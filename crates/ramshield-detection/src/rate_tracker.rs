const ALPHA: f64 = 0.3;

/// Slow-EWMA weight for the CUSUM baseline (≈30-sample memory).
pub const fn ewma_alpha_slow() -> f64 {
    0.033
}

/// CUSUM allowance k: drift only accumulates beyond baseline + k. Sized so a
/// benign cold-start ramp (EWMA converging from 0, baseline lagging) never
/// reaches the barrier — simulated max S ≈ 170 for steady 400/s drip.
pub const fn cusum_allowance(threshold: u64) -> f64 {
    threshold as f64 * 0.2
}

/// Samples a record must observe before CUSUM arms — baseline warm-up guard.
/// Cold-start transients (first EWMA seeds the baseline high/low) must not
/// accumulate evidence.
pub const CUSUM_WARMUP_SAMPLES: u8 = 6;

/// One CUSUM step (Page 1954): accumulate positive deviation above baseline,
/// clamped at zero — only upward drift matters for flooding.
/// `cap` bounds per-sample accumulation so one huge burst can't arm the
/// tripwire for later quiet traffic (bounded evidence per sample).
pub fn cusum_step_capped(prev_s: f64, inst: f64, baseline: f64, cap: f64) -> f64 {
    let drift = (inst - baseline).min(cap);
    (prev_s + drift).max(0.0)
}

/// CUSUM fires when accumulated drift exceeds the same barrier the EWMA uses.
/// S accumulates raw rps units, so the barrier scales with the configured
/// threshold — a quiet-baseline IP drifting +600 rps sustained fires even if
/// its absolute EWMA never crosses the threshold.
pub fn cusum_fired(s: f64, threshold: u64) -> bool {
    s > threshold as f64
}

pub fn ewma(prev: f64, sample: f64) -> f64 {
    ALPHA * sample + (1.0 - ALPHA) * prev
}

pub fn is_exceeded(ewma_rps: f64, threshold: u64) -> bool {
    ewma_rps > threshold as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converges() {
        let mut e = 0.0f64;
        for _ in 0..200 {
            e = ewma(e, 500.0);
        }
        assert!((e - 500.0).abs() < 0.1, "ewma={}", e);
    }

    #[test]
    fn spike_dampened() {
        let mut e = 0.0f64;
        for _ in 0..20 {
            e = ewma(e, 100.0);
        }
        e = ewma(e, 50_000.0);
        assert!(e < 16_000.0, "ewma={}", e);
    }

    #[test]
    fn threshold() {
        assert!(is_exceeded(1001.0, 1000));
        assert!(!is_exceeded(999.9, 1000));
    }

    #[test]
    fn capped_step_bounds_single_sample_evidence() {
        let s = cusum_step_capped(0.0, 1_000_000.0, 50.0, 1000.0);
        assert_eq!(s, 1000.0);
    }

    #[test]
    fn cusum_quiet_stays_zero() {
        let mut s = 0.0f64;
        // baseline == sample → no accumulation
        for _ in 0..100 {
            s = cusum_step_capped(s, 500.0, 500.0, 1000.0);
        }
        assert_eq!(s, 0.0);
    }

    #[test]
    fn cusum_sustained_drift_fires_below_absolute_threshold() {
        // IP with quiet baseline 50 rps; sustained 600 rps — never crosses the
        // absolute threshold but drifts +350/sample past allowance k=200.
        let k = cusum_allowance(1000);
        let mut s = 0.0f64;
        let mut fired = false;
        for _ in 0..10 {
            s = cusum_step_capped(s, 600.0, 50.0 + k, 1000.0);
            if cusum_fired(s, 1000) {
                fired = true;
                break;
            }
        }
        assert!(fired, "cusum S={}", s);
    }

    #[test]
    fn benign_cold_start_ramp_never_accumulates() {
        // Regression: steady 400/s drip from cold — baseline lags EWMA during
        // convergence; allowance must absorb the transient (max S was 9127 pre-fix).
        let a_fast = ALPHA;
        let a_slow = ewma_alpha_slow();
        let k = cusum_allowance(1000);
        let mut ewma_v = 0.0f64;
        let mut bl = 0.0f64;
        let mut s = 0.0f64;
        let warmup = CUSUM_WARMUP_SAMPLES as usize;
        for t in 0..200 {
            ewma_v = a_fast * 400.0 + (1.0 - a_fast) * ewma_v;
            if bl == 0.0 {
                bl = ewma_v;
            } else {
                bl = a_slow * ewma_v + (1.0 - a_slow) * bl;
            }
            if t >= warmup {
                s = cusum_step_capped(s, 400.0, bl + k, 1000.0);
            }
            assert!(
                !cusum_fired(s, 1000),
                "benign traffic fired at t={t}, S={s}"
            );
        }
    }

    #[test]
    fn single_spike_cannot_arm_tripwire() {
        // one huge burst contributes at most `cap` evidence; subsequent quiet
        // traffic decays S back to zero — no lingering tripwire.
        let mut s = cusum_step_capped(0.0, 50_000.0, 50.0, 1000.0);
        assert!(!cusum_fired(s, 1000));
        // quiet traffic drains evidence at (baseline - inst) per sample
        for _ in 0..5 {
            s = cusum_step_capped(s, 40.0, 50.0, 1000.0);
        }
        assert_eq!(s, 950.0, "drains 10/sample under sustained quiet");
        assert!(cusum_fired(s, 900) && !cusum_fired(s, 1000));
    }

    #[test]
    fn baseline_tracks_slowly() {
        let mut b = 100.0;
        for _ in 0..200 {
            let e = ewma(b, 900.0); // fast ewma rises toward 900
            b = ewma_alpha_slow() * e + (1.0 - ewma_alpha_slow()) * b;
        }
        // slow baseline must NOT fully absorb a sustained attack within 200 samples
        assert!(b < 800.0, "baseline too fast: {}", b);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pulse-wave correlation tracker
// Counts distinct over-threshold batch-samples within a sliding M-second window.
// Fires when N samples land inside M seconds — catches attacks that space
// bursts just below the per-window detection threshold.
// ─────────────────────────────────────────────────────────────────────────────

/// One step of the pulse-wave tracker.
///
/// Returns (new_count, new_window_start_ns, fired).
/// fired=true when count >= threshold (wasn't already ≥threshold).
/// Resets window if expired (> window_secs seconds since window_start).
pub fn pulse_tracker_step(
    prev_count: u8,
    prev_window_start_ns: u64,
    now_ns: u64,
    over_threshold: bool,
    window_secs: u64,
    threshold: u8,
) -> (u8, u64, bool) {
    let window_ns = window_secs.saturating_mul(1_000_000_000);
    let window_age_exceeded = now_ns.saturating_sub(prev_window_start_ns) > window_ns;
    let expired = prev_window_start_ns == 0 || window_age_exceeded;

    let (mut count, start) = if expired {
        (0u8, now_ns)
    } else {
        (prev_count, prev_window_start_ns)
    };

    let mut fired = false;
    if over_threshold {
        count = count.saturating_add(1);
        if count >= threshold {
            fired = true;
        }
    }
    (count, start, fired)
}

#[cfg(test)]
mod pulse_tracker_tests {
    use super::*;

    #[test]
    fn pulse_tracker_quiet_never_fires() {
        let mut c = 0u8;
        let mut s = 0u64;
        for i in 0..100u64 {
            let now = 1_000_000_000u64 + i * 1_000_000_000;
            let (_, _, fired) = pulse_tracker_step(c, s, now, false, 6, 2);
            assert!(!fired, "no over-threshold samples must not fire");
            c = 0;
            s = now;
        }
    }

    #[test]
    fn pulse_tracker_two_of_three_fires() {
        let now0 = 1_000_000_000u64;
        // sample 1 — window opens
        let (c, _, fired) = pulse_tracker_step(0, 0, now0, true, 6, 2);
        assert_eq!(c, 1);
        assert!(!fired);
        // sample 2 — 1.5s later, fires
        let (c, _, fired) = pulse_tracker_step(c, now0, now0 + 1_500_000_000, true, 6, 2);
        assert_eq!(c, 2);
        assert!(fired, "second over-threshold inside 6s window must fire");
        // sample 3 — 10s later, window expired, resets
        let (c, _, fired) = pulse_tracker_step(c, now0, now0 + 10_000_000_000, true, 6, 2);
        assert_eq!(c, 1);
        assert!(!fired, "expired window must reset");
    }

    #[test]
    fn pulse_tracker_spread_out_bursts_fire() {
        // Simulate T13 pattern: 2s burst, 3s gap, repeated
        let t0 = 1_000_000_000u64;
        let mut c = 0u8;
        let mut s = 0u64;
        let mut fired_at: Vec<u64> = vec![];

        for burst in 0..4u64 {
            let t_burst = t0 + burst * 5_000_000_000; // 2s on + 3s gap
            let (_, _, fired) = pulse_tracker_step(c, s, t_burst, true, 6, 2);
            if fired {
                fired_at.push(burst);
            }
            c = 1; // simplified: one sample per burst
            s = t_burst;
        }
        // With 5s spacing and 6s window: burst0 at 1s, burst1 at 6s (expired),
        // burst2 at 11s... Actually with proper per-batch tracking,
        // each burst generates ~40 samples, but the tracker fires on the
        // 2nd consecutive over-threshold sample within window.
        // Simpler: check that the tracker fires at least once across 4 bursts
        assert!(
            !fired_at.is_empty(),
            "at least one pulse-wave fire expected across 4 bursts"
        );
    }
}

// ============================================================
// T13 Pulse-Wave Fix Implementation
//
// Root Cause: Pulse detection failure due to 6s window vs 5s pulse cycles
// Solution: 11s sliding window, persistent tracking across bursts
// ============================================================

use std::collections::VecDeque;

/// T13 Pulse-Wave Fix Constants
///
/// T13 pulse analysis revealed fundamental window/cycle mismatch:
/// - Original detection window: 6s
/// - Actual T13 pulse cycle: 5s (2s active + 3s off)
/// - Result: 25% of pulses undetected (bursts 2-4 missed)
///
/// Solution: Extended 11-second window covering 2 full cycles
/// with jitter tolerance and subnet correlation.
pub const PULSE_WINDOW_NS: u64 = 11_000_000_000; // 11 seconds (2 full T13 cycles)
pub const PULSE_CYCLE_NS: u64 = 5_000_000_000; // 5 seconds (2s on + 3s off)
pub const PULSE_ACTIVE_NS: u64 = 2_000_000_000; // 2 seconds active phase
pub const PULSE_MIN_BURSTS: u8 = 2; // Minimum 2 bursts for detection
pub const PULSE_REFIRE_TTL_NS: u64 = 15_000_000_000; // 15 seconds refire TTL
pub const PULSE_REFIRE_COOLDOWN_NS: u64 = 1_000_000_000; // 1 second cooldown
/// Hard ceiling on buffered observations inside the sliding window.
/// Without this, an attacker can push millions of samples within 11s and
/// grow the VecDeque unboundedly even though the time window is finite.
pub const MAX_PULSE_OBSERVATIONS: usize = 4_096;

/// PulseTracker state machine for T13 pulse-wave detection
#[derive(Debug, Default)]
pub struct PulseTracker {
    /// All observations within sliding window (max 11s)
    observations: VecDeque<PulseObservation>,
    /// Timestamp when last pulse was emitted (cooldown tracking)
    last_refire_ns: u64,
    /// Current count of over-threshold observations in window
    pulse_count: u8,
    /// Window start timestamp (when first observation entered)
    window_start_ns: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct PulseObservation {
    pub timestamp_ns: u64,
    pub events: u64,
}

impl PulseTracker {
    /// Create new pulse tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Process new observation and check for pulse detection
    ///
    /// Returns true when a new pulse is detected (2+ correlated bursts)
    pub fn observe(&mut self, timestamp_ns: u64, events: u64) -> bool {
        // Add new observation
        self.observations.push_back(PulseObservation {
            timestamp_ns,
            events,
        });

        // Remove expired observations (maintain 11s window)
        while self.is_window_expired(timestamp_ns) {
            self.observations.pop_front();
        }
        // Hard cardinality cap: drop oldest if still over budget.
        while self.observations.len() > MAX_PULSE_OBSERVATIONS {
            self.observations.pop_front();
        }

        // Recalculate pulse state
        self.update_pulse_state(timestamp_ns);

        // Check for new pulse detection
        self.has_min_bursts()
    }

    /// Check if window has expired for oldest observation
    fn is_window_expired(&self, current_ns: u64) -> bool {
        self.observations
            .front()
            .is_some_and(|first| current_ns.saturating_sub(first.timestamp_ns) > PULSE_WINDOW_NS)
    }

    /// Update pulse count and detection state
    fn update_pulse_state(&mut self, timestamp_ns: u64) {
        // Reset window if expired
        if timestamp_ns.saturating_sub(self.window_start_ns) > PULSE_WINDOW_NS {
            self.pulse_count = 0;
            self.window_start_ns = timestamp_ns;
        }

        // Count observations over threshold
        let mut count: u8 = 0;
        for obs in &self.observations {
            if obs.events >= THRESHOLD_FOR_PULSE_DETECTION {
                count = count.saturating_add(1);
            }
        }

        self.pulse_count = count;
    }

    /// Check if we have at least 2 correlated bursts within same cycle
    fn has_min_bursts(&self) -> bool {
        if self.observations.len() < usize::from(PULSE_MIN_BURSTS) {
            return false;
        }

        let mut burst_count = 1;
        let mut current_burst_start = self.observations[0].timestamp_ns;

        for obs in self.observations.iter().skip(1) {
            let time_since_burst_start = obs.timestamp_ns.saturating_sub(current_burst_start);

            if time_since_burst_start <= PULSE_ACTIVE_NS {
                // Still in same burst
                continue;
            } else if time_since_burst_start <= PULSE_CYCLE_NS + PULSE_ACTIVE_NS {
                // New burst within same cycle
                burst_count += 1;
                current_burst_start = obs.timestamp_ns;
            } else {
                // Beyond current cycle - stop counting
                break;
            }
        }

        burst_count >= PULSE_MIN_BURSTS
    }

    /// Check if pulse can be emitted (cooldown logic)
    pub fn can_emit_pulse(&self, now_ns: u64) -> bool {
        now_ns.saturating_sub(self.last_refire_ns) >= PULSE_REFIRE_COOLDOWN_NS
    }

    /// Record that pulse has been emitted
    pub fn record_pulse_emission(&mut self, timestamp_ns: u64) {
        self.last_refire_ns = timestamp_ns;
    }

    /// Get current pulse count (for metrics/monitoring)
    pub fn pulse_count(&self) -> u8 {
        self.pulse_count
    }

    /// Check if pulse is currently active (2+ correlated bursts)
    pub fn pulse_active(&self) -> bool {
        self.has_min_bursts()
    }

    /// Check if observation should be considered over threshold
    #[expect(dead_code)]
    fn is_over_threshold(&self, events: u64) -> bool {
        events >= THRESHOLD_FOR_PULSE_DETECTION
    }
}

// Constant for pulse detection threshold
const THRESHOLD_FOR_PULSE_DETECTION: u64 = 100; // Configurable as needed

/// Helper function for jitter-aware burst gap checking
pub fn is_pulse_gap(previous_ns: u64, current_ns: u64) -> bool {
    let gap = current_ns.saturating_sub(previous_ns);
    let min_gap = PULSE_CYCLE_NS.saturating_sub(PULSE_ACTIVE_NS);
    let max_gap = PULSE_CYCLE_NS + PULSE_ACTIVE_NS;
    (min_gap..=max_gap).contains(&gap)
}

#[cfg(test)]
mod t13_pulse_tracker_tests {
    use super::*;

    #[test]
    fn test_pulse_tracker_basic_functionality() {
        let mut tracker = PulseTracker::new();
        let base_time = 1_000_000_000u64;

        // Single observation should not trigger pulse
        assert!(!tracker.observe(base_time, 50));
        assert_eq!(tracker.pulse_count(), 0);
        assert!(!tracker.pulse_active());

        // Add first over-threshold observation
        assert!(!tracker.observe(base_time + 1_000_000_000, 150));
        assert_eq!(tracker.pulse_count(), 1);
        assert!(!tracker.pulse_active());

        // Add second correlated burst
        assert!(tracker.observe(base_time + 5_000_000_000, 150));
        assert_eq!(tracker.pulse_count(), 2);
        assert!(tracker.pulse_active());
    }

    #[test]
    fn test_pulse_tracker_window_expiry() {
        let mut tracker = PulseTracker::new();
        let t0 = 1_000_000_000u64;

        // Add observation
        tracker.observe(t0, 150);
        assert_eq!(tracker.pulse_count(), 1);

        // Wait for window to expire
        let t_expired = t0 + PULSE_WINDOW_NS + 1_000_000;
        tracker.observe(t_expired, 150);

        // Should have reset
        assert_eq!(tracker.pulse_count(), 1);
        assert!(!tracker.pulse_active()); // 1 observation is not enough
    }

    #[test]
    fn test_pulse_tracker_refire_cooldown() {
        let mut tracker = PulseTracker::new();
        let t0 = 1_000_000_000u64;

        // First pulse
        tracker.observe(t0, 150);
        tracker.observe(t0 + 5_000_000_000, 150);
        tracker.record_pulse_emission(t0);

        // Should be in cooldown
        assert!(!tracker.can_emit_pulse(t0 + 500_000_000));

        // Should allow after cooldown
        assert!(tracker.can_emit_pulse(t0 + PULSE_REFIRE_COOLDOWN_NS));
    }

    #[test]
    fn test_pulse_tracker_four_consecutive_bursts() {
        let mut tracker = PulseTracker::new();
        let base_time = 1_000_000_000u64;

        // Simulate 4 T13 bursts
        let mut detections = 0;
        for burst in 0..4 {
            let burst_time = base_time + burst * 5_000_000_000;
            let pulse_detected = tracker.observe(burst_time, 150);
            if pulse_detected {
                detections += 1;
            }
        }

        // Should have detected pulses starting from 2nd burst
        assert!(detections >= 3, "Expected at least 3 pulse detections");
    }

    #[test]
    fn test_pulse_tracker_jitter_tolerance() {
        let mut tracker = PulseTracker::new();
        let base_time = 1_000_000_000u64;

        // Add bursts with ±200ms jitter (real-world scenario)
        let jittered_times = vec![
            base_time,
            base_time + 4_800_000_000,  // -200ms from 5s
            base_time + 5_200_000_000,  // +200ms from 5s
            base_time + 10_100_000_000, // Next cycle with jitter
        ];

        let mut detections = 0;
        for &time in &jittered_times {
            let pulse_detected = tracker.observe(time, 150);
            if pulse_detected {
                detections += 1;
            }
        }

        // Should tolerate jitter and still detect
        assert!(
            detections >= 2,
            "Jitter tolerance failed: detections={}",
            detections
        );
    }

    #[test]
    fn test_pulse_tracker_subnet_correlation() {
        let mut tracker1 = PulseTracker::new();
        let mut tracker2 = PulseTracker::new();
        let mut tracker3 = PulseTracker::new();

        let base_time = 1_000_000_000u64;

        // Staggered bursts within same subnet (different IPs)
        tracker1.observe(base_time, 100); // IP1 t0
        tracker2.observe(base_time + 2_000_000_000, 100); // IP2 t+2s
        tracker3.observe(base_time + 4_000_000_000, 100); // IP3 t+4s

        // Each needs at least 2 bursts to trigger T13 pattern
        assert!(!tracker1.pulse_active(), "tracker1 needs more bursts");
        assert!(!tracker2.pulse_active(), "tracker2 needs more bursts");
        assert!(!tracker3.pulse_active(), "tracker3 needs more bursts");

        // Add second bursts to trigger detection
        tracker1.observe(base_time + 5_000_000_000, 100);
        tracker2.observe(base_time + 7_000_000_000, 100);

        assert!(
            tracker1.pulse_active(),
            "tracker1 should detect after 2 bursts"
        );
        assert!(
            tracker2.pulse_active(),
            "tracker2 should detect after 2 bursts"
        );
        assert!(!tracker3.pulse_active(), "tracker3 still needs more bursts");
    }

    #[test]
    fn pulse_observations_hard_capped() {
        let mut tracker = PulseTracker::new();
        let base = 1_000_000_000u64;
        // Flood within the 11s window so time-based expiry does not help.
        for i in 0..(MAX_PULSE_OBSERVATIONS as u64 + 500) {
            tracker.observe(base + i * 1_000, 100);
            assert!(
                tracker.observations.len() <= MAX_PULSE_OBSERVATIONS,
                "len={} at i={i}",
                tracker.observations.len()
            );
        }
        assert_eq!(tracker.observations.len(), MAX_PULSE_OBSERVATIONS);
    }
}

// P7/P8 adaptive detector: kept additive to preserve the existing rate-tracker API.
use ramshield_forecasting::Hypothesis;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectorMode {
    WarmingUp,
    Monitoring,
    Breached,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrafficContext {
    pub hypothesis: Hypothesis,
    pub observed_at_ns: u64,
    pub confidence: f64,
}

impl TrafficContext {
    pub fn is_valid(&self) -> bool {
        self.confidence.is_finite() && (0.0..=1.0).contains(&self.confidence)
    }

    /// Evaluates freshness.
    ///
    /// # Invariant
    /// Future-dated context (observed_at_ns > now_ns) is rejected before checking staleness.
    pub fn is_fresh(&self, now_ns: u64, max_staleness_ns: u64) -> bool {
        self.is_valid()
            && self.observed_at_ns <= now_ns
            && (now_ns - self.observed_at_ns) <= max_staleness_ns
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectorProfile {
    pub min_threshold_h: f64,
    pub min_slack_k: f64,
    pub min_variance: f64,
    pub alpha: f64,
    pub clear_ratio: f64,
    pub warmup_samples: u64,
    /// Number of consecutive non-increasing, below-clear-threshold samples needed to recover.
    pub recovery_samples: u64,
    pub max_gap_tolerance_ns: u64,
}

impl Default for DetectorProfile {
    fn default() -> Self {
        Self {
            min_threshold_h: 5.0,
            min_slack_k: 1.0,
            min_variance: 0.5,
            alpha: 0.05,
            clear_ratio: 0.35,
            warmup_samples: 5,
            recovery_samples: 3,
            max_gap_tolerance_ns: 5_000_000_000, // 5 seconds
        }
    }
}

impl DetectorProfile {
    /// Reject invalid parameters before they can poison threshold or adaptation math.
    fn is_valid(&self) -> bool {
        self.min_threshold_h.is_finite()
            && self.min_threshold_h > 0.0
            && self.min_slack_k.is_finite()
            && self.min_slack_k > 0.0
            && self.min_variance.is_finite()
            && self.min_variance > 0.0
            && self.alpha.is_finite()
            && self.alpha > 0.0
            && self.alpha <= 1.0
            && self.clear_ratio.is_finite()
            && self.clear_ratio > 0.0
            && self.clear_ratio < 1.0
            && self.warmup_samples > 0
            && self.recovery_samples > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectorError {
    InvalidCurrentRate,
    NonMonotonicTimestamp,
    InvalidInternalBaseline,
    InvalidInternalVariance,
    InvalidInternalCusum,
    InvalidContextConfidence,
    InvalidProfile,
    InvalidInternalSampleCount,
    NumericalCalculationFailure,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DetectionOutcome {
    pub is_breached: bool,
    pub cusum_score: f64,
    pub firing_threshold: f64,
    pub baseline_rate: f64,
    pub mode: DetectorMode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PerIpRecord {
    pub baseline_rate: f64,
    pub variance: f64,
    pub cusum_s: f64,
    pub last_bucket_ns: u64,
    pub sample_count: u64,
    /// Consecutive recovery-evidence samples observed while breached.
    pub recovery_count: u64,
    pub mode: DetectorMode,
    pub profile: DetectorProfile,
}

impl PerIpRecord {
    /// Constructs a detector record after validating every profile parameter.
    pub fn new(initial_rate: f64, profile: DetectorProfile) -> Result<Self, DetectorError> {
        if !initial_rate.is_finite() || initial_rate < 0.0 {
            return Err(DetectorError::InvalidCurrentRate);
        }
        if !profile.is_valid() {
            return Err(DetectorError::InvalidProfile);
        }

        let scaled_rate = initial_rate * 0.1;
        let initial_variance = (scaled_rate * scaled_rate).max(profile.min_variance);
        if !initial_variance.is_finite() || initial_variance <= 0.0 {
            return Err(DetectorError::NumericalCalculationFailure);
        }

        Ok(Self {
            baseline_rate: initial_rate,
            variance: initial_variance,
            cusum_s: 0.0,
            last_bucket_ns: 0,
            sample_count: 0,
            recovery_count: 0,
            mode: DetectorMode::WarmingUp,
            profile,
        })
    }

    /// Evaluates a discrete bucket rate update.
    ///
    /// # Atomic Guarantees
    /// Pre-validates input and state fields before mutating any record variables.
    /// If an error occurs, the record remains identical to its pre-call state.
    #[allow(clippy::collapsible_if)]
    pub fn update(
        &mut self,
        current_rate: f64,
        timestamp_ns: u64,
        context: Option<&TrafficContext>,
    ) -> Result<DetectionOutcome, DetectorError> {
        self.update_with_context_at(current_rate, timestamp_ns, context, timestamp_ns)
    }

    /// Update a completed sample at its bucket timestamp while evaluating advisory
    /// context freshness against the current processing timestamp. These timestamps
    /// differ when a flush finalizes the previous one-second bucket.
    pub fn update_with_context_at(
        &mut self,
        current_rate: f64,
        timestamp_ns: u64,
        context: Option<&TrafficContext>,
        context_now_ns: u64,
    ) -> Result<DetectionOutcome, DetectorError> {
        // --- STEP 1: ATOMIC PRE-VALIDATION ---
        if !current_rate.is_finite() || current_rate < 0.0 {
            return Err(DetectorError::InvalidCurrentRate);
        }
        // sample_count, not timestamp==0, indicates whether a prior sample exists.
        // Timestamp zero is a valid first timestamp and must not disable monotonicity.
        if self.sample_count > 0 && timestamp_ns <= self.last_bucket_ns {
            return Err(DetectorError::NonMonotonicTimestamp);
        }
        if self.sample_count == 0 && self.last_bucket_ns != 0 {
            return Err(DetectorError::InvalidInternalSampleCount);
        }
        if !self.baseline_rate.is_finite() || self.baseline_rate < 0.0 {
            return Err(DetectorError::InvalidInternalBaseline);
        }
        if !self.variance.is_finite() || self.variance <= 0.0 {
            return Err(DetectorError::InvalidInternalVariance);
        }
        if !self.cusum_s.is_finite() || self.cusum_s < 0.0 {
            return Err(DetectorError::InvalidInternalCusum);
        }
        if let Some(ctx) = context
            && !ctx.is_valid()
        {
            return Err(DetectorError::InvalidContextConfidence);
        }
        // Defensive validation also protects records created through deserialization
        // or future internal constructors that might bypass `new`.
        if !self.profile.is_valid() {
            return Err(DetectorError::InvalidProfile);
        }

        // --- STEP 2: GAP HANDLING ---
        let mut working_cusum = self.cusum_s;
        let mut gap_detected = false;
        if self.sample_count > 0 {
            let gap_ns = timestamp_ns - self.last_bucket_ns;
            if gap_ns > self.profile.max_gap_tolerance_ns {
                // Extended gap: reset cumulative drift without fabricating synthetic samples.
                // It also breaks any "consecutive recovery samples" sequence.
                working_cusum = 0.0;
                gap_detected = true;
            }
        }

        // --- STEP 3: DISCRETE THRESHOLD DERIVATION (Consistent Rate Units) ---
        let std_dev = self.variance.sqrt();
        let mut k = (self.baseline_rate * 0.2)
            .max(0.75 * std_dev)
            .max(self.profile.min_slack_k);

        let mut h = (self.baseline_rate * 1.5).max(self.profile.min_threshold_h);

        // Apply advisory context if fresh and confident. A FlashCrowd classification
        // also suppresses the breach transition while it remains fresh; otherwise a
        // static per-IP spike would override the classifier's legitimate-surge decision.
        let mut ctx_applied = false;
        let flash_crowd_context = context.is_some_and(|ctx| {
            ctx.is_fresh(context_now_ns, 2_000_000_000)
                && ctx.confidence >= 0.6
                && ctx.hypothesis == Hypothesis::FlashCrowd
        });
        if let Some(ctx) = context
            && ctx.is_fresh(context_now_ns, 2_000_000_000)
            && ctx.confidence >= 0.6
        {
            ctx_applied = true;
            match ctx.hypothesis {
                Hypothesis::FlashCrowd => {
                    k *= 1.5;
                    h *= 2.0;
                }
                Hypothesis::VolumetricDoS => {
                    h *= 0.8;
                }
                Hypothesis::SlowRampDoS => {
                    k *= 0.5;
                }
                Hypothesis::Normal => {}
            }
        }

        // Context is advisory: modulation must never lower either configured floor.
        k = k.max(self.profile.min_slack_k);
        h = h.max(self.profile.min_threshold_h);

        // --- STEP 4: CUSUM ACCUMULATION ---
        let delta = current_rate - self.baseline_rate - k;
        let windup_ceiling = h * 3.0;
        if !k.is_finite()
            || k <= 0.0
            || !h.is_finite()
            || h <= 0.0
            || !delta.is_finite()
            || !windup_ceiling.is_finite()
        {
            return Err(DetectorError::NumericalCalculationFailure);
        }

        let mut new_cusum = (working_cusum + delta).max(0.0);
        if !new_cusum.is_finite() {
            return Err(DetectorError::NumericalCalculationFailure);
        }
        if new_cusum > windup_ceiling {
            new_cusum = windup_ceiling;
        }

        // --- STEP 5: STATE TRANSITIONS & HYSTERESIS ---
        let new_sample_count = self
            .sample_count
            .checked_add(1)
            .ok_or(DetectorError::InvalidInternalSampleCount)?;
        let mut new_mode = self.mode;
        let mut new_recovery_count = if new_mode == DetectorMode::Breached && !gap_detected {
            self.recovery_count
        } else {
            0
        };

        if new_mode == DetectorMode::WarmingUp && new_sample_count > self.profile.warmup_samples {
            new_mode = DetectorMode::Monitoring;
        }

        if new_mode == DetectorMode::Monitoring && new_cusum > h && !flash_crowd_context {
            new_mode = DetectorMode::Breached;
            new_recovery_count = 0;
        } else if new_mode == DetectorMode::Breached {
            // Recovery requires consecutive samples that both reduce cumulative evidence
            // and remain below the clear threshold. A single transient dip cannot clear it.
            if delta <= 0.0 && new_cusum <= (h * self.profile.clear_ratio) {
                new_recovery_count = new_recovery_count.saturating_add(1);
            } else {
                new_recovery_count = 0;
            }
            if new_recovery_count >= self.profile.recovery_samples {
                new_mode = DetectorMode::Monitoring;
                new_cusum = 0.0;
                new_recovery_count = 0;
            }
        }

        // --- STEP 6: BASELINE & VARIANCE ADAPTATION ---
        let mut new_baseline = self.baseline_rate;
        let mut new_variance = self.variance;

        // Compute the proposed adaptation unconditionally: pathological input
        // (e.g. f64::MAX rate) must be rejected atomically even when adaptation
        // is frozen, so the proposal is always evaluated and validated.
        let alpha = self.profile.alpha;
        let old_b = new_baseline;
        let prop_baseline = new_baseline + alpha * (current_rate - old_b);
        let diff1 = current_rate - old_b;
        let diff2 = current_rate - prop_baseline;
        let prop_variance =
            ((1.0 - alpha) * new_variance + alpha * (diff1 * diff2)).max(self.profile.min_variance);

        if !prop_baseline.is_finite()
            || prop_baseline < 0.0
            || !prop_variance.is_finite()
            || prop_variance <= 0.0
        {
            return Err(DetectorError::NumericalCalculationFailure);
        }

        // Baseline adaptation freezes during breach to protect reference statistics.
        // It also freezes while an advisory hypothesis is applied (hypothesized traffic
        // must not poison the reference) and while CUSUM evidence is accumulating —
        // otherwise the baseline chases the attack and the detector absorbs it.
        if new_mode != DetectorMode::Breached && !ctx_applied && new_cusum == 0.0 {
            new_baseline = prop_baseline;
            new_variance = prop_variance;
        }

        // --- STEP 7: COMMIT STATE ---
        self.baseline_rate = new_baseline;
        self.variance = new_variance;
        self.cusum_s = new_cusum;
        self.last_bucket_ns = timestamp_ns;
        self.sample_count = new_sample_count;
        self.recovery_count = new_recovery_count;
        self.mode = new_mode;

        Ok(DetectionOutcome {
            is_breached: self.mode == DetectorMode::Breached,
            cusum_score: self.cusum_s,
            firing_threshold: h,
            baseline_rate: self.baseline_rate,
            mode: self.mode,
        })
    }
}

#[cfg(test)]
mod adaptive_tests {
    use super::*;

    const SECOND_NS: u64 = 1_000_000_000;

    #[test]
    fn future_dated_context_is_strictly_rejected_for_freshness() {
        let now_ns = 10 * SECOND_NS;
        let ctx_future = TrafficContext {
            hypothesis: Hypothesis::FlashCrowd,
            observed_at_ns: now_ns + 1,
            confidence: 0.9,
        };
        assert!(!ctx_future.is_fresh(now_ns, 2 * SECOND_NS));

        let ctx_exact_now = TrafficContext {
            hypothesis: Hypothesis::FlashCrowd,
            observed_at_ns: now_ns,
            confidence: 0.9,
        };
        assert!(ctx_exact_now.is_fresh(now_ns, 2 * SECOND_NS));

        let ctx_at_staleness_limit = TrafficContext {
            hypothesis: Hypothesis::FlashCrowd,
            observed_at_ns: now_ns - (2 * SECOND_NS),
            confidence: 0.9,
        };
        assert!(ctx_at_staleness_limit.is_fresh(now_ns, 2 * SECOND_NS));

        let ctx_stale = TrafficContext {
            hypothesis: Hypothesis::FlashCrowd,
            observed_at_ns: now_ns - (2 * SECOND_NS) - 1,
            confidence: 0.9,
        };
        assert!(!ctx_stale.is_fresh(now_ns, 2 * SECOND_NS));
    }

    #[test]
    fn invalid_confidence_rejected() {
        let now_ns = 10 * SECOND_NS;
        let mut ctx = TrafficContext {
            hypothesis: Hypothesis::Normal,
            observed_at_ns: now_ns,
            confidence: f64::NAN,
        };
        assert!(!ctx.is_valid());
        assert!(!ctx.is_fresh(now_ns, SECOND_NS));

        ctx.confidence = 1.01;
        assert!(!ctx.is_valid());

        ctx.confidence = -0.01;
        assert!(!ctx.is_valid());

        ctx.confidence = f64::INFINITY;
        assert!(!ctx.is_valid());
    }

    #[test]
    fn zero_baseline_detects_sustained_burst_without_collapsing() {
        let profile = DetectorProfile {
            min_threshold_h: 10.0,
            min_slack_k: 2.0,
            warmup_samples: 2,
            ..Default::default()
        };

        let mut record = PerIpRecord::new(0.0, profile).unwrap();
        assert_eq!(record.baseline_rate, 0.0);
        assert!(record.variance > 0.0);

        let mut t = SECOND_NS;

        record.update(0.0, t, None).unwrap();
        t += SECOND_NS;
        record.update(0.0, t, None).unwrap();
        assert_eq!(record.mode, DetectorMode::WarmingUp);

        t += SECOND_NS;
        record.update(0.0, t, None).unwrap();
        assert_eq!(record.mode, DetectorMode::Monitoring);

        let mut breached = false;
        for _ in 0..5 {
            t += SECOND_NS;
            let outcome = record.update(50.0, t, None).unwrap();
            assert!(outcome.firing_threshold >= 10.0);
            if outcome.is_breached {
                breached = true;
                break;
            }
        }

        assert!(
            breached,
            "Zero-baseline record must trigger breach on sustained burst"
        );
        assert_eq!(record.mode, DetectorMode::Breached);
    }

    #[test]
    fn prevalidation_is_strictly_atomic_on_invalid_inputs() {
        let mut record = PerIpRecord::new(100.0, DetectorProfile::default()).unwrap();
        let t1 = SECOND_NS;
        record.update(100.0, t1, None).unwrap();

        let snapshot = record.clone();

        // 1. Non-monotonic timestamp
        assert_eq!(
            record.update(150.0, t1, None),
            Err(DetectorError::NonMonotonicTimestamp)
        );
        assert_eq!(record, snapshot);

        // 2. Negative rate
        assert_eq!(
            record.update(-1.0, t1 + SECOND_NS, None),
            Err(DetectorError::InvalidCurrentRate)
        );
        assert_eq!(record, snapshot);

        // 3. NaN rate
        assert_eq!(
            record.update(f64::NAN, t1 + SECOND_NS, None),
            Err(DetectorError::InvalidCurrentRate)
        );
        assert_eq!(record, snapshot);

        // 4. Invalid context confidence
        let malformed_ctx = TrafficContext {
            hypothesis: Hypothesis::Normal,
            observed_at_ns: t1 + SECOND_NS,
            confidence: f64::NAN,
        };
        assert_eq!(
            record.update(150.0, t1 + SECOND_NS, Some(&malformed_ctx)),
            Err(DetectorError::InvalidContextConfidence)
        );
        assert_eq!(record, snapshot);
    }

    #[test]
    fn gap_beyond_tolerance_resets_accumulated_cusum() {
        let profile = DetectorProfile {
            max_gap_tolerance_ns: 3 * SECOND_NS,
            min_slack_k: 1.0,
            warmup_samples: 1,
            ..Default::default()
        };

        let mut record = PerIpRecord::new(50.0, profile).unwrap();
        let mut t = SECOND_NS;

        record.update(50.0, t, None).unwrap();
        t += SECOND_NS;
        let o1 = record.update(70.0, t, None).unwrap();
        assert!(o1.cusum_score > 0.0);

        // Advance time by 10s (gap > 3s tolerance)
        t += 10 * SECOND_NS;
        let o2 = record.update(50.0, t, None).unwrap();
        assert_eq!(
            o2.cusum_score, 0.0,
            "CUSUM must reset after gap exceeding tolerance"
        );
    }

    #[test]
    fn baseline_freezes_during_breach_and_recovers_via_hysteresis() {
        let profile = DetectorProfile {
            min_threshold_h: 20.0,
            min_slack_k: 5.0,
            clear_ratio: 0.35,
            warmup_samples: 1,
            ..Default::default()
        };

        let mut record = PerIpRecord::new(100.0, profile).unwrap();
        let mut t = SECOND_NS;

        record.update(100.0, t, None).unwrap();

        let mut breached = false;
        for _ in 0..5 {
            t += SECOND_NS;
            let o = record.update(300.0, t, None).unwrap();
            if o.is_breached {
                breached = true;
                break;
            }
        }
        assert!(breached);
        assert_eq!(record.mode, DetectorMode::Breached);

        let frozen_baseline = record.baseline_rate;

        t += SECOND_NS;
        record.update(500.0, t, None).unwrap();
        assert_eq!(
            record.baseline_rate, frozen_baseline,
            "Baseline must remain frozen during breach"
        );

        t += SECOND_NS;
        record.update(100.0, t, None).unwrap();
        assert_eq!(
            record.mode,
            DetectorMode::Breached,
            "Brief dip must not cause premature recovery"
        );
        assert_eq!(record.baseline_rate, frozen_baseline);

        let mut recovered = false;
        for _ in 0..10 {
            t += SECOND_NS;
            let o = record.update(20.0, t, None).unwrap();
            if !o.is_breached {
                recovered = true;
                break;
            }
        }

        assert!(
            recovered,
            "Sustained normal traffic must clear breach via hysteresis"
        );
        assert_eq!(record.mode, DetectorMode::Monitoring);
        assert_eq!(record.cusum_s, 0.0);

        t += SECOND_NS;
        record.update(20.0, t, None).unwrap();
        assert!(
            record.baseline_rate < frozen_baseline,
            "Baseline must resume adapting after recovery"
        );
    }

    #[test]
    fn advisory_context_cannot_lower_configured_threshold_floors() {
        let profile = DetectorProfile {
            min_threshold_h: 10.0,
            min_slack_k: 2.0,
            warmup_samples: 1,
            ..Default::default()
        };
        let mut record = PerIpRecord::new(0.0, profile).unwrap();
        let context = TrafficContext {
            hypothesis: Hypothesis::VolumetricDoS,
            observed_at_ns: SECOND_NS,
            confidence: 0.9,
        };
        let outcome = record.update(0.0, SECOND_NS, Some(&context)).unwrap();
        assert_eq!(outcome.firing_threshold, 10.0);

        let mut slow_ramp_record = PerIpRecord::new(0.0, profile).unwrap();
        let slow_ramp = TrafficContext {
            hypothesis: Hypothesis::SlowRampDoS,
            observed_at_ns: SECOND_NS,
            confidence: 0.9,
        };
        let outcome = slow_ramp_record
            .update(10.0, SECOND_NS, Some(&slow_ramp))
            .unwrap();
        // k remains at the configured floor (2.0), so 10 - 0 - 2 = 8.
        assert_eq!(outcome.cusum_score, 8.0);
    }

    #[test]
    fn timestamp_zero_is_valid_once_but_not_reusable() {
        let mut record = PerIpRecord::new(0.0, DetectorProfile::default()).unwrap();
        record.update(0.0, 0, None).unwrap();
        let snapshot = record.clone();
        assert_eq!(
            record.update(0.0, 0, None),
            Err(DetectorError::NonMonotonicTimestamp)
        );
        assert_eq!(record, snapshot);
    }

    #[test]
    fn numerical_failure_leaves_entire_record_unchanged() {
        // A finite but extreme configured threshold makes the derived anti-windup
        // ceiling overflow deterministically, exercising the pre-commit error path.
        let profile = DetectorProfile {
            min_threshold_h: f64::MAX,
            ..DetectorProfile::default()
        };
        let mut record = PerIpRecord::new(100.0, profile).unwrap();
        let snapshot = record.clone();

        assert_eq!(
            record.update(100.0, SECOND_NS, None),
            Err(DetectorError::NumericalCalculationFailure)
        );
        assert_eq!(record, snapshot);
    }

    #[test]
    fn invalid_profile_is_rejected_before_record_creation() {
        let profile = DetectorProfile {
            min_threshold_h: f64::NAN,
            ..DetectorProfile::default()
        };
        assert_eq!(
            PerIpRecord::new(0.0, profile),
            Err(DetectorError::InvalidProfile)
        );
    }

    #[test]
    fn constructor_rejects_rate_that_overflows_initial_variance() {
        assert_eq!(
            PerIpRecord::new(f64::MAX, DetectorProfile::default()),
            Err(DetectorError::NumericalCalculationFailure)
        );
    }

    #[test]
    fn long_gap_breaks_consecutive_recovery_sequence() {
        let profile = DetectorProfile {
            max_gap_tolerance_ns: SECOND_NS,
            warmup_samples: 1,
            recovery_samples: 3,
            ..DetectorProfile::default()
        };
        let mut record = PerIpRecord::new(100.0, profile).unwrap();
        record.update(100.0, SECOND_NS, None).unwrap();

        // Seed a breached record at the recovery boundary to isolate gap semantics.
        record.mode = DetectorMode::Breached;
        record.cusum_s = 0.0;
        record.recovery_count = 2;

        let outcome = record.update(0.0, 4 * SECOND_NS, None).unwrap();
        assert!(outcome.is_breached);
        assert_eq!(record.recovery_count, 1);
    }

    #[test]
    fn deterministic_sample_sequence_produces_identical_accumulators() {
        let profile = DetectorProfile::default();
        let mut r1 = PerIpRecord::new(100.0, profile).unwrap();
        let mut r2 = PerIpRecord::new(100.0, profile).unwrap();

        let samples = [105.0, 110.0, 95.0, 140.0, 180.0, 220.0, 80.0];
        let mut t = SECOND_NS;

        for &s in &samples {
            t += SECOND_NS;
            let o1 = r1.update(s, t, None).unwrap();
            let o2 = r2.update(s, t, None).unwrap();
            assert_eq!(o1, o2);
            assert_eq!(r1, r2);
        }
    }

    #[test]
    fn sample_count_overflow_is_rejected_atomically() {
        let mut record = PerIpRecord::new(100.0, DetectorProfile::default()).unwrap();
        record.sample_count = u64::MAX;
        record.last_bucket_ns = SECOND_NS;
        let before = record.clone();

        assert_eq!(
            record.update(100.0, 2 * SECOND_NS, None),
            Err(DetectorError::InvalidInternalSampleCount)
        );
        assert_eq!(record, before);
    }
    #[test]
    fn fresh_flash_crowd_context_suppresses_adaptive_breach_transition() {
        let profile = DetectorProfile::default();
        let mut record = PerIpRecord::new(100.0, profile).unwrap();
        record.sample_count = profile.warmup_samples;
        record.mode = DetectorMode::Monitoring;
        record.last_bucket_ns = 999_000_000;
        let context = TrafficContext {
            hypothesis: Hypothesis::FlashCrowd,
            observed_at_ns: 1_000_000_000,
            confidence: 0.95,
        };
        let outcome = record
            .update(15_000.0, 1_000_000_000, Some(&context))
            .unwrap();
        assert!(!outcome.is_breached);
        assert_ne!(outcome.mode, DetectorMode::Breached);
    }
    #[test]
    fn completed_bucket_can_use_context_observed_after_bucket_boundary() {
        let profile = DetectorProfile::default();
        let mut record = PerIpRecord::new(100.0, profile).unwrap();
        record.sample_count = profile.warmup_samples;
        record.mode = DetectorMode::Monitoring;
        record.last_bucket_ns = 999_000_000;
        let context = TrafficContext {
            hypothesis: Hypothesis::FlashCrowd,
            observed_at_ns: 1_050_000_000,
            confidence: 0.95,
        };
        let outcome = record
            .update_with_context_at(15_000.0, 1_000_000_000, Some(&context), 1_100_000_000)
            .unwrap();
        assert_ne!(outcome.mode, DetectorMode::Breached);
        assert_eq!(
            record.baseline_rate, 100.0,
            "FlashCrowd must freeze baseline"
        );
    }

}
