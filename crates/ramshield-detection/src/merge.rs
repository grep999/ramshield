use super::*;

/// Named outcome of updating one IP's detection state.
pub(crate) struct MergeRecordOutcome {
    pub(crate) threat_score: f32,
    pub(crate) should_block: bool,
    pub(crate) stored: bool,
}

impl DetectionEngine {
    /// Update one IP and report its detection outcome without a positional tuple.
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
    ) -> MergeRecordOutcome {
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
                let baseline_for_cusum = if prior_baseline > 0.0 && prior_baseline.is_finite() {
                    prior_baseline
                } else {
                    rec.ewma_rps
                };

                rec.sample_count = rec.sample_count.saturating_add(1);
                if rec.sample_count >= CUSUM_WARMUP_SAMPLES {
                    let k = cusum_allowance(det_thr);
                    rec.cusum_s = cusum_step_capped(
                        rec.cusum_s,
                        inst_rps,
                        baseline_for_cusum + k,
                        det_thr as f64,
                    );
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

                let block =
                    hot || cusum_fired(rec.cusum_s, det_thr) || pulse_fired || relative_fired;
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
        MergeRecordOutcome {
            threat_score: threat,
            should_block: block,
            stored,
        }
    }
}
