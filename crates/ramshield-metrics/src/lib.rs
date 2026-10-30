use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const XDP_PROJECTION_STALE_SECS: u64 = 60;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::System;

mod cache;
mod counters;
mod history;
mod prometheus;
mod system;
mod types;
mod xdp_health;

pub use system::{get_system_usage, now_ms};
pub use types::{
    BatchRecord, BlockRecord, DashboardSnapshot, ModuleStats, PipelineFlow, ProtectionState,
    SubnetRow,
};

const HISTORY: usize = 80;

pub struct Metrics {
    pub requests_total: Arc<AtomicU64>,
    pub blocks_total: Arc<AtomicU64>,
    pub events_ingested: Arc<AtomicU64>,
    /// Number of detection pre-aggregation flushes.
    ///
    /// Each flush walks the whole shared pre-aggregation map, so the flush
    /// rate is the direct driver of the detection path's CPU cost at high IP
    /// cardinality. Exported so an operator can watch it.
    pub pre_aggs_flushes: Arc<AtomicU64>,
    /// Conflated aggregate of IPC rejections: auth 401s + connection
    /// refusals + channel-full event drops. Clean breakdown:
    /// ipc_event_drops / ipc_auth_rejections / ipc_rejected_connections.
    pub events_rejected: Arc<AtomicU64>,
    /// Channel-full event drops (clean breakdown of events_rejected).
    pub ipc_event_drops: Arc<AtomicU64>,
    /// IPC frames rejected at auth — 401s (clean breakdown of events_rejected).
    pub ipc_auth_rejections: Arc<AtomicU64>,
    /// IPC frames rejected at authorization — 403s (clean breakdown of
    /// events_rejected). Auth passed but role insufficient for the request.
    pub ipc_authz_rejections: Arc<AtomicU64>,
    /// Connections refused at the semaphore (clean breakdown of events_rejected).
    pub ipc_rejected_connections: Arc<AtomicU64>,
    pub frames_rejected: Arc<AtomicU64>,
    /// Low-signal events shed at the IPC high-water mark to preserve space
    /// for attack telemetry (status>=400, anomalous fp, >64 KiB).
    pub events_shed: Arc<AtomicU64>,
    /// Observation events the native capture producer built but could not
    /// hand to the downstream channel. The native ingest path uses a
    /// non-blocking send, so a saturated or closed channel drops the event
    /// rather than stalling the capture thread; this counter is the only
    /// operator-visible record that it happened.
    ///
    /// Scope: **channel rejections only**. A packet that never becomes an
    /// event — a non-IP frame, a trusted-overlay skip, or one refused by the
    /// per-second admission budget — is not counted here, because no event
    /// was built to deliver.
    pub native_ingest_drops: Arc<AtomicU64>,
    /// #90: raw frames the V3 drain loop walked out of completed blocks.
    /// Compare against packets on the wire and against
    /// `native_ingest_emitted` to locate under-capture: walked << wire means
    /// the walk aborts early; walked ~= wire but emitted << walked means
    /// events are lost between parse and send.
    pub native_frames_walked: Arc<AtomicU64>,
    /// #90: frames process_packet received but parse_l3 rejected (non-IP,
    /// truncated, trusted-overlay skip). These are intentionally not events.
    pub native_parse_rejects: Arc<AtomicU64>,
    /// TPACKET_V3 descriptors rejected before L3 parsing due to invalid block,
    /// header, packet-range, or descriptor-chain metadata.
    pub native_tpacket_rejects: Arc<AtomicU64>,
    /// #90: events actually handed to the downstream channel.
    pub native_ingest_emitted: Arc<AtomicU64>,
    /// #90: frames the kernel reports delivered to each capture socket
    /// (PACKET_STATISTICS.tp_packets, accumulated per epoch). Compare with
    /// native_frames_walked to separate kernel-side loss from walk loss.
    pub native_socket_packets: Arc<AtomicU64>,
    /// #90: frames the ring had no room for (PACKET_STATISTICS.tp_drops).
    pub native_socket_drops: Arc<AtomicU64>,
    /// #90: descriptors the completed TPACKET_V3 blocks declared
    /// (`num_pkts` summed per block). Declared minus walked is what the
    /// walker never reached: early return, rejected block, or bad chain.
    pub native_frames_declared: Arc<AtomicU64>,
    /// #90: descriptors that produced a validated packet slice and entered
    /// the L3/delivery path. Every one of these takes exactly one terminal
    /// outcome (parse reject, overlay skip, rate limit, delivered, drop) —
    /// the accounting invariant asserted by the regression test.
    pub native_frames_validated: Arc<AtomicU64>,
    /// #90: validated frames deliberately not turned into events because
    /// the source is a shared overlay/LB address without an overlay identity.
    pub native_overlay_skips: Arc<AtomicU64>,
    /// #90: validated frames refused by the per-second admission budget.
    /// The kernel captured and the walker validated them; they are not
    /// a capture loss, but they are not detection events either.
    pub native_rate_limit_skips: Arc<AtomicU64>,
    /// The subset of `native_ingest_drops` caused by the downstream channel
    /// being **closed** rather than full. A closed channel means the
    /// detection pipeline is gone, not merely busy — a strictly more serious
    /// condition, and it is broken out so an operator can tell the two apart.
    pub native_ingest_drops_disconnected: Arc<AtomicU64>,
    /// How many capture worker slots the native path plans to run (gauge).
    /// Set once, when the capture path starts.
    pub native_capture_workers_expected: Arc<AtomicU64>,
    /// How many of those slots were actually started (gauge). A spawn
    /// failure is a real loss of capture capacity, so it is counted here
    /// rather than swallowed.
    pub native_capture_workers_started: Arc<AtomicU64>,
    /// How many capture worker slots are alive right now (gauge). A slot
    /// that is between two capture attempts is still alive, so a retry
    /// does not read as "capture gone".
    pub native_capture_workers_running: Arc<AtomicU64>,
    /// How many capture attempts have failed (counter, monotonic).
    pub native_capture_worker_failures: Arc<AtomicU64>,
    /// Instantaneous bounded ingest-queue occupancy (gauge; written by the
    /// engine snapshot path — see Engine::dashboard_snapshot / get_module_stats).
    pub ingest_channel_depth: Arc<AtomicU64>,
    /// High-water mark of `ingest_channel_depth` since process start (gauge).
    ///
    /// The instantaneous depth says how full the queue is right now; the peak
    /// says how close it has ever come to saturation, which is what tells an
    /// operator whether the drops reported by `native_ingest_drops` are a
    /// symptom of a queue that is genuinely too small rather than a brief
    /// spike. It only ever rises.
    pub ingest_channel_depth_peak: Arc<AtomicU64>,
    /// Userspace mirror of active CIDR LPM blocks (gauge; written by the
    /// enforcement tick). Kernel BLOCKCIDR maps are authoritative; mirror
    /// exists for cheap telemetry without per-tick map iteration.
    pub active_cidr_blocks: Arc<AtomicU64>,
    pub batches_total: Arc<AtomicU64>,
    pub promotions_total: Arc<AtomicU64>,
    pub cold_skipped_total: Arc<AtomicU64>,
    pub blocks_detection: Arc<AtomicU64>,
    pub blocks_subnet: Arc<AtomicU64>,
    pub blocks_forecast: Arc<AtomicU64>,
    /// Block commands dropped because the detection→enforcement channel was
    /// full. A dropped security command means the attacker keeps flooding
    /// while the engine believes the IP is blocked; this counter is the only
    /// operator-visible signal that happened.
    pub enforcement_dropped: Arc<AtomicU64>,
    /// Track when detection updates fail due to capacity exceeded
    pub capacity_exceeded_count: Arc<AtomicU64>,
    /// Track how many IPs fail due to capacity exceeded
    pub capacity_exceeded_ips: Arc<AtomicU64>,
    pub forecast_ticks: Arc<AtomicU64>,
    pub entropy_ticks: Arc<AtomicU64>,
    pub hw_rps_bits: Arc<AtomicU64>,
    pub hw_z_bits: Arc<AtomicU64>,
    pub hw_forecast_bits: Arc<AtomicU64>,
    pub entropy_bits: Arc<AtomicU64>,
    /// Number of out-of-order samples dropped because they arrived before the
    /// 1-second bucket boundary. Preserved for telemetry of detection accuracy.
    pub out_of_order_sample_dropped_total: Arc<AtomicU64>,
    /// Number of times a numerical calculation (CUSUM, variance, windup ceiling)
    /// produced a non-finite value and was rejected. Preserved for debugging.
    pub numerical_calculation_failure_total: Arc<AtomicU64>,
    /// Number of times an adaptive breach was triggered via the CUSUM detector.
    pub adaptive_breach_total: Arc<AtomicU64>,
    /// Number of IPs currently in Breached mode (frozen baseline). Gauge for
    /// operators monitoring active breach state.
    pub baseline_frozen_ips_gauge: Arc<AtomicU64>,
    /// P2: CGNAT graduated mitigation counters
    pub cgnat_classify_ticks: Arc<AtomicU64>,
    pub cgnat_tier_allow: Arc<AtomicU64>,
    pub cgnat_tier_challenge: Arc<AtomicU64>,
    pub cgnat_tier_powdrop: Arc<AtomicU64>,
    pub cgnat_tier_block: Arc<AtomicU64>,
    /// P2: SHM rule table publish counters
    pub shm_publish_count: Arc<AtomicU64>,
    pub shm_lookup_count: Arc<AtomicU64>,
    pub shm_cache_hits: Arc<AtomicU64>,
    /// P2: Analytics streaming counters
    pub hll_insert_count: Arc<AtomicU64>,
    pub cms_increment_count: Arc<AtomicU64>,
    pub cms_decay_ticks: Arc<AtomicU64>,
    /// XDP kernel dataplane counters (from AyaXdpApplier::counters())
    /// COUNTERS PerCpuArray: [v4_drop, v6_drop, wire_pass, parse_fail]
    pub xdp_v4_drops: Arc<AtomicU64>,
    pub xdp_v6_drops: Arc<AtomicU64>,
    pub xdp_attribution_gaps: Arc<AtomicU64>,
    pub xdp_blocked_ips_zero_drops: Arc<AtomicU64>,
    pub xdp_wire_pass: Arc<AtomicU64>,
    pub xdp_parse_fails: Arc<AtomicU64>,
    /// XDP apply attempts that failed. The block is held in userspace (and
    /// WAL) but NEVER reached the kernel, so the wire keeps passing the
    /// attacker while the engine believes the target is blocked. Only the
    /// CIDR LPM tries can exhaust (102_400 hard cap, no LRU support); the
    /// per-IP maps are LRU and self-evict, so ENOSPC there is impossible.
    /// Without this counter a full trie is a silent loss of the subnet-
    /// swarm mitigation leg — a warn! log and nothing scrapeable.
    pub xdp_apply_failures: Arc<AtomicU64>,
    /// Enforcement: last committed WAL LSN (atomic read for dashboard).
    pub wal_lsn: Arc<AtomicU64>,
    /// Enforcement: pending TTL expirations count.
    pub pending_expirations: Arc<AtomicU64>,
    /// P3: Mesh CRDT counters
    pub mesh_record_ban_count: Arc<AtomicU64>,
    pub mesh_record_unban_count: Arc<AtomicU64>,
    pub mesh_purge_ticks: Arc<AtomicU64>,
    pub mesh_hlc_ticks: Arc<AtomicU64>,
    pub last_batch_events: Arc<AtomicU64>,
    pub last_batch_promoted: Arc<AtomicU64>,
    pub last_batch_blocks: Arc<AtomicU64>,
    /// Dashboard login attempts that failed (wrong password, lockout).
    pub auth_login_failures_total: Arc<AtomicU64>,
    /// Dashboard login attempts that succeeded.
    pub auth_login_successes_total: Arc<AtomicU64>,
    /// Total IP blocks that expired via TTL or manual unblock (qual metric).
    pub blocks_expired_total: Arc<AtomicU64>,
    /// Total WAL segments pruned by retention policy (qual metric).
    pub wal_segments_pruned_total: Arc<AtomicU64>,
    /// Total XDP evictions (LRU map pressure — qual metric).
    pub xdp_evictions_total: Arc<AtomicU64>,
    /// Total dashboard login lockout events (qual metric).
    pub auth_lockout_total: Arc<AtomicU64>,
    /// Latest Argon2 verification wait duration in ms (qual metric; gauge).
    pub auth_verification_wait_ms: Arc<AtomicU64>,
    /// Dashboard API requests blocked by the cross-origin CSRF guard.
    pub csrf_blocked_total: Arc<AtomicU64>,
    /// Dashboard API requests allowed through the CSRF guard.
    pub csrf_allowed_total: Arc<AtomicU64>,
    /// Bloom advisory-cache observability (Patch A). The bloom is a revisit
    /// cache for *promoted* IPs, not a block list, so its fill must be
    /// measurable: `bloom_inserts_epoch` counts distinct promotes since the
    /// last 8s clear; `bloom_fp_ppm` is derived in render_prometheus from
    /// n and m (k=2). FP only ever OPENS the promote gate (over-promote),
    /// never rejects — so an over-full bloom is a tuning signal, not an
    /// outage.
    pub bloom_bits: Arc<AtomicU64>,
    pub bloom_inserts_epoch: Arc<AtomicU64>,
    pub bloom_clears_total: Arc<AtomicU64>,
    pub bloom_saturation_clears_total: Arc<AtomicU64>,
    /// Seconds since the last successful XDP reconciliation. 0 if never reconciled.
    pub reconcile_age_seconds: Arc<AtomicU64>,
    /// Metric gauge indicating whether the active XDP projection is stale relative to userspace.
    pub xdp_projection_stale: Arc<AtomicU64>,
    /// True while an active XDP dataplane requires a fresh userspace→kernel projection.
    pub xdp_projection_active: Arc<AtomicBool>,
    /// Unix timestamp of the last successful reconciliation.
    pub reconcile_last_success_unix: Arc<AtomicU64>,
    /// Total reconciliation failures.
    pub reconcile_failures_total: Arc<AtomicU64>,
    pub reconcile_successes_total: Arc<AtomicU64>,
    pub last_batch: Arc<Mutex<Option<Arc<BatchRecord>>>>,
    pub batch_history: Arc<Mutex<VecDeque<Arc<BatchRecord>>>>,
    pub block_log: Arc<Mutex<VecDeque<BlockRecord>>>,
    pub block_log_cap: usize,
    /// Write-path sequence for the JSON caches (RAM-for-CPU item 16): bumped
    /// under the same mutex the ring mutation holds, read by get_*_json.
    /// Plain atomics — Metrics itself always lives behind an Arc.
    block_seq: AtomicU64,
    batch_seq: AtomicU64,
    // Per-instance caches, not function statics: several Metrics objects
    // exist in tests/embedded use; a shared static leaks one instance's text.
    metrics_cache: Mutex<Option<(std::time::Instant, Arc<str>)>>,
    blocks_json_cache: Mutex<Option<(u64, Arc<str>)>>,
    batches_json_cache: Mutex<Option<(u64, Arc<str>)>>,
    started_ms: u64,
}

impl Metrics {
    pub fn new() -> Self {
        Self::with_block_log(1_000)
    }

    /// `block_log_size`: ring size served by `/api/history/blocks`.
    /// Was a hardcoded 40 — useless during floods. Config-driven now
    /// (`[dashboard] block_log_size`, default 1000).
    pub fn with_block_log(block_log_size: usize) -> Self {
        Self {
            requests_total: Arc::new(AtomicU64::new(0)),
            blocks_total: Arc::new(AtomicU64::new(0)),
            events_ingested: Arc::new(AtomicU64::new(0)),
            pre_aggs_flushes: Arc::new(AtomicU64::new(0)),
            events_rejected: Arc::new(AtomicU64::new(0)),
            ipc_event_drops: Arc::new(AtomicU64::new(0)),
            ipc_auth_rejections: Arc::new(AtomicU64::new(0)),
            ipc_authz_rejections: Arc::new(AtomicU64::new(0)),
            ipc_rejected_connections: Arc::new(AtomicU64::new(0)),
            frames_rejected: Arc::new(AtomicU64::new(0)),
            events_shed: Arc::new(AtomicU64::new(0)),
            native_ingest_drops: Arc::new(AtomicU64::new(0)),
            native_frames_walked: Arc::new(AtomicU64::new(0)),
            native_parse_rejects: Arc::new(AtomicU64::new(0)),
            native_tpacket_rejects: Arc::new(AtomicU64::new(0)),
            native_ingest_emitted: Arc::new(AtomicU64::new(0)),
            native_socket_packets: Arc::new(AtomicU64::new(0)),
            native_socket_drops: Arc::new(AtomicU64::new(0)),
            native_frames_declared: Arc::new(AtomicU64::new(0)),
            native_frames_validated: Arc::new(AtomicU64::new(0)),
            native_overlay_skips: Arc::new(AtomicU64::new(0)),
            native_rate_limit_skips: Arc::new(AtomicU64::new(0)),
            native_ingest_drops_disconnected: Arc::new(AtomicU64::new(0)),
            native_capture_workers_expected: Arc::new(AtomicU64::new(0)),
            native_capture_workers_started: Arc::new(AtomicU64::new(0)),
            native_capture_workers_running: Arc::new(AtomicU64::new(0)),
            native_capture_worker_failures: Arc::new(AtomicU64::new(0)),
            ingest_channel_depth: Arc::new(AtomicU64::new(0)),
            ingest_channel_depth_peak: Arc::new(AtomicU64::new(0)),
            active_cidr_blocks: Arc::new(AtomicU64::new(0)),
            batches_total: Arc::new(AtomicU64::new(0)),
            promotions_total: Arc::new(AtomicU64::new(0)),
            cold_skipped_total: Arc::new(AtomicU64::new(0)),
            blocks_detection: Arc::new(AtomicU64::new(0)),
            blocks_subnet: Arc::new(AtomicU64::new(0)),
            blocks_forecast: Arc::new(AtomicU64::new(0)),
            enforcement_dropped: Arc::new(AtomicU64::new(0)),
            capacity_exceeded_count: Arc::new(AtomicU64::new(0)),
            capacity_exceeded_ips: Arc::new(AtomicU64::new(0)),
            forecast_ticks: Arc::new(AtomicU64::new(0)),
            entropy_ticks: Arc::new(AtomicU64::new(0)),
            hw_rps_bits: Arc::new(AtomicU64::new(0)),
            hw_z_bits: Arc::new(AtomicU64::new(0)),
            hw_forecast_bits: Arc::new(AtomicU64::new(0)),
            entropy_bits: Arc::new(AtomicU64::new(0)),
            out_of_order_sample_dropped_total: Arc::new(AtomicU64::new(0)),
            numerical_calculation_failure_total: Arc::new(AtomicU64::new(0)),
            adaptive_breach_total: Arc::new(AtomicU64::new(0)),
            baseline_frozen_ips_gauge: Arc::new(AtomicU64::new(0)),
            cgnat_classify_ticks: Arc::new(AtomicU64::new(0)),
            cgnat_tier_allow: Arc::new(AtomicU64::new(0)),
            cgnat_tier_challenge: Arc::new(AtomicU64::new(0)),
            cgnat_tier_powdrop: Arc::new(AtomicU64::new(0)),
            cgnat_tier_block: Arc::new(AtomicU64::new(0)),
            shm_publish_count: Arc::new(AtomicU64::new(0)),
            shm_lookup_count: Arc::new(AtomicU64::new(0)),
            shm_cache_hits: Arc::new(AtomicU64::new(0)),
            hll_insert_count: Arc::new(AtomicU64::new(0)),
            cms_increment_count: Arc::new(AtomicU64::new(0)),
            cms_decay_ticks: Arc::new(AtomicU64::new(0)),
            xdp_v4_drops: Arc::new(AtomicU64::new(0)),
            xdp_v6_drops: Arc::new(AtomicU64::new(0)),
            xdp_attribution_gaps: Arc::new(AtomicU64::new(0)),
            xdp_blocked_ips_zero_drops: Arc::new(AtomicU64::new(0)),
            xdp_wire_pass: Arc::new(AtomicU64::new(0)),
            xdp_parse_fails: Arc::new(AtomicU64::new(0)),
            xdp_apply_failures: Arc::new(AtomicU64::new(0)),
            wal_lsn: Arc::new(AtomicU64::new(0)),
            pending_expirations: Arc::new(AtomicU64::new(0)),
            mesh_record_ban_count: Arc::new(AtomicU64::new(0)),
            mesh_record_unban_count: Arc::new(AtomicU64::new(0)),
            mesh_purge_ticks: Arc::new(AtomicU64::new(0)),
            mesh_hlc_ticks: Arc::new(AtomicU64::new(0)),
            last_batch_events: Arc::new(AtomicU64::new(0)),
            last_batch_promoted: Arc::new(AtomicU64::new(0)),
            last_batch_blocks: Arc::new(AtomicU64::new(0)),
            auth_login_failures_total: Arc::new(AtomicU64::new(0)),
            auth_login_successes_total: Arc::new(AtomicU64::new(0)),
            blocks_expired_total: Arc::new(AtomicU64::new(0)),
            wal_segments_pruned_total: Arc::new(AtomicU64::new(0)),
            xdp_evictions_total: Arc::new(AtomicU64::new(0)),
            auth_lockout_total: Arc::new(AtomicU64::new(0)),
            auth_verification_wait_ms: Arc::new(AtomicU64::new(0)),
            csrf_blocked_total: Arc::new(AtomicU64::new(0)),
            csrf_allowed_total: Arc::new(AtomicU64::new(0)),
            bloom_bits: Arc::new(AtomicU64::new(0)),
            bloom_inserts_epoch: Arc::new(AtomicU64::new(0)),
            bloom_clears_total: Arc::new(AtomicU64::new(0)),
            bloom_saturation_clears_total: Arc::new(AtomicU64::new(0)),
            reconcile_age_seconds: Arc::new(AtomicU64::new(0)),
            xdp_projection_stale: Arc::new(AtomicU64::new(0)),
            xdp_projection_active: Arc::new(AtomicBool::new(false)),
            reconcile_last_success_unix: Arc::new(AtomicU64::new(0)),
            reconcile_failures_total: Arc::new(AtomicU64::new(0)),
            reconcile_successes_total: Arc::new(AtomicU64::new(0)),
            last_batch: Arc::new(Mutex::new(None)),
            batch_history: Arc::new(Mutex::new(VecDeque::with_capacity(HISTORY))),
            block_log: Arc::new(Mutex::new(VecDeque::with_capacity(block_log_size.max(1)))),
            block_log_cap: block_log_size.max(1),
            block_seq: AtomicU64::new(0),
            batch_seq: AtomicU64::new(0),
            metrics_cache: Mutex::new(None),
            blocks_json_cache: Mutex::new(None),
            batches_json_cache: Mutex::new(None),
            started_ms: now_ms(),
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "tests/cache.rs"]
mod cache_tests;

#[cfg(test)]
mod tests;
