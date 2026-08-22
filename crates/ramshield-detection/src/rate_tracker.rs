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
