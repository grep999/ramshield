use super::*;
use crate::rate_tracker::{DetectorMode, DetectorProfile, PerIpRecord};

impl DetectionEngine {
    /// ponytail: returns the 5-tuple `(ewma_rps, threat, should_block, was_blocked, stored)`.
    /// `was_blocked` is set true if the IP already had a BlockState, so callers can
    /// skip the extra store.get() they used to do.
    /// `stored` is false when a net-new key was refused by the RAM budget —
    /// callers must NOT count the IP as promoted.
    /// `sk`: pre-computed subnet key (avoids redundant `subnet_key_u128` call).
    pub(crate) fn merge_record(
        &self,
        ip: IpAddr,
        agg: &IpAgg,
        det: &DetectionConfig,
        ram_lim: usize,
        now: u64,
        sk: Option<SubnetKey>,
    ) -> (f64, f32, bool, bool, bool) {
        // P0 fix (round-4 Q3): was get() -> clone -> mutate -> insert(),
        // spanning two shard locks. A block committed by the enforcement
        // actor between the read and the write was silently reverted by
        // this flusher's stale `Clean` snapshot (attacker resurrection
        // under flood). Now everything happens inside Store::update_ip,
        // under ONE shard lock, mutating the live record in place — and
        // block_state is never written here (enforcement owns it).
        let ip_agg = agg.clone();
        let det_thr = det.rps_threshold;
        let window_ns = det.rate_window_secs * 1_000_000_000;
        let pulse_win = det.pulse_window_secs;
        let pulse_thr = det.pulse_threshold_samples;
        let ((was_blocked, (ewma_rps, threat, block)), stored) = self.store.update_ip(
            ip,
            IpRecord {
                ip,
                request_count: 0,
                ewma_rps: 0.0,
                cusum_s: 0.0,
                baseline_rps: 0.0,
                prev_sample_hot: false,
                sample_count: 0,
                relative_breach_streak: 0,
                pulse_samples_in_window: 0,
                pulse_window_start_ns: 0,
                first_seen_ns: ip_agg.first_ts_ns,
                last_seen_ns: ip_agg.last_ts_ns,
                bytes_in: 0,
                status_dist: [0; 5],
                proto_fingerprint: ip_agg.proto_fp,
                threat_score: 0.0,
                block_state: BlockState::Clean,
                variance: 0.1,
                last_bucket_ns: 0,
                adaptive_sample_count: 0,
                recovery_count: 0,
                bucket_accum_pps: 0.0,
                detector_mode: 0,
            },
            ram_lim,
            |rec| {
                let was_blocked = matches!(rec.block_state, BlockState::Blocked { .. });
                rec.request_count = rec.request_count.saturating_add(ip_agg.count as u64);
                rec.last_seen_ns = ip_agg.last_ts_ns;
                rec.bytes_in = rec.bytes_in.saturating_add(ip_agg.bytes);
                for i in 0..5 {
                    rec.status_dist[i] = rec.status_dist[i].saturating_add(ip_agg.status_dist[i]);
                }

                // P1: true instantaneous rate from the batch's own time span — NOT the
                // cumulative count/elapsed-since-first-seen (sawtooth after window
                // halving poisoned the EWMA sample).
                let span_ns = ip_agg.last_ts_ns.saturating_sub(ip_agg.first_ts_ns);
                let inst_rps = if span_ns > 0 {
                    ip_agg.count as f64 / (span_ns as f64 / 1e9)
                } else {
                    // whole batch inside one clock tick: assume 1s granularity floor
                    ip_agg.count as f64
                };
                rec.ewma_rps = ewma(rec.ewma_rps, inst_rps);

                // CUSUM baseline is evaluated from the PREVIOUS slow baseline,
                // then updated. Relative detection uses exactly the same prior
                // baseline, so the current sample cannot move its own threshold.
                let prior_baseline = rec.baseline_rps;

                rec.sample_count = rec.sample_count.saturating_add(1);

                // P7/P8 adaptive: bucket quantization — accumulate sub-second
                // counts, only advance CUSUM at completed 1-second boundaries.
                rec.bucket_accum_pps += inst_rps;
                let bucket_ready = rec.last_bucket_ns == 0
                    || now.saturating_sub(rec.last_bucket_ns) >= 1_000_000_000;

                let adaptive_blocked = if bucket_ready {
                    let rate = if rec.last_bucket_ns == 0 {
                        // First sample: use inst_rps directly.
                        inst_rps
                    } else {
                        rec.bucket_accum_pps
                    };
                    // Save state in case update() returns error (D-06 atomicity).
                    let prev_accum = rec.bucket_accum_pps;
                    let prev_bucket_ns = rec.last_bucket_ns;
                    rec.bucket_accum_pps = 0.0;
                    // Do NOT pre-set last_bucket_ns here; let tracker.update validate
                    // against its own state. We will persist the advanced timestamp
                    // on success (Ok arm) or restore on specific errors.

                    // Build PerIpRecord from IpRecord state.
                    let profile = DetectorProfile {
                        min_threshold_h: det_thr as f64 * 1.5,
                        min_slack_k: det_thr as f64 * 0.75,
                        min_variance: 0.1,
                        alpha: 0.3,
                        clear_ratio: 0.35,
                        warmup_samples: CUSUM_WARMUP_SAMPLES as u64,
                        recovery_samples: 3,
                        max_gap_tolerance_ns: 5_000_000_000,
                    };
                    let mut tracker = PerIpRecord {
                        baseline_rate: rec.baseline_rps,
                        variance: rec.variance,
                        cusum_s: rec.cusum_s,
                        last_bucket_ns: rec.last_bucket_ns,
                        sample_count: rec.adaptive_sample_count,
                        recovery_count: rec.recovery_count,
                        mode: match rec.detector_mode {
                            0 => DetectorMode::WarmingUp,
                            1 => DetectorMode::Monitoring,
                            _ => DetectorMode::Breached,
                        },
                        profile,
                    };

                    let was_breached = rec.detector_mode == DetectorMode::Breached as u8;
                    match tracker.update(rate, now, None) {
                        Ok(outcome) => {
                            // Write adaptive state back to IpRecord.
                            rec.baseline_rps = outcome.baseline_rate;
                            rec.cusum_s = outcome.cusum_score;
                            rec.variance = tracker.variance;
                            rec.adaptive_sample_count = tracker.sample_count;
                            rec.recovery_count = tracker.recovery_count;
                            rec.last_bucket_ns = tracker.last_bucket_ns;
                            rec.detector_mode = outcome.mode as u8;
                            let now_breached = outcome.mode == DetectorMode::Breached;
                            if !was_breached && now_breached {
                                self.metrics.inc_baseline_frozen_ips();
                            } else if was_breached && !now_breached {
                                self.metrics.dec_baseline_frozen_ips();
                            }
                            if outcome.is_breached {
                                self.metrics.inc_adaptive_breach();
                            }
                            outcome.is_breached
                        }
                        Err(e) => {
                            // Restore pre-update state per D-06 atomicity, but do
                            // not restore last_bucket_ns: a backwards wall-clock
                            // step (NonMonotonicTimestamp) must not keep the
                            // record failing until wall time catches up.
                            rec.bucket_accum_pps = prev_accum;
                            match e {
                                crate::rate_tracker::DetectorError::NonMonotonicTimestamp => {
                                    // Accept the current timestamp so future
                                    // samples are monotonic again; clock skew is
                                    // not, by itself, an attack signal — skip
                                    // the adaptive decision this bucket.
                                    rec.last_bucket_ns = now;
                                    self.metrics.inc_non_monotonic_timestamp();
                                    false
                                }
                                crate::rate_tracker::DetectorError::InvalidCurrentRate
                                | crate::rate_tracker::DetectorError::InvalidInternalBaseline
                                | crate::rate_tracker::DetectorError::InvalidInternalVariance
                                | crate::rate_tracker::DetectorError::InvalidInternalCusum
                                | crate::rate_tracker::DetectorError::InvalidInternalSampleCount
                                | crate::rate_tracker::DetectorError::InvalidContextConfidence
                                | crate::rate_tracker::DetectorError::InvalidProfile => {
                                    rec.last_bucket_ns = prev_bucket_ns;
                                    self.metrics.inc_detector_invalid_input();
                                    // Corrupted internal state must not silently
                                    // mean "not breached": fall back to the
                                    // static threshold check.
                                    is_exceeded(rec.ewma_rps, det_thr)
                                }
                                crate::rate_tracker::DetectorError::NumericalCalculationFailure => {
                                    // Numerical failure — fail closed: fall back
                                    // to the static threshold check rather than
                                    // treating every detector error as "not
                                    // breached".
                                    rec.last_bucket_ns = prev_bucket_ns;
                                    self.metrics.inc_numerical_calculation_failure();
                                    is_exceeded(rec.ewma_rps, det_thr)
                                }
                            }
                        }
                    }
                } else {
                    false
                };

                let relative_breach = if det.relative_enabled
                    && prior_baseline.is_finite()
                    && prior_baseline > 0.0
                    && u32::from(rec.sample_count) >= det.relative_min_samples
                {
                    let need = (det.relative_factor * prior_baseline).max(det.relative_floor_rps);
                    need.is_finite() && inst_rps.is_finite() && inst_rps >= need
                } else {
                    false
                };
                if relative_breach {
                    rec.relative_breach_streak = rec.relative_breach_streak.saturating_add(1);
                } else {
                    rec.relative_breach_streak = 0;
                }

                // Freeze the slow reference while a relative breach is being
                // established. Otherwise the detector learns the attack rate
                // into its own baseline and can outrun its breach hysteresis.
                // A non-breach resumes normal adaptation.
                if !relative_breach {
                    rec.baseline_rps = if prior_baseline > 0.0 && prior_baseline.is_finite() {
                        ewma_alpha_slow() * rec.ewma_rps
                            + (1.0 - ewma_alpha_slow()) * prior_baseline
                    } else {
                        // Seed from the observed batch rate, not the cold-start fast
                        // EWMA. Otherwise the first few samples can create an
                        // artificially low baseline and trip the relative detector
                        // during perfectly steady startup traffic.
                        inst_rps
                    };
                }

                let rps_score = (rec.ewma_rps / det_thr as f64).min(1.0);
                let total: u32 = rec.status_dist.iter().sum();
                let err_frac = rec.status_dist[4] as f64 / total.max(1) as f64;
                rec.threat_score = (rps_score * 0.7 + err_frac * 0.3).min(1.0) as f32;

                if now.saturating_sub(rec.first_seen_ns) > window_ns {
                    rec.request_count /= 2;
                    rec.first_seen_ns = now;
                }

                let ewma_rps = rec.ewma_rps;
                let threat = rec.threat_score;
                let over_threshold = is_exceeded(ewma_rps, det_thr);
                // Debounce: single noisy sample must not block. Fire on EWMA over
                // threshold twice in a row, or on accumulated CUSUM drift.
                let hot = over_threshold && rec.prev_sample_hot;
                rec.prev_sample_hot = over_threshold;
                // Pulse-wave correlation: catch 2s-on/3s-off patterns that the EWMA
                // debounce misses (EWMA decays between bursts). Counts distinct
                // over-threshold samples inside a sliding M-second window.
                let (pulse_count, pulse_start, pulse_fired) = pulse_tracker_step(
                    rec.pulse_samples_in_window,
                    rec.pulse_window_start_ns,
                    now,
                    over_threshold,
                    pulse_win,
                    pulse_thr,
                );
                rec.pulse_samples_in_window = pulse_count;
                rec.pulse_window_start_ns = pulse_start;
                let relative_fired = det.relative_enabled
                    && relative_breach
                    && rec.relative_breach_streak >= det.relative_min_breaches;

                let block = hot || pulse_fired || relative_fired || adaptive_blocked;
                (was_blocked, (ewma_rps, threat, block))
            },
        );
        // Only update subnet index when the record was actually stored.
        // When stored=false (capacity exceeded), no entry exists in inner,
        // so a ghost in subnet_index would leak forever — no eviction path
        // will ever clean it.  Gate on stored to prevent the leak.
        if stored {
            self.store.update_subnet_index(ip, sk, false);
        }
        // block emitted even when already blocked: caller relies on the
        // enforcement dedup to refresh TTL (semantics preserved from pre-fix).
        (ewma_rps, threat, block, was_blocked, stored)
    }
}
