use super::*;

impl Metrics {
    pub fn inc_requests(&self) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_blocks(&self) {
        self.blocks_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_ingested(&self, n: u64) {
        self.events_ingested.fetch_add(n, Ordering::Relaxed);
    }

    /// Record one detection pre-aggregation flush.
    pub fn inc_pre_aggs_flush(&self) {
        self.pre_aggs_flushes.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_rejected(&self, n: u64) {
        self.events_rejected.fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc_ipc_event_drops(&self, n: u64) {
        self.ipc_event_drops.fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc_ipc_auth_rejections(&self, n: u64) {
        self.ipc_auth_rejections.fetch_add(n, Ordering::Relaxed);
    }

    /// P2: frames rejected at authorization (403 — auth ok, role insufficient).
    pub fn inc_ipc_authz_rejections(&self, n: u64) {
        self.ipc_authz_rejections.fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc_ipc_rejected_connections(&self, n: u64) {
        self.ipc_rejected_connections
            .fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc_frames_rejected(&self) {
        self.frames_rejected.fetch_add(1, Ordering::Relaxed);
    }

    /// A block command was dropped because the detection→enforcement channel
    /// was full. Distinct from `capacity_exceeded_*` (store RAM refusal):
    /// here the IP WAS blocked in memory but the kernel never learned.
    pub fn inc_enforcement_dropped(&self) {
        self.enforcement_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// A block/unblock decision failed to reach the kernel (map insert error:
    /// CIDR LPM trie full, or a transient XDP load/attach failure). Userspace
    /// and the WAL still hold it; the wire does not. Mirror of
    /// inc_enforcement_dropped for the dataplane leg.
    pub fn inc_xdp_apply_failures(&self) {
        self.xdp_apply_failures.fetch_add(1, Ordering::Relaxed);
    }

    /// Low-signal events shed at the IPC high-water mark to preserve space
    /// for attack telemetry (status>=400, anomalous fingerprint, >64 KiB).
    pub fn inc_shed(&self, n: u64) {
        self.events_shed.fetch_add(n, Ordering::Relaxed);
    }

    /// The native capture producer could not deliver an observation event
    /// downstream: the channel was full (backpressure) or closed (the
    /// detection pipeline is gone).
    ///
    /// The native path sends without blocking, so a drop is the intended
    /// overload policy — but it is only acceptable if it is *visible*. This
    /// counter is that visibility.
    pub fn inc_native_ingest_drop(&self, n: u64) {
        self.native_ingest_drops.fetch_add(n, Ordering::Relaxed);
    }

    /// The native capture producer dropped an observation event because the
    /// downstream channel was **closed**. This is the strictly more serious
    /// half of `inc_native_ingest_drop`: the pipeline is gone, not busy.
    pub fn inc_native_ingest_disconnect(&self, n: u64) {
        self.native_ingest_drops_disconnected
            .fetch_add(n, Ordering::Relaxed);
    }

    /// The native capture path plans this many worker slots (gauge). Set
    /// once, when the capture path starts.
    pub fn set_native_capture_workers_expected(&self, n: u64) {
        self.native_capture_workers_expected
            .store(n, Ordering::Relaxed);
    }

    /// This many capture worker slots were actually started (gauge). A
    /// spawn failure is a real loss of capture capacity, so it is
    /// counted here rather than swallowed.
    pub fn set_native_capture_workers_started(&self, n: u64) {
        self.native_capture_workers_started
            .store(n, Ordering::Relaxed);
    }

    /// A capture worker slot became alive (gauge).
    pub fn inc_native_capture_workers_running(&self) {
        self.native_capture_workers_running
            .fetch_add(1, Ordering::Relaxed);
    }

    /// A capture worker slot exited (gauge).
    pub fn dec_native_capture_workers_running(&self) {
        self.native_capture_workers_running
            .fetch_sub(1, Ordering::Relaxed);
    }

    /// A capture attempt failed (counter, monotonic).
    pub fn inc_native_capture_worker_failure(&self) {
        self.native_capture_worker_failures
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Patch A: total bloom capacity in bits (gauge). Written once at
    /// detection construction; `bloom_fp_ppm` is derived from it at render.
    pub fn set_bloom_bits(&self, bits: u64) {
        self.bloom_bits.store(bits, Ordering::Relaxed);
    }

    /// Patch A: add this flush's distinct promotes to the current epoch.
    /// `n` is the promoted count, NOT the block count — the bloom tracks
    /// promoted IPs so a promote-without-block host is still revisitable.
    pub fn record_bloom_inserts(&self, n: u64) {
        self.bloom_inserts_epoch.fetch_add(n, Ordering::Relaxed);
    }

    /// Patch A: 8s bloom epoch ended. Zero the insert counter so
    /// `bloom_fp_ppm` returns to 0, and bump the clears counter.
    pub fn bloom_epoch_clear(&self) {
        self.bloom_inserts_epoch.store(0, Ordering::Relaxed);
        self.bloom_clears_total.fetch_add(1, Ordering::Relaxed);
    }

    /// D6: count saturation-driven clears (n≥m/20 threshold) separately from
    /// timed epoch clears. Ops can alert on chronic undersize without
    /// drowning in routine 8s clear noise.
    pub fn record_bloom_saturation_clear(&self) {
        self.bloom_saturation_clears_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Gauge: bounded ingest-queue occupancy. Written by the engine snapshot
    /// path (dashboard refresh + SSE), same source value on both writers.
    ///
    /// This is also where the high-water mark is maintained: the gauge is the
    /// only place the depth is observed, so tracking the peak here means the
    /// peak cannot drift out of step with the value it summarises.
    pub fn set_channel_depth(&self, depth: usize) {
        let depth = depth as u64;
        self.ingest_channel_depth.store(depth, Ordering::Relaxed);
        // fetch_max only ever raises the value, so the peak is monotonic
        // without a compare-and-swap loop and without a lock.
        self.ingest_channel_depth_peak
            .fetch_max(depth, Ordering::Relaxed);
    }

    /// Gauge: userspace mirror of active CIDR LPM blocks. Written by the
    /// enforcement 250 ms tick from EnforcementService::active_cidrs.
    pub fn set_active_cidr_blocks(&self, n: usize) {
        self.active_cidr_blocks.store(n as u64, Ordering::Relaxed);
    }

    pub fn set_wal_lsn(&self, lsn: u64) {
        self.wal_lsn.store(lsn, Ordering::Relaxed);
    }

    pub fn set_pending_expirations(&self, n: u64) {
        self.pending_expirations.store(n, Ordering::Relaxed);
    }

    /// Refresh the age gauge on every tick (successful or not) so it tracks
    /// wall time even when reconcile attempts succeed silently.
    /// Record a failed dashboard login attempt. Increments the per-IP
    /// failure window in AuthState and the global counter.
    pub fn inc_auth_login_failure(&self) {
        self.auth_login_failures_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record a successful dashboard login.
    pub fn inc_auth_login_success(&self) {
        self.auth_login_successes_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record an out-of-order sample drop (arrived before 1s bucket boundary).
    pub fn inc_out_of_order_sample_dropped(&self) {
        self.out_of_order_sample_dropped_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record a numerical calculation failure (CUSUM, variance, windup ceiling).
    pub fn inc_numerical_calculation_failure(&self) {
        self.numerical_calculation_failure_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record an adaptive breach triggered via the CUSUM detector.
    pub fn inc_adaptive_breach(&self) {
        self.adaptive_breach_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the baseline-frozen IPs gauge (current count of IPs in Breached mode).
    pub fn inc_baseline_frozen_ips(&self) {
        self.baseline_frozen_ips_gauge
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record a dashboard API request blocked by the cross-origin CSRF guard.
    pub fn inc_csrf_blocked(&self) {
        self.csrf_blocked_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a dashboard API request allowed through the CSRF guard.
    pub fn inc_csrf_allowed(&self) {
        self.csrf_allowed_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a TTL-expired block transition to Unblock (qual metric).
    pub fn inc_blocks_expired(&self) {
        self.blocks_expired_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a WAL segment pruned by retention.
    pub fn inc_wal_segments_pruned(&self) {
        self.wal_segments_pruned_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Add multiple WAL segments pruned at once (retention batch).
    pub fn inc_wal_segments_pruned_n(&self, n: u64) {
        if n > 0 {
            self.wal_segments_pruned_total
                .fetch_add(n, Ordering::Relaxed);
        }
    }

    /// Record an XDP eviction (LRU map pressure).
    pub fn inc_xdp_evictions(&self) {
        self.xdp_evictions_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record multiple XDP evictions at once (post-reconcile batch).
    pub fn inc_xdp_evictions_n(&self, n: u64) {
        if n > 0 {
            self.xdp_evictions_total.fetch_add(n, Ordering::Relaxed);
        }
    }

    /// Record a dashboard login lockout event.
    pub fn inc_auth_lockout(&self) {
        self.auth_lockout_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Update the latest Argon2 verification wait (gauge in ms).
    pub fn set_auth_verification_wait_ms(&self, val: u64) {
        self.auth_verification_wait_ms.store(val, Ordering::Relaxed);
    }

    // ponytail: P2/P3 module counters — writer methods so the dashboard reads
    // live values instead of dead zeros. Each caller owns its Arc<Metrics>
    // and calls the matching inc_* on the hot path.

    /// CGNAT graduated-mitigation verdicts.
    pub fn inc_cgnat_classify(&self) {
        self.cgnat_classify_ticks.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_cgnat_allow(&self) {
        self.cgnat_tier_allow.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_cgnat_challenge(&self) {
        self.cgnat_tier_challenge.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_cgnat_powdrop(&self) {
        self.cgnat_tier_powdrop.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_cgnat_block(&self) {
        self.cgnat_tier_block.fetch_add(1, Ordering::Relaxed);
    }

    /// Analytics streaming counters.
    pub fn inc_hll_insert(&self) {
        self.hll_insert_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_hll_inserts(&self, n: u64) {
        self.hll_insert_count.fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc_cms_increment(&self) {
        self.cms_increment_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_cms_increments(&self, n: u64) {
        self.cms_increment_count.fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc_shm_publish(&self) {
        self.shm_publish_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_shm_lookup(&self) {
        self.shm_lookup_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_shm_cache_hit(&self) {
        self.shm_cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_cms_decay(&self) {
        self.cms_decay_ticks.fetch_add(1, Ordering::Relaxed);
    }

    /// Mesh CRDT counters.
    pub fn inc_mesh_record_ban(&self) {
        self.mesh_record_ban_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_mesh_record_unban(&self) {
        self.mesh_record_unban_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_mesh_purge(&self) {
        self.mesh_purge_ticks.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_mesh_hlc(&self) {
        self.mesh_hlc_ticks.fetch_add(1, Ordering::Relaxed);
    }
}
