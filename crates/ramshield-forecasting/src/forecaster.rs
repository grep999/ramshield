use super::*;

impl PeakReservoir {
    pub(crate) const WARM_TICKS: u64 = 60;

    pub(crate) fn new(cap: usize) -> Self {
        Self {
            vals: Vec::with_capacity(cap),
            cap,
            ticks: 0,
        }
    }

    pub(crate) fn push(&mut self, dev: f64) {
        self.ticks += 1;
        if dev <= 0.0 {
            return;
        }
        if self.vals.len() == self.cap {
            // evict a random-ish old entry — reservoir sampling lite
            let idx = (self.ticks as usize) % self.cap;
            self.vals[idx] = dev;
        } else {
            self.vals.push(dev);
        }
    }

    /// Empirical (1 − tail) quantile of observed peaks, e.g. tail=0.001 → q99.9.
    pub(crate) fn extreme_quantile(&mut self, tail: f64) -> Option<f64> {
        if self.ticks < Self::WARM_TICKS || self.vals.len() < 10 {
            return None;
        }
        // ponytail: sort in-place, O(n) allocation saved per tick
        self.vals
            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx =
            ((self.vals.len() as f64) * (1.0 - tail)).clamp(0.0, (self.vals.len() - 1) as f64);
        Some(self.vals[idx as usize])
    }

    #[cfg(test)]
    pub(crate) fn warm(&self) -> bool {
        self.ticks >= Self::WARM_TICKS
    }
}

impl Forecaster {
    pub fn new(
        store: Arc<Store>,
        config: ForecastingConfig,
        enforcement_tx: mpsc::Sender<EnforceCommand>,
        metrics: Arc<Metrics>,
    ) -> Self {
        let hw = HoltWinters::new(
            config.ewma_alpha,
            config.hw_beta,
            config.hw_gamma,
            config.seasonality_period,
        );
        Self {
            store,
            config,
            enforcement_tx,
            metrics,
            hw: tokio::sync::Mutex::new(hw),
            ewma_var: tokio::sync::Mutex::new(EwmAVar::new(120)),
            cusum: tokio::sync::Mutex::new(CusumState::new(0.5, 4.0)),
            peaks: tokio::sync::Mutex::new(PeakReservoir::new(512)),
            bayesian: tokio::sync::Mutex::new(HypothesisTracker::new()),
            prev_entropy: tokio::sync::Mutex::new(0.0),
            last_slow_ramp_warn_ms: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub async fn run(self: Arc<Self>) {
        let mut t1 = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tokio::select! {
            _ = t1.tick() => { self.tick_hw().await; }
            }
        }
    }

    pub(crate) async fn tick_hw(&self) {
        let traffic = &self.store.traffic;
        let rps = traffic.events_last_second.load(Ordering::Relaxed) as f64;
        let n = traffic.unique_ips_window.load(Ordering::Relaxed);

        // ── Signal extraction ────────────────────────────────────────────────
        let (z, f) = {
            let mut hw = self.hw.lock().await;
            let f = hw.update(rps);
            let residual = rps - f;
            let z = self.ewma_var.lock().await.update(residual);
            // feed abs residual into PeakReservoir for self-calibrated extremes
            let dev = residual.abs();
            self.peaks.lock().await.push(dev);
            self.metrics.set_forecast_hw(rps, z, f);
            (z, f)
        };

        let cusum_alarm = self.cusum.lock().await.update(z);

        // ── Threat: drain sample + aggregate ─────────────────────────────────
        // P0 fix: the drained sample is passed to preemptive_block below.
        // Draining here AND again inside preemptive_block meant the second
        // drain always saw an empty queue — forecast-driven blocking was
        // dead code. The sample lives for the whole tick now.
        let threat_sample = self.store.traffic.drain_threat_sample();
        let threat = {
            let sample = &threat_sample;
            if sample.is_empty() {
                0.0
            } else {
                let mut m = 0.0f32;
                for (_, t) in sample {
                    if *t > m {
                        m = *t;
                    }
                }
                m as f64
            }
        };

        // ── Entropy delta: current Shannon entropy minus baseline ────────────
        let delta_h = {
            // ponytail: stack array, no heap alloc per tick
            let mut counts = [0u64; 256];
            let mut total = 0u64;
            for (i, slot) in self.store.traffic.subnet_window.iter().enumerate() {
                let v = slot.load(Ordering::Relaxed);
                counts[i] = v;
                total += v;
            }
            let h = if total > 100 {
                shannon_entropy(&counts, total)
            } else {
                0.0
            };
            let mut prev = self.prev_entropy.lock().await;
            let dh = if *prev == 0.0 { 0.0 } else { h - *prev };
            *prev = h;
            self.metrics.set_entropy(h);
            dh
        };

        // ── Bayesian update ──────────────────────────────────────────────────
        let hypothesis = {
            let mut bt = self.bayesian.lock().await;
            let p = bt.bayesian_update(z, delta_h, threat, cusum_alarm);
            let priors = p;
            let decision = bt.best_above_threshold();

            let priors_str = format!(
                "H0={:.3} H1={:.3} H2={:.3} H3={:.3}",
                priors[0], priors[1], priors[2], priors[3]
            );
            debug!(
                "Bayesian rps={:.1} z={:.2} ΔH={:.2} threat={:.2} cusum={} | {}",
                rps, z, delta_h, threat, cusum_alarm, priors_str
            );
            decision
        };

        // ── Type-specific response ───────────────────────────────────────────
        if n < 10 {
            trace!(
                n,
                rps = %format!("{:.1}", rps),
                z = %format!("{:.2}", z),
                delta_h = %format!("{:.2}", delta_h),
                "forecast tick: skipped, insufficient samples"
            );
            return; // not enough data for any decision
        }
        match hypothesis {
            Some((Hypothesis::VolumetricDoS, conf)) => {
                warn!(
                    "BAYESIAN H1 VOLUMETRIC conf={:.2} z={:.2} threat={:.2} rps={:.1}",
                    conf, z, threat, rps
                );
                self.preemptive_block(&threat_sample).await;
            }
            Some((Hypothesis::SlowRampDoS, conf)) => {
                // H2 persists across ticks once Bayesian confidence locks in;
                // WARN at most once per 30s, debug ticks in between (log-flood
                // fix: 1 Hz WARN spam while a low-z ramp cools down).
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let last = self
                    .last_slow_ramp_warn_ms
                    .load(std::sync::atomic::Ordering::Relaxed);
                if now_ms.saturating_sub(last) >= 30_000 {
                    warn!(
                        "BAYESIAN H2 SLOW-RAMP conf={:.2} z={:.2} cusum rps={:.1}",
                        conf, z, rps
                    );
                    self.last_slow_ramp_warn_ms
                        .store(now_ms, std::sync::atomic::Ordering::Relaxed);
                } else {
                    debug!(
                        "BAYESIAN H2 SLOW-RAMP (cooldown) conf={:.2} z={:.2} cusum rps={:.1}",
                        conf, z, rps
                    );
                }
                self.preemptive_block(&threat_sample).await;
                self.cusum.lock().await.reset();
            }
            Some((Hypothesis::FlashCrowd, conf)) => {
                info!(
                    "BAYESIAN H3 FLASH-CROWD conf={:.2} ΔH={:.2} rps={:.1} — no block",
                    conf, delta_h, rps
                );
                // intentional: flash crowd = legitimate traffic surge, no blocking
            }
            _ => {
                trace!(
                    n,
                    rps = %format!("{:.1}", rps),
                    z = %format!("{:.2}", z),
                    delta_h = %format!("{:.2}", delta_h),
                    threat = %format!("{:.2}", threat),
                    cusum_alarm,
                    "forecast tick: below all hypothesis gates"
                );
            }
        }

        // ── Legacy fallback: EWMA peak alarm (transitional, remove in v0.4) ─
        let spot_alarm = self.peaks.lock().await.extreme_quantile(0.001).map_or(
            z > self.config.anomaly_zscore,
            |q| {
                let dev = (rps - f).abs();
                dev > q
            },
        );
        if spot_alarm && z > self.config.anomaly_zscore && hypothesis.is_none() {
            warn!("LEGACY SPOT z={:.2} rps={:.1}", z, rps);
            self.preemptive_block(&threat_sample).await;
        }
    }

    pub(crate) async fn preemptive_block(&self, sample: &[(std::net::IpAddr, f32)]) {
        // P0 fix: sample is passed in (drained once per tick by tick_hw).
        // The old double-drain made this a no-op every time.
        if sample.is_empty() {
            return;
        }

        let mut n = 0usize;
        let mut rejected_q = 0u32;
        for &(ip, threat) in sample {
            if threat <= 0.7 {
                continue;
            }
            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 1,
                source: "forecasting".into(),
                actor: "system".into(),
                timestamp_utc: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                ttl_seconds: FORECAST_BLOCK_TTL_SECS,
                reason: "forecast_anomaly".into(),
                ip,
                cidr: None,
                action: EnforceAction::Block,
                evidence_source: ramshield_types::EvidenceSource::LocalSignals,
            };
            if self.enforcement_tx.try_send(cmd).is_err() {
                // Sampled: queue-full fires per IP in a burst.
                rejected_q += 1;
                if rejected_q & 0x3FF == 1 {
                    warn!(
                        ip = %ip,
                        rejected_q,
                        "enforcement queue full; forecast block rejected (sampled 1/1024)"
                    );
                }
            }
            self.metrics
                .record_block_ip(&ip, "forecast_anomaly", "forecasting");
            self.metrics.blocks_forecast.fetch_add(1, Ordering::Relaxed);
            n += 1;
        }
        if n > 0 {
            info!("pre-emptive blocks: {}", n);
        }
    }
}
