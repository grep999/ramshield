pub const ALPHA: f64 = 0.3;

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
