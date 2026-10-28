use super::*;

impl Metrics {
    /// Render all counters in Prometheus exposition format.
    /// ponytail: replaced dead stdout printer; upgrade path is
    /// metrics-exporter-prometheus if cardinality ever demands it.
    pub fn render_prometheus(&self) -> String {
        let elapsed = ((now_ms().saturating_sub(self.started_ms)) as f64 / 1000.0).max(0.001);

        macro_rules! emit {
            ($name:literal, $val:expr, $help:literal, $typ:literal) => {{
                let mut out = String::new();
                out.push_str(&format!("# HELP {} {}\n", $name, $help));
                out.push_str(&format!("# TYPE {} {}\n", $name, $typ));
                out.push_str(&format!("{} {}\n", $name, $val));
                out
            }};
        }

        let mut out = String::new();

        out.push_str(&emit!(
            "ramshield_uptime_seconds",
            elapsed as u64,
            "Process uptime in seconds.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_requests_total",
            self.requests_total.load(Ordering::Relaxed),
            "Total IPC requests received.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_blocks_total",
            self.blocks_total.load(Ordering::Relaxed),
            "Total blocks issued.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_events_ingested_total",
            self.events_ingested.load(Ordering::Relaxed),
            "Total events ingested.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_pre_aggs_flushes_total",
            self.pre_aggs_flushes.load(Ordering::Relaxed),
            "Detection pre-aggregation flushes; each one walks the whole shared aggregate map.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_frames_rejected_total",
            self.frames_rejected.load(Ordering::Relaxed),
            "IPC frames rejected at parse or auth-stripped decode.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_events_rejected_total",
            self.events_rejected.load(Ordering::Relaxed),
            "Total events rejected (conflated: auth 401s + connection refusals + channel-full drops).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_ipc_event_drops_total",
            self.ipc_event_drops.load(Ordering::Relaxed),
            "Event-channel-full drops (clean subset of events_rejected_total).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_ipc_auth_rejections_total",
            self.ipc_auth_rejections.load(Ordering::Relaxed),
            "Auth-rejected frames — 401s (clean subset of events_rejected_total).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_ipc_rejected_connections_total",
            self.ipc_rejected_connections.load(Ordering::Relaxed),
            "Connections refused at the accept semaphore (clean subset of events_rejected_total).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_events_shed_total",
            self.events_shed.load(Ordering::Relaxed),
            "Total low-signal events shed at the IPC high-water mark.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_native_ingest_drops_total",
            self.native_ingest_drops.load(Ordering::Relaxed),
            "Total observation events the native capture producer could not deliver downstream (channel full or closed).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_native_frames_walked_total",
            self.native_frames_walked.load(Ordering::Relaxed),
            "#90: frames the TPACKET_V3 drain loop walked out of completed blocks.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_native_parse_rejects_total",
            self.native_parse_rejects.load(Ordering::Relaxed),
            "#90: frames received by the capture path but rejected by L3 parsing.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_native_ingest_emitted_total",
            self.native_ingest_emitted.load(Ordering::Relaxed),
            "#90: observation events actually handed to the downstream channel.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_native_socket_packets_total",
            self.native_socket_packets.load(Ordering::Relaxed),
            "#90: frames the kernel delivered to the capture sockets (PACKET_STATISTICS).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_native_socket_drops_total",
            self.native_socket_drops.load(Ordering::Relaxed),
            "#90: frames the capture ring had no room for (PACKET_STATISTICS drops).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_native_ingest_drops_disconnected_total",
            self.native_ingest_drops_disconnected.load(Ordering::Relaxed),
            "Subset of native ingest drops caused by the downstream channel being closed rather than full.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_native_capture_workers_expected",
            self.native_capture_workers_expected.load(Ordering::Relaxed),
            "Capture worker slots the native capture path plans to run.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_native_capture_workers_started",
            self.native_capture_workers_started.load(Ordering::Relaxed),
            "Capture worker slots that were actually started.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_native_capture_workers_running",
            self.native_capture_workers_running.load(Ordering::Relaxed),
            "Capture worker slots alive right now.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_native_capture_worker_failures_total",
            self.native_capture_worker_failures.load(Ordering::Relaxed),
            "Capture attempts that failed.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_ingest_channel_depth",
            self.ingest_channel_depth.load(Ordering::Relaxed),
            "Current buffered events in the bounded ingest queue.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_ingest_channel_depth_peak",
            self.ingest_channel_depth_peak.load(Ordering::Relaxed),
            "High-water mark of the bounded ingest queue since process start.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_active_cidr_blocks",
            self.active_cidr_blocks.load(Ordering::Relaxed),
            "Active IPv4/IPv6 CIDR prefixes applied to the kernel LPM trie (userspace mirror).",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_batches_total",
            self.batches_total.load(Ordering::Relaxed),
            "Total detection batches processed.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_promotions_total",
            self.promotions_total.load(Ordering::Relaxed),
            "Total IPs promoted to tracking.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_cold_skipped_total",
            self.cold_skipped_total.load(Ordering::Relaxed),
            "Total cold IPs skipped.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_blocks_detection",
            self.blocks_detection.load(Ordering::Relaxed),
            "Blocks from detection engine.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_blocks_subnet",
            self.blocks_subnet.load(Ordering::Relaxed),
            "Blocks from subnet module.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_enforcement_dropped_total",
            self.enforcement_dropped.load(Ordering::Relaxed),
            "Block commands dropped on a full detection-to-enforcement channel (attacker unblocked in kernel).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_blocks_forecast",
            self.blocks_forecast.load(Ordering::Relaxed),
            "Blocks from forecasting.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_blocks_expired_total",
            self.blocks_expired_total.load(Ordering::Relaxed),
            "Total IP blocks that expired via TTL or manual unblock.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_wal_segments_pruned_total",
            self.wal_segments_pruned_total.load(Ordering::Relaxed),
            "Total WAL segments pruned by retention policy.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_xdp_evictions_total",
            self.xdp_evictions_total.load(Ordering::Relaxed),
            "Total XDP evictions from LRU maps.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_auth_lockout_total",
            self.auth_lockout_total.load(Ordering::Relaxed),
            "Total dashboard login lockout events.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_auth_verification_wait_ms",
            self.auth_verification_wait_ms.load(Ordering::Relaxed),
            "Latest Argon2 verification wait duration in ms.",
            "gauge"
        ));
        // The 403 breakdown: the aggregate above counts every rejected
        // request, this one counts the authorization rejections, so the
        // two can be compared.
        out.push_str(&emit!(
            "ramshield_ipc_authz_rejections_total",
            self.ipc_authz_rejections.load(Ordering::Relaxed),
            "Total IPC requests rejected with 403 for authorization reasons.",
            "counter"
        ));
        // The store's RAM refusal: the IP was dropped from tracking
        // entirely, so the count is the only trace it left.
        out.push_str(&emit!(
            "ramshield_store_ram_refusals_total",
            self.capacity_exceeded_count.load(Ordering::Relaxed),
            "Total records the store refused to admit because of the RAM budget.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_store_ram_refused_ips_total",
            self.capacity_exceeded_ips.load(Ordering::Relaxed),
            "Total IPs the store refused to admit because of the RAM budget.",
            "counter"
        ));
        // Patch A: bloom is an advisory revisit cache over PROMOTED ips (not
        // a block list). n = distinct promotes this 8s epoch, m = capacity in
        // bits. FP is estimated with k=2 (two hash slots per insert):
        //   p = (1 - e^(-2n/m))^2
        // Reported in ppm because at production n/m the useful values are
        // tiny; a percentage would round to 0.000.
        let bloom_bits = self.bloom_bits.load(Ordering::Relaxed);
        let bloom_n = self.bloom_inserts_epoch.load(Ordering::Relaxed);
        let bloom_fp_ppm = if bloom_bits == 0 {
            0u64
        } else {
            let ratio = 2.0 * bloom_n as f64 / bloom_bits as f64;
            let p = (1.0 - (-ratio).exp()).powi(2);
            (p * 1_000_000.0) as u64
        };
        out.push_str(&emit!(
            "ramshield_bloom_bits",
            bloom_bits,
            "Bloom filter capacity in bits (m).",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_bloom_inserts_epoch",
            bloom_n,
            "Distinct promoted IPs inserted into the bloom since the last clear (n).",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_bloom_clears_total",
            self.bloom_clears_total.load(Ordering::Relaxed),
            "Bloom epoch clears (advisory cache resets).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_bloom_saturation_clears_total",
            self.bloom_saturation_clears_total.load(Ordering::Relaxed),
            "Bloom saturation-driven clears (n≥m/20).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_bloom_fp_ppm",
            bloom_fp_ppm,
            "Estimated bloom false-positive rate in parts per million (k=2).",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_forecast_ticks",
            self.forecast_ticks.load(Ordering::Relaxed),
            "Forecast ticks executed.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_entropy_ticks",
            self.entropy_ticks.load(Ordering::Relaxed),
            "Entropy check ticks executed.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_hw_rps",
            Metrics::f64(&self.hw_rps_bits),
            "Holt-Winters RPS forecast.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_hw_zscore",
            Metrics::f64(&self.hw_z_bits),
            "Holt-Winters z-score.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_hw_forecast",
            Metrics::f64(&self.hw_forecast_bits),
            "Holt-Winters forecast value.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_entropy",
            Metrics::f64(&self.entropy_bits),
            "Current IP entropy.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_cgnat_classify_ticks",
            self.cgnat_classify_ticks.load(Ordering::Relaxed),
            "CGNAT graduated classification cycles.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_cgnat_tier_allow",
            self.cgnat_tier_allow.load(Ordering::Relaxed),
            "CGNAT allow-tier verdicts.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_cgnat_tier_challenge",
            self.cgnat_tier_challenge.load(Ordering::Relaxed),
            "CGNAT challenge-tier (429+JS) verdicts.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_cgnat_tier_powdrop",
            self.cgnat_tier_powdrop.load(Ordering::Relaxed),
            "CGNAT proof-of-work drop verdicts.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_cgnat_tier_block",
            self.cgnat_tier_block.load(Ordering::Relaxed),
            "CGNAT hard-block verdicts.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_shm_publish_total",
            self.shm_publish_count.load(Ordering::Relaxed),
            "SHM rule-table publishes.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_shm_lookup_total",
            self.shm_lookup_count.load(Ordering::Relaxed),
            "SHM rule-table lookups.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_shm_cache_hits",
            self.shm_cache_hits.load(Ordering::Relaxed),
            "SHM rule-table lookups hitting an active rule.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_hll_insert_total",
            self.hll_insert_count.load(Ordering::Relaxed),
            "HLL cardinality-sketch inserts.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_cms_increment_total",
            self.cms_increment_count.load(Ordering::Relaxed),
            "Count-min sketch increments.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_cms_decay_ticks",
            self.cms_decay_ticks.load(Ordering::Relaxed),
            "Count-min sketch decay sweeps.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_mesh_record_ban_total",
            self.mesh_record_ban_count.load(Ordering::Relaxed),
            "Mesh CRDT ban records.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_mesh_record_unban_total",
            self.mesh_record_unban_count.load(Ordering::Relaxed),
            "Mesh CRDT unban records.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_mesh_purge_ticks",
            self.mesh_purge_ticks.load(Ordering::Relaxed),
            "Mesh CRDT TTL-purge sweeps.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_mesh_hlc_ticks",
            self.mesh_hlc_ticks.load(Ordering::Relaxed),
            "Mesh HLC logical-clock ticks.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_xdp_v4_drops",
            self.xdp_v4_drops.load(Ordering::Relaxed),
            "XDP kernel IPv4 drops (COUNTERS PerCpuArray).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_xdp_v6_drops",
            self.xdp_v6_drops.load(Ordering::Relaxed),
            "XDP kernel IPv6 drops (COUNTERS PerCpuArray).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_xdp_attribution_gaps",
            self.xdp_attribution_gaps.load(Ordering::Relaxed),
            "XDP drop events whose source IP is not userspace-blocked (kernel/userspace drift indicator).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_xdp_blocked_ips_zero_drops",
            self.xdp_blocked_ips_zero_drops.load(Ordering::Relaxed),
            "Blocked IPs with zero attributed XDP drops since block (early-release candidate gauge).",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_xdp_wire_pass",
            self.xdp_wire_pass.load(Ordering::Relaxed),
            "XDP kernel wire passes (COUNTERS PerCpuArray).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_wal_lsn",
            self.wal_lsn.load(Ordering::Relaxed),
            "Last committed WAL sequence number.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_pending_expirations",
            self.pending_expirations.load(Ordering::Relaxed),
            "Pending TTL expirations in enforcement ring.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_xdp_parse_fails",
            self.xdp_parse_fails.load(Ordering::Relaxed),
            "XDP kernel parse failures (COUNTERS PerCpuArray).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_xdp_apply_failures_total",
            self.xdp_apply_failures.load(Ordering::Relaxed),
            "Block/unblock decisions that failed to reach the kernel; the wire keeps passing the target while userspace+ WAL believe it is blocked (CIDR trie full / XDP failure).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_xdp_projection_stale",
            self.xdp_projection_stale.load(Ordering::Relaxed),
            "Whether the active configured XDP projection is stale according to the engine freshness contract (1=true, 0=false).",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_xdp_reconcile_age_seconds",
            self.reconcile_age_seconds.load(Ordering::Relaxed),
            "Seconds since the last successful store→XDP reconciliation (grows when reconcile is failing).",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_xdp_reconcile_last_success_unix",
            self.reconcile_last_success_unix.load(Ordering::Relaxed),
            "Unix timestamp of the last successful reconciliation.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_xdp_reconcile_failures_total",
            self.reconcile_failures_total.load(Ordering::Relaxed),
            "Failed reconciliation attempts (kernel-side map update or read errors).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_xdp_reconcile_successes_total",
            self.reconcile_successes_total.load(Ordering::Relaxed),
            "Successful reconciliation runs.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_auth_login_failures_total",
            self.auth_login_failures_total.load(Ordering::Relaxed),
            "Dashboard login attempts that failed (wrong password or lockout).",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_auth_login_successes_total",
            self.auth_login_successes_total.load(Ordering::Relaxed),
            "Dashboard login attempts that succeeded.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_csrf_blocked_total",
            self.csrf_blocked_total.load(Ordering::Relaxed),
            "Dashboard API requests blocked by the cross-origin CSRF guard.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_csrf_allowed_total",
            self.csrf_allowed_total.load(Ordering::Relaxed),
            "Dashboard API requests allowed through the CSRF guard.",
            "counter"
        ));
        // No trailing println! here — every emit stanza already ends in '\n',
        // and stdout writes from a render fn were a stray-syscall bug (2026-09).
        out
    }
}
