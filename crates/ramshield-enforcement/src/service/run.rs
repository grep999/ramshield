use crate::*;

impl EnforcementService {
    pub async fn run(mut self, mut command_rx: mpsc::Receiver<EnforceCommand>) -> Result<()> {
        info!("Enforcement service started");
        let expected = self.store.get_all_blocked_ips();
        let expected_cidrs: Vec<IpNetwork> =
            self.store.active_cidrs.iter().map(|e| *e.key()).collect();
        match self.xdp.reconcile(&expected, &expected_cidrs) {
            Ok(_) => {
                self.blocked_ips = expected.into_iter().collect();
                let now_unix = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                self.metrics.record_reconcile_success(now_unix);
                info!("XDP reconciled with {} blocked IPs", self.blocked_ips.len());
            }
            Err(e) => {
                let now_unix = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                self.metrics.record_reconcile_failure(
                    now_unix,
                    self.metrics
                        .reconcile_last_success_unix
                        .load(std::sync::atomic::Ordering::Relaxed),
                );
                error!("Initial XDP reconciliation failed: {}", e)
            }
        }

        let mut tick = tokio::time::interval(Duration::from_millis(250));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Continuous store→XDP reconcile: every 40 ticks ≈ 10s. Closes map-loss
        // drift after driver reload or external map wipe without waiting for restart.
        let mut reconcile_ticks: u32 = 0;
        const RECONCILE_EVERY_TICKS: u32 = 40;
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    #[cfg(feature = "mesh")]
                    {
                        let report = self.apply_mesh_messages().await;
                        if report.received > 0 {
                            debug!(
                                received = report.received,
                                applied = report.applied,
                                ignored = report.ignored,
                                failed = report.failed,
                                "mesh application batch outcome"
                            );
                        }
                    }
                    self.expire_due().await;
                    let now_unix = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    self.metrics.tick_reconcile_age(now_unix);
                    reconcile_ticks = reconcile_ticks.wrapping_add(1);
                    if reconcile_ticks.is_multiple_of(RECONCILE_EVERY_TICKS) {
                        let expected = self.store.get_all_blocked_ips();
                        let expected_cidrs: Vec<IpNetwork> = self.store.active_cidrs.iter().map(|e| *e.key()).collect();
                        match self.xdp.reconcile(&expected, &expected_cidrs) {
                            Ok(state) => {
                                self.metrics.inc_xdp_evictions_n(state.evicted_count);
                                self.blocked_ips = expected.into_iter().collect();
                                // Reconcile can shrink the blocked set out-of-band
                                // (map wipe, external unblock): drop orphan
                                // attribution before it skews the zero-drop gauge.
                                self.drops_by_blocked
                                    .retain(|ip, _| self.blocked_ips.contains(ip));
                                let now_unix = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();
                                self.metrics.record_reconcile_success(now_unix);
                                debug!(
                                    n = self.blocked_ips.len(),
                                    "periodic XDP reconcile ok"
                                );
                            }
                            Err(e) => {
                                let now_unix = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();
                                self.metrics.record_reconcile_failure(
                                    now_unix,
                                    self.metrics
                                        .reconcile_last_success_unix
                                        .load(std::sync::atomic::Ordering::Relaxed),
                                );
                                error!("periodic XDP reconciliation failed: {e}");
                            }
                        }
                    }
                    // XDP kernel counters: drain ringbuf events, read counters
                    let drops = self.xdp.drain_drop_events();
                    self.attribute_drops(drops);
                    match self.xdp.counters() {
                        Ok(c) => {
                            self.metrics.set_xdp_counters(c[0], c[1], c[2], c[3]);
                            // 4 Hz audit: proves kernel counter deltas
                            // propagate to Metrics (SSE xdp.* series).
                            debug!(
                                v4_drops = c[0],
                                v6_drops = c[1],
                                wire_pass = c[2],
                                parse_fails = c[3],
                                "xdp counters read"
                            );
                        }
                        // Non-fatal (stub backend, missing COUNTERS map).
                        // trace: must not become a per-tick debug flood.
                        Err(e) => trace!(error = %e, "xdp counters unavailable"),
                    }
                    // ponytail: publish enforcement state to Metrics so the
                    // dashboard reads live values instead of dead zeros.
                    if let Some(lsn) = self.last_wal_lsn {
                        self.metrics.set_wal_lsn(lsn);
                    }
                    // Qual metric: drain WAL prune counter.
                    if let Some(ref w) = self.wal {
                        let n = w.take_segments_pruned();
                        if n > 0 {
                            self.metrics.inc_wal_segments_pruned_n(n);
                        }
                    }
                    self.metrics.set_pending_expirations(self.expirations.len() as u64);
                    self.metrics.set_active_cidr_blocks(self.store.active_cidrs.len());
                    // Refresh the checkpoint mirror: absolute deadlines for
                    // pending TTLs. Cheap (pending expirations are small); the
                    // checkpoint builder reads this under the barrier instead
                    // of reaching into enforcement internals.
                    if let Some(shared) = &self.checkpoint_shared {
                        let now_ns = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_nanos() as u64)
                            .unwrap_or(0);
                        let mut ip_exp = HashMap::new();
                        for (&ip, &deadline) in &self.expirations {
                            // deadline stored as whole-second bucket — it is an
                            // Instant measured against self.epoch; convert via
                            // epoch + bucket secs.
                            let bucket = deadline.0;
                            let at = self.epoch + Duration::from_secs(bucket);
                            let ns = unix_ns_from_instant(at, now_ns);
                            ip_exp.insert(ip, ns);
                        }
                        let mut cidr_exp = HashMap::new();
                        for (&network, &deadline) in &self.cidr_expirations {
                            let ns = unix_ns_from_instant(deadline, now_ns);
                            cidr_exp.insert(network, ns);
                        }
                        shared.publish(SharedState {
                            ip_expirations: ip_exp.into_iter().collect(),
                            cidr_expirations: cidr_exp.into_iter().collect(),
                        });
                    }
                    // HLC activity: published from the CRDT that owns the clock.
                    // NOTE: mesh_blocklist_len is NOT written into
                    // mesh_record_ban_count — that atomic is a cumulative
                    // counter owned by detection (record_ban). Overwriting it
                    // with len() every tick made the counter meaningless.
                    #[cfg(feature = "mesh")]
                    if let Some(mesh) = &self.mesh_blocklist {
                        self.metrics.mesh_hlc_ticks.store(
                            mesh.hlc_ticks(),
                            std::sync::atomic::Ordering::Relaxed,
                        );
                    }
                    // mesh_purge_ticks is owned by the real purge site
                    // (coordinator maintenance sweep), not this tick loop.
                    if self.shutdown.load(Ordering::Acquire) { break; }
                }
                cmd = command_rx.recv() => {
                    match cmd {
                        Some(cmd) => {
                            if let Err(e) = self.enforce(cmd).await { error!("Enforcement failed: {}", e); }
                        }
                        // All senders dropped — clean shutdown.
                        None => break,
                    }
                }
            }
            if self.shutdown.load(Ordering::Acquire) {
                break;
            }
        }
        #[cfg(feature = "mesh")]
        if let Some(handle) = &self.mesh_handle {
            handle.shutdown();
        }
        info!("Enforcement service stopped");
        Ok(())
    }

    /// Attribute drained XDP drop events (called each 250 ms tick).
    ///
    /// Invariant: attribution is OBSERVABILITY ONLY — per-IP drop counts
    /// never enter detection or the forecaster. A drop is the consequence
    /// of our own block, not independent evidence of maliciousness; feeding
    /// it to a learner would self-confirm every block (false positives can
    /// never be exonerated). Counts keyed to userspace-blocked IPs bound
    /// the map by |blocked_ips|. Unattributed drops (IP not blocked) count
    /// as kernel/userspace drift indicators.
    pub(crate) fn attribute_drops(&mut self, drops: Vec<XdpDropEvent>) {
        let mut gaps = 0u64;
        for ev in drops {
            if self.blocked_ips.contains(&ev.ip) {
                *self.drops_by_blocked.entry(ev.ip).or_insert(0) += 1;
            } else {
                gaps += 1;
            }
        }
        if gaps > 0 {
            self.metrics
                .xdp_attribution_gaps
                .fetch_add(gaps, Ordering::Relaxed);
        }
        // Map holds only IPs with >=1 drop; the rest of the blocked set is
        // zero-drop. saturating_sub guards a transient reconcile skew.
        let zero = self
            .blocked_ips
            .len()
            .saturating_sub(self.drops_by_blocked.len());
        self.metrics
            .xdp_blocked_ips_zero_drops
            .store(zero as u64, Ordering::Relaxed);
    }
}
