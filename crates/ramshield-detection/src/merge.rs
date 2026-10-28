use super::*;
use crate::rate_tracker::{DetectorMode, DetectorProfile, PerIpRecord, TrafficContext, cusum_fired};

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
        // One lock-free snapshot per flush; the bucket helper validates freshness at
        // the completed bucket timestamp and never takes a forecast mutex.
        let traffic_context = self.traffic_context();
        let adaptive_now_ns = ramshield_types::monotonic_now_ns();
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
                adaptive_baseline_rps: 0.0,
                adaptive_cusum_s: 0.0,
                adaptive_sample_count: 0,
                adaptive_bucket_id: 0,
                adaptive_bucket_packets: 0,
                adaptive_bucket_initialized: false,
                variance: 0.1,
                last_bucket_ns: 0,
                recovery_count: 0,
                bucket_accum_pps: 0.0,
                detector_mode: 0,
                adaptive_model_sample_count: 0,
                adaptive_model_variance: 0.1,
                adaptive_model_last_bucket_ns: 0,
                adaptive_model_recovery_count: 0,
                adaptive_model_mode: 0,
                first_seen_ns: ip_agg.first_ts_ns,
                last_seen_ns: ip_agg.last_ts_ns,
                bytes_in: 0,
                status_dist: [0; 5],
                proto_fingerprint: ip_agg.proto_fp,
                threat_score: 0.0,
                block_state: BlockState::Clean,
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

                // Preserve the existing detector's state independently. The adaptive
                // detector has its own persisted baseline/CUSUM/count so migration from
                // old IpRecord values cannot reinterpret legacy evidence under new rules.
                let prior_baseline = rec.baseline_rps;

                rec.sample_count = rec.sample_count.saturating_add(1);

                // P7/P8 adaptive: bucket quantization — accumulate sub-second
                // counts, only advance CUSUM at completed 1-second boundaries.
                rec.bucket_accum_pps += inst_rps;
                let bucket_ready = rec.last_bucket_ns == 0
                    || now.saturating_sub(rec.last_bucket_ns) >= 1_000_000_000;

                let legacy_adaptive_blocked = if bucket_ready {
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
                    rec.last_bucket_ns = now;

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

                    match tracker.update(rate, now, None) {
                        Ok(outcome) => {
                            // Write adaptive state back to IpRecord.
                            rec.baseline_rps = outcome.baseline_rate;
                            rec.cusum_s = outcome.cusum_score;
                            rec.variance = tracker.variance;
                            rec.adaptive_sample_count = tracker.sample_count;
                            rec.recovery_count = tracker.recovery_count;
                            rec.detector_mode = outcome.mode as u8;
                            outcome.is_breached
                        }
                        Err(_) => {
                            // Numerical failure — restore pre-update state per D-06 atomicity.
                            rec.bucket_accum_pps = prev_accum;
                            rec.last_bucket_ns = prev_bucket_ns;
                            self.metrics.inc_numerical_calculation_failure();
                            false
                        }
                    }
                } else {
                    false
                };

                // Evaluate context before moving the owned snapshot into the independent
                // adaptive model. Its contextual result remains authoritative under FlashCrowd.
                let flash_crowd = traffic_context.as_ref().is_some_and(|ctx| {
                    ctx.hypothesis == ramshield_forecasting::Hypothesis::FlashCrowd
                        && ctx.confidence >= 0.6
                        && ctx.is_fresh(adaptive_now_ns, 2_000_000_000)
                });
                let adaptive_breach = update_adaptive_bucket(
                    rec,
                    ip_agg.count as u64,
                    adaptive_now_ns,
                    traffic_context,
                );
                if adaptive_breach || legacy_adaptive_blocked {
                    self.metrics.inc_adaptive_breach();
                }

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

                // Freeze the legacy slow reference while relative breach evidence is
                // being established; adaptive baseline is independent and updated above.
                if !relative_breach {
                    rec.baseline_rps = if prior_baseline > 0.0 && prior_baseline.is_finite() {
                        ewma_alpha_slow() * rec.ewma_rps
                            + (1.0 - ewma_alpha_slow()) * prior_baseline
                    } else {
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

                let block = adaptive_block_gate(
                    hot,
                    adaptive_breach,
                    legacy_adaptive_blocked || cusum_fired(rec.cusum_s, det_thr),
                    pulse_fired,
                    relative_fired,
                    flash_crowd,
                );
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

/// The adaptive detector owns contextual sensitivity. During a fresh, confident
/// FlashCrowd classification, static legacy gates must not override that decision;
/// the adaptive breach remains authoritative.
fn adaptive_block_gate(
    hot: bool,
    adaptive_breach: bool,
    legacy_cusum: bool,
    pulse: bool,
    relative: bool,
    flash_crowd: bool,
) -> bool {
    adaptive_breach || (!flash_crowd && (hot || legacy_cusum || pulse || relative))
}

/// Accumulate per-IP events into discrete one-second buckets. A completed bucket
/// is submitted exactly once, at its end timestamp. State is copied first so an
/// invalid detector sample cannot partially advance the bucket or adaptive record.
fn update_adaptive_bucket(
    rec: &mut IpRecord,
    packet_count: u64,
    now_ns: u64,
    context: Option<TrafficContext>,
) -> bool {
    const BUCKET_NS: u64 = 1_000_000_000;
    let current_bucket = now_ns / BUCKET_NS;
    let mut candidate = rec.clone();

    if !candidate.adaptive_bucket_initialized {
        candidate.adaptive_bucket_initialized = true;
        candidate.adaptive_bucket_id = current_bucket;
        candidate.adaptive_bucket_packets = packet_count;
        *rec = candidate;
        return false;
    }

    if current_bucket < candidate.adaptive_bucket_id {
        // Clock moved backwards or caller supplied a different clock domain.
        return false;
    }

    if current_bucket == candidate.adaptive_bucket_id {
        candidate.adaptive_bucket_packets = candidate
            .adaptive_bucket_packets
            .saturating_add(packet_count);
        *rec = candidate;
        return false;
    }

    // Finalize the last observed bucket. If one or more whole buckets were skipped,
    // do not synthesize zero traffic; the detector's gap policy handles the elapsed time.
    let Some(end_bucket) = candidate.adaptive_bucket_id.checked_add(1) else {
        return false;
    };
    let Some(end_ns) = end_bucket.checked_mul(BUCKET_NS) else {
        return false;
    };
    let completed_rate = candidate.adaptive_bucket_packets as f64;
    let breached =
        update_adaptive_detector(&mut candidate, completed_rate, end_ns, context, now_ns);

    // Begin collecting the current bucket only after the completed bucket was
    // accepted. On a rejected sample, preserve all state including the pending bucket.
    if candidate.adaptive_model_last_bucket_ns != end_ns {
        return false;
    }
    candidate.adaptive_bucket_id = current_bucket;
    candidate.adaptive_bucket_packets = packet_count;
    candidate.adaptive_bucket_initialized = true;
    *rec = candidate;
    breached
}

/// Update the adaptive detector while preserving atomic state on rejected samples.
/// Returns true only when the accepted sample leaves the detector in Breached mode.
fn update_adaptive_detector(
    rec: &mut IpRecord,
    inst_rps: f64,
    now_ns: u64,
    context: Option<TrafficContext>,
    context_now_ns: u64,
) -> bool {
    let initial_rate = if rec.adaptive_model_sample_count == 0 {
        inst_rps
    } else if rec.adaptive_baseline_rps.is_finite() && rec.adaptive_baseline_rps >= 0.0 {
        rec.adaptive_baseline_rps
    } else {
        0.0
    };
    let Ok(mut adaptive) = PerIpRecord::new(initial_rate, DetectorProfile::default()) else {
        return false;
    };

    if rec.adaptive_model_sample_count > 0 {
        // Do not silently repair corrupt persisted state: reject this update without
        // changing any adaptive fields, so the caller can observe the failure boundary.
        if !rec.adaptive_baseline_rps.is_finite()
            || rec.adaptive_baseline_rps < 0.0
            || !rec.adaptive_cusum_s.is_finite()
            || rec.adaptive_cusum_s < 0.0
            || !rec.adaptive_model_variance.is_finite()
            || rec.adaptive_model_variance <= 0.0
            || rec.adaptive_model_mode > 2
        {
            return false;
        }
        adaptive.baseline_rate = rec.adaptive_baseline_rps;
        adaptive.cusum_s = rec.adaptive_cusum_s;
        adaptive.sample_count = rec.adaptive_model_sample_count;
        adaptive.variance = rec.adaptive_model_variance;
        adaptive.last_bucket_ns = rec.adaptive_model_last_bucket_ns;
        adaptive.recovery_count = rec.adaptive_model_recovery_count;
        adaptive.mode = match rec.adaptive_model_mode {
            0 => DetectorMode::WarmingUp,
            1 => DetectorMode::Monitoring,
            2 => DetectorMode::Breached,
            _ => unreachable!("mode was validated above"),
        };
    }

    let Ok(outcome) =
        adaptive.update_with_context_at(inst_rps, now_ns, context.as_ref(), context_now_ns)
    else {
        // PerIpRecord::update validates and calculates before committing. Do not copy
        // anything back on failure; the caller's persisted adaptive fields stay intact.
        return false;
    };

    rec.adaptive_baseline_rps = adaptive.baseline_rate;
    rec.adaptive_cusum_s = adaptive.cusum_s;
    rec.adaptive_model_sample_count = adaptive.sample_count;
    rec.adaptive_model_variance = adaptive.variance;
    rec.adaptive_model_last_bucket_ns = adaptive.last_bucket_ns;
    rec.adaptive_model_recovery_count = adaptive.recovery_count;
    rec.adaptive_model_mode = match adaptive.mode {
        DetectorMode::WarmingUp => 0,
        DetectorMode::Monitoring => 1,
        DetectorMode::Breached => 2,
    };
    outcome.is_breached
}

#[cfg(test)]
mod adaptive_integration_tests {
    use super::*;

    fn record() -> IpRecord {
        IpRecord {
            ip: "192.0.2.10".parse().unwrap(),
            request_count: 0,
            ewma_rps: 0.0,
            cusum_s: 0.0,
            baseline_rps: 0.0,
            prev_sample_hot: false,
            sample_count: 0,
            relative_breach_streak: 0,
            pulse_samples_in_window: 0,
            pulse_window_start_ns: 0,
            adaptive_baseline_rps: 0.0,
            adaptive_cusum_s: 0.0,
            adaptive_sample_count: 0,
            adaptive_bucket_id: 0,
            adaptive_bucket_packets: 0,
            adaptive_bucket_initialized: false,
            variance: 0.1,
            last_bucket_ns: 0,
            recovery_count: 0,
            bucket_accum_pps: 0.0,
            detector_mode: 0,
            adaptive_model_sample_count: 0,
            adaptive_model_variance: 0.1,
            adaptive_model_last_bucket_ns: 0,
            adaptive_model_recovery_count: 0,
            adaptive_model_mode: 0,
            first_seen_ns: 0,
            last_seen_ns: 0,
            bytes_in: 0,
            status_dist: [0; 5],
            proto_fingerprint: 0,
            threat_score: 0.0,
            block_state: BlockState::Clean,
        }
    }

    #[test]
    fn adaptive_state_persists_across_flush_updates() {
        let mut rec = record();
        let legacy_state = (
            rec.variance,
            rec.last_bucket_ns,
            rec.adaptive_sample_count,
            rec.recovery_count,
            rec.detector_mode,
        );
        assert!(!update_adaptive_detector(
            &mut rec,
            100.0,
            1_000_000_000,
            None,
            1_000_000_000
        ));
        assert_eq!(rec.adaptive_model_sample_count, 1);
        assert_eq!(
            (
                rec.variance,
                rec.last_bucket_ns,
                rec.adaptive_sample_count,
                rec.recovery_count,
                rec.detector_mode,
            ),
            legacy_state,
            "the independent adaptive detector must not mutate master-owned state"
        );
        assert!(rec.adaptive_model_variance.is_finite() && rec.adaptive_model_variance > 0.0);
        let baseline = rec.adaptive_baseline_rps;
        let variance = rec.adaptive_model_variance;

        assert!(!update_adaptive_detector(
            &mut rec,
            105.0,
            2_000_000_000,
            None,
            2_000_000_000
        ));
        assert_eq!(rec.adaptive_model_sample_count, 2);
        assert!(rec.adaptive_model_last_bucket_ns == 2_000_000_000);
        assert!(rec.adaptive_model_variance.is_finite() && rec.adaptive_model_variance > 0.0);
        assert_ne!(rec.adaptive_baseline_rps, baseline);
        assert_ne!(rec.adaptive_model_variance, variance);
    }

    #[test]
    fn rejected_adaptive_sample_leaves_persisted_state_unchanged() {
        let mut rec = record();
        assert!(!update_adaptive_detector(
            &mut rec,
            100.0,
            1_000_000_000,
            None,
            1_000_000_000
        ));
        let before = (
            rec.adaptive_baseline_rps,
            rec.adaptive_cusum_s,
            rec.adaptive_model_sample_count,
            rec.adaptive_model_variance,
            rec.adaptive_model_last_bucket_ns,
            rec.adaptive_model_recovery_count,
            rec.adaptive_model_mode,
        );
        assert!(!update_adaptive_detector(
            &mut rec,
            100.0,
            1_000_000_000,
            None,
            1_000_000_000
        ));
        let after = (
            rec.adaptive_baseline_rps,
            rec.adaptive_cusum_s,
            rec.adaptive_model_sample_count,
            rec.adaptive_model_variance,
            rec.adaptive_model_last_bucket_ns,
            rec.adaptive_model_recovery_count,
            rec.adaptive_model_mode,
        );
        assert_eq!(before, after);
    }

    #[test]
    fn invalid_rate_does_not_mutate_adaptive_state() {
        let mut rec = record();
        assert!(!update_adaptive_detector(
            &mut rec,
            100.0,
            1_000_000_000,
            None,
            1_000_000_000
        ));
        let before = (
            rec.adaptive_baseline_rps,
            rec.adaptive_cusum_s,
            rec.adaptive_model_sample_count,
            rec.adaptive_model_variance,
            rec.adaptive_model_last_bucket_ns,
            rec.adaptive_model_recovery_count,
            rec.adaptive_model_mode,
        );
        assert!(!update_adaptive_detector(
            &mut rec,
            f64::NAN,
            2_000_000_000,
            None,
            2_000_000_000
        ));
        let after = (
            rec.adaptive_baseline_rps,
            rec.adaptive_cusum_s,
            rec.adaptive_model_sample_count,
            rec.adaptive_model_variance,
            rec.adaptive_model_last_bucket_ns,
            rec.adaptive_model_recovery_count,
            rec.adaptive_model_mode,
        );
        assert_eq!(before, after);
    }
    #[test]
    fn corrupt_persisted_adaptive_state_is_not_silently_repaired() {
        let mut rec = record();
        assert!(!update_adaptive_detector(
            &mut rec,
            100.0,
            1_000_000_000,
            None,
            1_000_000_000
        ));
        rec.adaptive_model_variance = f64::NAN;
        let before = (
            rec.adaptive_baseline_rps,
            rec.adaptive_cusum_s,
            rec.adaptive_model_sample_count,
            rec.adaptive_model_variance.to_bits(),
            rec.adaptive_model_last_bucket_ns,
            rec.adaptive_model_recovery_count,
            rec.adaptive_model_mode,
        );

        assert!(!update_adaptive_detector(
            &mut rec,
            105.0,
            2_000_000_000,
            None,
            2_000_000_000
        ));
        let after = (
            rec.adaptive_baseline_rps,
            rec.adaptive_cusum_s,
            rec.adaptive_model_sample_count,
            rec.adaptive_model_variance.to_bits(),
            rec.adaptive_model_last_bucket_ns,
            rec.adaptive_model_recovery_count,
            rec.adaptive_model_mode,
        );
        assert_eq!(before, after);
    }

    #[test]
    fn adaptive_breach_signal_reaches_the_runtime_gate() {
        let mut rec = record();
        assert!(!update_adaptive_detector(
            &mut rec,
            100.0,
            1_000_000_000,
            None,
            1_000_000_000
        ));
        let mut breached = false;
        for i in 1..=8 {
            let now = (i + 1) * 1_000_000_000;
            breached |= update_adaptive_detector(&mut rec, 300.0, now, None, now);
        }
        assert!(
            breached,
            "sustained upward drift must reach the runtime breach signal"
        );
        assert_eq!(rec.adaptive_model_mode, 2);
    }
    #[test]
    fn flash_crowd_suppresses_static_legacy_gates_but_not_adaptive_breach() {
        assert!(!adaptive_block_gate(true, false, true, true, true, true));
        assert!(adaptive_block_gate(false, true, false, false, false, true));
        assert!(adaptive_block_gate(true, false, false, false, false, false));
    }

    #[test]
    fn adaptive_detector_only_consumes_completed_one_second_buckets() {
        let mut rec = record();
        assert!(!update_adaptive_bucket(&mut rec, 10, 100_000_000, None));
        assert!(!update_adaptive_bucket(&mut rec, 20, 800_000_000, None));
        assert_eq!(rec.adaptive_model_sample_count, 0);
        assert_eq!(rec.adaptive_bucket_packets, 30);

        assert!(!update_adaptive_bucket(&mut rec, 5, 1_100_000_000, None));
        assert_eq!(rec.adaptive_model_sample_count, 1);
        assert_eq!(rec.adaptive_model_last_bucket_ns, 1_000_000_000);
        assert_eq!(rec.adaptive_bucket_id, 1);
        assert_eq!(rec.adaptive_bucket_packets, 5);
    }

    #[test]
    fn adaptive_bucket_rejects_backward_clock_without_mutation() {
        let mut rec = record();
        assert!(!update_adaptive_bucket(&mut rec, 10, 1_100_000_000, None));
        let before = (
            rec.adaptive_bucket_id,
            rec.adaptive_bucket_packets,
            rec.adaptive_bucket_initialized,
            rec.adaptive_model_sample_count,
            rec.adaptive_model_last_bucket_ns,
        );
        assert!(!update_adaptive_bucket(&mut rec, 5, 900_000_000, None));
        let after = (
            rec.adaptive_bucket_id,
            rec.adaptive_bucket_packets,
            rec.adaptive_bucket_initialized,
            rec.adaptive_model_sample_count,
            rec.adaptive_model_last_bucket_ns,
        );
        assert_eq!(before, after);
    }
}
