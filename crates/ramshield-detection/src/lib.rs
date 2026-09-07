//! Detection engine — batch-first, subnet-scale diagnosis.
//! Unified: engine pipeline from src (BloomFilter, pre-aggs, workers) +
//! crate batch.rs aggregation (IPv6 /64 keys, IpNetwork metadata).

pub mod batch;
pub mod rate_tracker;

use ahash::AHashMap as HashMap;
use arc_swap::ArcSwap;
use batch::{IpAgg, aggregate};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use dashmap::DashMap;
use ramshield_config::{ConfigHandle, DetectionConfig};
use ramshield_metrics::Metrics;
use ramshield_storage::{BlockState, IpRecord, Store, SubnetKey, Value, subnet_key_u128};
use ramshield_types::BlockReason;
use ramshield_types::{ConnectionEvent, EnforceAction, EnforceCommand, IpNetwork};
use rate_tracker::{
    CUSUM_WARMUP_SAMPLES, cusum_allowance, cusum_fired, cusum_step_capped, ewma, ewma_alpha_slow,
    is_exceeded, pulse_tracker_step,
};
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

// ── Bloom filter — 2-hash, no false negatives for inserted IPs ───────────────
#[derive(Clone)]
pub struct BloomFilter {
    bits: Vec<u64>,
    size: usize,
}

impl BloomFilter {
    pub fn new(bits: usize) -> Self {
        Self {
            bits: vec![0; bits.div_ceil(64)],
            size: bits,
        }
    }

    /// Wipe the filter. Required because the bloom is advisory and grows
    /// monotonically otherwise — every bit becomes set within hours of
    /// any real traffic, after which `contains_hashed` always returns
    /// true and the cold-skip short-circuit at line 365 stops skipping
    /// anything, ballooning the store to O(total_ips_ever_seen).
    pub fn clear(&mut self) {
        self.bits.fill(0);
    }

    pub fn slots(ip: &IpAddr) -> (usize, usize) {
        // ahash, not DefaultHasher (SipHash): slots() runs once per event at
        // ingest rate. Collision-storm resistance is not a property the bloom
        // needs (false positives are its whole design); speed is.
        let mut h = ahash::AHasher::default();
        ip.hash(&mut h);
        let x = h.finish();
        let a = x as usize;
        let b = (x.rotate_left(17) as usize).wrapping_mul(2_654_435_761);
        (a, b)
    }

    pub fn contains_hashed(&self, a: usize, b: usize) -> bool {
        let a = a % self.size;
        let b = b % self.size;
        (self.bits[a / 64] >> (a % 64)) & 1 == 1 && (self.bits[b / 64] >> (b % 64)) & 1 == 1
    }

    pub fn insert_hashed(&mut self, a: usize, b: usize) {
        let a = a % self.size;
        let b = b % self.size;
        self.bits[a / 64] |= 1u64 << (a % 64);
        self.bits[b / 64] |= 1u64 << (b % 64);
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let (a, b) = Self::slots(&ip);
        self.contains_hashed(a, b)
    }

    pub fn insert(&mut self, ip: IpAddr) {
        let (a, b) = Self::slots(&ip);
        self.insert_hashed(a, b);
    }
}

// ── Status-code → bucket table (600 B, L1-resident; kills per-event /100) ──────
/// ponytail: derive /24 counts from already-aggregated IpAggs instead of re-scanning
/// raw events. One pass, no second HashMap allocation.
/// Events + distinct member IPs per /24 (aggs are already per-IP-distinct).
fn subnet_counts_of(aggs: &[(IpAddr, IpAgg)]) -> HashMap<SubnetKey, (u32, Vec<IpAddr>)> {
    let mut subnets: HashMap<SubnetKey, (u32, Vec<IpAddr>)> =
        HashMap::with_capacity(aggs.len().min(512));
    for (ip, agg) in aggs {
        if let Some(sk) = subnet_key_u128(*ip) {
            let e = subnets.entry(sk).or_insert((0, Vec::new()));
            e.0 += agg.count;
            e.1.push(*ip);
        }
    }
    subnets
}

/// ponytail: status → bucket helper kept here because the const
/// table lives next to its only consumer. Single L1 lookup; replaces per-event /100.
pub(crate) const fn status_bucket(code: u16) -> u8 {
    if code >= 100 && code < 600 {
        (code / 100 - 1) as u8
    } else {
        255 // invalid
    }
}
// ponytail: const-eval'd table, upgrade path = none needed (600 B, one lookup).
#[rustfmt::skip]
const STATUS_BUCKET: [u8; 600] = {
    let mut t = [255u8; 600];
    let mut i = 0;
    while i < 600 {
        t[i] = status_bucket(i as u16);
        i += 1;
    }
    t
};

// ── Detection engine ─────────────────────────────────────────────────────────

/// Capacity of the ingest channel. IPC telemetry imports this — do not
/// duplicate the number elsewhere.
pub const CHANNEL_CAPACITY: u64 = 64_000;

/// Item 3: worker-local buffers merge into shared pre_aggs at least this
/// often even if neither time nor shared-size triggers fire — bounds the
/// per-worker head-of-line to ~8k uniques (~600KB) during an
/// extreme IP-fanout burst.
const LOCAL_MERGE_SOFT_CAP: usize = 8_192;

pub struct DetectionEngine {
    store: Arc<Store>,
    config: ConfigHandle,
    metrics: Arc<Metrics>,
    event_tx: Sender<ConnectionEvent>,
    event_rx: Arc<Receiver<ConnectionEvent>>,
    enforcement_tx: mpsc::Sender<EnforceCommand>,
    bloom: ArcSwap<BloomFilter>,
    shutdown: Arc<AtomicBool>,
    /// Pre-aggregation buffer — DashMap is internally thread-safe, no Arc needed.
    /// ahash (item 1): every event hashes its IP here under attacker volume.
    pre_aggs: DashMap<IpAddr, IpAgg, ahash::RandomState>,
    last_pre_aggs_flush_ns: AtomicU64,
    /// F1: single-flusher gate (N batch workers share the flush trigger).
    flushing: AtomicBool,
    /// F9: batch/subnet threads, joined at shutdown (was: detached + blind
    /// 5s sleep in main). Each does a final flush before exit.
    worker_handles: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>>,
}

/// Releases the single-flusher gate even on early return/panic.
struct FlushGuard<'a>(&'a AtomicBool);
impl Drop for FlushGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl DetectionEngine {
    pub fn new(
        store: Arc<Store>,
        config: ConfigHandle,
        enforcement_tx: mpsc::Sender<EnforceCommand>,
        metrics: Arc<Metrics>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        let bloom_bits = config.load().detection.bloom_bits;
        // 64k cap ≈ 4MB RSS, fills in ~64ms at 1M eps — keeps the batch
        // processor honest without megabytes of dead head-of-line buffer.
        // Exported so IPC telemetry can't drift from the real size (F3).
        // ponytail: hardcoded; lift to Config.detection.batch_channel_capacity
        // when traffic profiles diverge.
        let (tx, rx) = bounded::<ConnectionEvent>(CHANNEL_CAPACITY as usize);
        // pre_aggs writers = batch workers (≤ cores), so 64 shards removes
        // every realistic cross-thread collision. The old derivation
        // (bloom_bits/1024) sized the shard array off an UNRELATED
        // structure: prod 1M-bit bloom -> 1024 shards, stress 8M -> 8192 —
        // every shard lookup chased a huge array of cache lines it could
        // never reuse (TLB tax per event). Shards should track worker
        // count, not bloom size. ponytail: revisit if workers ever > 64.
        Self {
            store,
            config,
            metrics,
            event_tx: tx,
            event_rx: Arc::new(rx),
            enforcement_tx,
            bloom: ArcSwap::from_pointee(BloomFilter::new(bloom_bits)),
            shutdown,
            pre_aggs: DashMap::with_hasher_and_shard_amount(ahash::RandomState::new(), 64),
            last_pre_aggs_flush_ns: AtomicU64::new(now_ns()),
            flushing: AtomicBool::new(false),
            worker_handles: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn event_sender(&self) -> Sender<ConnectionEvent> {
        self.event_tx.clone()
    }

    /// Move a worker's local buffer into shared `pre_aggs` (RAM-for-CPU
    /// item 3). Runs once per flush boundary, not per event. Same-IP
    /// consolidation across workers uses IpAgg::merge_with, so the batch
    /// that reaches flush_batch keeps the exact old cross-worker semantics
    /// (one entry per IP per flush window, summed counts).
    fn merge_local(&self, local: &mut HashMap<IpAddr, IpAgg>) {
        if local.is_empty() {
            return;
        }
        for (ip, agg) in local.drain() {
            self.pre_aggs
                .entry(ip)
                .and_modify(|cur| cur.merge_with(&agg))
                .or_insert(agg);
        }
    }

    /// Test/diagnostic entry: absorb straight into the shared map (what the
    /// pre-item-3 stream path did). Production workers absorb into a local
    /// buffer and merge via `merge_local`.
    #[cfg(test)]
    fn absorb_shared(&self, ev: ConnectionEvent) {
        self.pre_aggs
            .entry(ev.ip)
            .and_modify(|a| a.absorb(&ev))
            .or_insert_with(|| {
                let mut a = IpAgg::default();
                a.absorb(&ev);
                a
            });
    }

    fn pre_aggs_needs_flush_due_to_timeout(&self, interval_ms: u64) -> bool {
        let last_flush = self.last_pre_aggs_flush_ns.load(Ordering::Relaxed);
        now_ns().saturating_sub(last_flush) >= interval_ms * 1_000_000
    }

    fn flush_pre_aggs_to_store(&self) {
        // P1 fix (F1 race + F7 rate): N workers can hit the flush trigger in
        // the same tick. Old path: iter_mut + mem::take + clear() — a worker
        // inserting during another's walk had its fresh event erased by the
        // clear(), silently dropping up to walk_duration x rate events
        // (~2K/flush at 1M eps) and pushing zero-ghost uniques. Now:
        // (a) CAS gate — one flusher at a time, others skip (they'll retry
        //     next loop iteration); (b) pop() drain — each entry removed
        //     under its own shard lock, racy inserts survive to the next
        //     flush instead of being cleared; (c) the real elapsed window is
        //     passed to flush_batch so events_last_second is a true rate
        //     even when prod flushes every 100ms (old code stored raw
        //     per-flush counts, lieing 10x to the forecaster).
        if self
            .flushing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        let _flush_guard = FlushGuard(&self.flushing);

        let now = now_ns();
        let prev = self.last_pre_aggs_flush_ns.swap(now, Ordering::Relaxed);
        let window_ns = now.saturating_sub(prev).max(1);

        if self.pre_aggs.is_empty() {
            return;
        }

        // (DashMap 6 has no pop() — reviewer snippet was aspirational.
        // Collect keys, then per-key remove(): each remove is atomic under the
        // shard lock and returns the CURRENT value, so events racing in between
        // are either included here or survive as a fresh entry for next flush.)
        let keys: Vec<IpAddr> = self.pre_aggs.iter().map(|e| *e.key()).collect();
        let mut aggs: Vec<(IpAddr, IpAgg)> = Vec::with_capacity(keys.len());
        for k in keys {
            if let Some((ip, agg)) = self.pre_aggs.remove(&k) {
                aggs.push((ip, agg));
            }
        }

        let total_events: u64 = aggs.iter().map(|a| a.1.count as u64).sum();
        self.metrics.inc_ingested(total_events);

        let subnet_counts = subnet_counts_of(&aggs);
        self.flush_batch(
            &aggs,
            &subnet_counts,
            &HashMap::new(),
            total_events,
            window_ns,
        );
    }

    /// Spawns `n` batch-processor threads (default: CPU cores) consuming from the
    /// shared event channel, plus one subnet-analysis thread.  Each batch thread
    /// writes to the same `pre_aggs` DashMap — sharded internally so concurrent
    /// writers on different IPs don't block each other.
    ///
    /// ponytail: if `n == 0`, fall back to available_parallelism. Add a config knob
    /// when worker_threads tuning becomes a real SLO target.
    pub fn spawn_workers(self: Arc<Self>, n: usize) {
        let det = self.config.load().detection.clone();
        let n_workers = if n == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        } else {
            n
        };
        info!(
            "Detection: spawning {} batch processors (max {} events / {} ms window), 1 subnet loop",
            n_workers, det.batch_max_events, det.batch_window_ms
        );

        // crossbeam Receiver inside Arc — clone Arc for each worker (cheap refcount bump).
        // Each worker drains aggressively with try_recv() inside a recv_timeout window.
        let mut handles = self.worker_handles.lock().unwrap();
        for i in 0..n_workers {
            let eng = self.clone();
            let rx = self.event_rx.clone();
            handles.push(
                std::thread::Builder::new()
                    .name(format!("rs-batch-{i}"))
                    .spawn(move || eng.batch_processor_loop_from(rx))
                    .expect("spawn batch processor"),
            );
        }

        let eng = self.clone();
        handles.push(
            std::thread::Builder::new()
                .name("rs-subnet".into())
                .spawn(move || eng.subnet_batch_loop())
                .expect("spawn subnet batch loop"),
        );
        drop(handles);
    }

    /// F9: block until batch/subnet threads exit (each final-flushes on the
    /// way out). Returns after `grace` elapses at worst.
    pub fn join_workers(&self, grace: std::time::Duration) {
        let handles: Vec<_> = self.worker_handles.lock().unwrap().drain(..).collect();
        // Workers exit within recv_timeout (<= batch_window_ms) of the flag
        // + one final flush; poll-until-finished gives the grace cap without
        // inventing a join_timeout (std has none). Last-resort join() is safe
        // because every worker path ends in break on the shutdown flag.
        let deadline = std::time::Instant::now() + grace;
        loop {
            if handles.iter().all(|h| h.is_finished()) {
                break;
            }
            if std::time::Instant::now() >= deadline {
                warn!("join_workers: grace expired with workers still running");
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        for h in handles {
            if h.is_finished() {
                let _ = h.join();
            }
        }
    }

    /// Core batch loop — takes an explicit Receiver so N workers can share the
    /// same crossbeam channel (Receiver is Clone).
    ///
    /// RAM-for-CPU item 3: events are absorbed into a worker-LOCAL
    /// open-addressed map — zero shared locks on the per-event path (the old
    /// shared DashMap took a shard write-lock per event, ping-ponging cache
    /// lines between workers). The buffer merges into `pre_aggs` only at the
    /// flush boundary (interval grain), and the flush itself is the F1
    /// single-flusher CAS path, so cross-worker counts keep the old
    /// semantics: one entry per IP per flush window.
    /// ponytail: memory guard stays on shared pre_aggs.len() only; local
    /// buffers add ≤ one flush interval of uniques per worker (~100ms of
    /// fanout). Revisit if pre_aggs_max_size tuning becomes an SLO.
    fn batch_processor_loop_from(&self, rx: Arc<Receiver<ConnectionEvent>>) {
        let mut local: HashMap<IpAddr, IpAgg> = HashMap::new();
        loop {
            if self.shutdown.load(Ordering::Acquire) {
                // P2 fix (F9): events sitting in pre_aggs at exit (up to one
                // flush interval — 100ms in prod) were never promoted to the
                // store; WAL persists blocks, not counts. Final flush on the
                // way out; the single-flusher CAS gate makes N concurrent
                // exit-flushes safe (one drains, others no-op).
                self.merge_local(&mut local);
                self.flush_pre_aggs_to_store();
                info!("Batch processor shutting down");
                break;
            }

            // Load config once per iteration (interval is fixed for process lifetime).
            let cfg = self.config.load();
            let window = Duration::from_millis(cfg.detection.batch_window_ms);
            let max = cfg.detection.batch_max_events;

            // Drain events into the worker-local buffer — no shared lock.
            match rx.recv_timeout(window) {
                Ok(ev) => local.entry(ev.ip).or_default().absorb(&ev),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    // Senders all gone: deliver what we hold, then exit.
                    self.merge_local(&mut local);
                    self.flush_pre_aggs_to_store();
                    break;
                }
            }

            // Drain remaining events up to batch_max_events
            for _ in 0..max.saturating_sub(1) {
                match rx.try_recv() {
                    Ok(ev) => local.entry(ev.ip).or_default().absorb(&ev),
                    Err(_) => break,
                }
            }

            // Flush pre_aggs to main store when size or timeout threshold hit
            if self.pre_aggs.len() >= cfg.detection.pre_aggs_max_size
                || local.len() >= LOCAL_MERGE_SOFT_CAP
                || self
                    .pre_aggs_needs_flush_due_to_timeout(cfg.detection.pre_aggs_flush_interval_ms)
            {
                self.merge_local(&mut local);
                self.flush_pre_aggs_to_store();
            }
        }
    }

    /// Test/IPC entry: aggregate raw events, then flush.
    pub fn flush_events(&self, events: &[ConnectionEvent]) {
        let a = aggregate(events);
        let aggs: Vec<(IpAddr, IpAgg)> = a.ips.into_iter().collect();
        // Synthetic batch: treat as a 1s window (caller-side tests assert counts, not rates).
        self.flush_batch(
            &aggs,
            &a.subnets,
            &a.networks,
            events.len() as u64,
            1_000_000_000,
        );
    }

    /// Single pass over aggregates: promote, merge, emit blocks. No store access for cold IPs.
    fn flush_batch(
        &self,
        ip_aggs: &[(IpAddr, IpAgg)],
        subnet_counts: &HashMap<SubnetKey, (u32, Vec<IpAddr>)>,
        networks: &HashMap<SubnetKey, IpNetwork>,
        total_events: u64,
        // F7: wall-clock ns span these events were collected over.
        window_ns: u64,
    ) {
        let cfg = self.config.load();
        let det = &cfg.detection;
        let ram_lim = cfg.engine.ram_limit_mb * 1024 * 1024;
        let now = now_ns();

        // Incremental counters for forecasting (no full-store scan).
        let subnet_vals: Vec<u64> = subnet_counts.values().map(|&(ev, _)| ev as u64).collect();
        // F7: events_last_second must be a RATE. Prod flushes every 100ms;
        // storing the raw per-flush count lied 10x low to the forecaster.
        let rate = total_events.saturating_mul(1_000_000_000) / window_ns;
        self.store
            .traffic
            .record_flush(rate, ip_aggs.len() as u64, &subnet_vals);

        for (&sk, &(count, ref members)) in subnet_counts.iter() {
            let net = networks.get(&sk).copied().unwrap_or_else(|| {
                // pre-agg path passes no networks map — reconstruct the /24
                // (v4) or /64 (v6) network from the subnet key itself.
                // ponytail: v6 keys carry the full /64 in low 64 bits; a
                // from_key constructor on IpNetwork would avoid this branch.
                if sk <= 0xFFFF_FFFF {
                    let o = [
                        (sk >> 24) as u8,
                        (sk >> 16) as u8,
                        (sk >> 8) as u8,
                        sk as u8,
                    ];
                    IpNetwork::ipv4_subnet(std::net::Ipv4Addr::from(o))
                } else {
                    IpNetwork::ipv6_subnet(std::net::Ipv6Addr::from(sk))
                }
            });
            self.store
                .merge_subnet_window(sk, net, count, Some(members), now);
        }

        let mut blocks = Vec::new();
        let mut threat_sample = Vec::with_capacity(64);
        let mut promoted = 0u32;
        let mut cold_skipped = 0u32;
        let mut promoted_events = 0u32;
        let mut cold_skipped_events = 0u32;

        let unique_ips = ip_aggs.len();
        let hot_subnets = subnet_counts.len();

        for &(ip, ref agg) in ip_aggs {
            let sk = subnet_key_u128(ip);
            let subnet_hot = sk
                .and_then(|k| subnet_counts.get(&k))
                .is_some_and(|&(ev, _)| ev as u64 >= det.subnet_window_threshold);

            let (a, b) = BloomFilter::slots(&ip);
            // ponytail: poison-recovery — bloom is advisory (false-positive
            // cache); a panic mid-hold leaves valid data, so recover instead
            // of panicking every future request. Upgrade: parking_lot.
            // ponytail: ArcSwap::load is lock-free, no poison risk.
            let bloom_hit = self.bloom.load().contains_hashed(a, b);

            if agg.count < det.promote_min_events && !subnet_hot && !bloom_hit {
                cold_skipped += 1;
                cold_skipped_events += agg.count;
                continue;
            }

            // ponytail: merge_record does the single store lookup (is_blocked check
            // was a second DashMap hit on the same key).
            let (_ewma_rps, threat, should_block, _was_blocked) =
                self.merge_record(ip, agg, det, ram_lim, now, sk);
            // Note: we do NOT skip already-blocked IPs here. The pulse-wave
            // tracker needs to keep running on every batch to count distinct
            // over-threshold samples; subsequent bursts must still record
            // their state. The enforcement layer deduplicates block commands
            // by (ip, reason), so duplicate emits just refresh the TTL.

            promoted += 1;
            promoted_events += agg.count;

            if threat > 0.5 {
                threat_sample.push((ip, threat));
            }

            if should_block {
                // ponytail: debounce removed single-sample is_exceeded bypass
                blocks.push((ip, BlockReason::HighRps, det.block_ttl_secs));
            }
        }

        // Batch bloom insert — ArcSwap clone+insert+store.
        if !blocks.is_empty() {
            let mut bf = (*self.bloom.load_full()).clone();
            for &(ip, _, _) in &blocks {
                let (a, b) = BloomFilter::slots(&ip);
                bf.insert_hashed(a, b);
            }
            self.bloom.store(Arc::new(bf));
        }
        threat_sample.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        threat_sample.truncate(128);
        self.store.traffic.push_threat_samples(threat_sample);
        self.store
            .traffic
            .promoted_ips
            .store(promoted as u64, Ordering::Relaxed);

        let block_count = blocks.len() as u32;
        for b in &blocks {
            self.metrics
                .record_block_ip(&b.0, b.1.as_str(), "detection");
        }
        // ponytail: warn once per 1024 rejections — log churn kills throughput
        // under sustained queue pressure. Upgrade: sliding-window rate limiter
        // if ops needs exact rejection counts (metric already tracks blocks).
        let mut rejected = 0u32;
        for b in blocks {
            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 1,
                source: "detection".into(),
                actor: "system".into(),
                timestamp_utc: (now / 1_000_000_000) as i64,
                ttl_seconds: b.2,
                reason: b.1.as_str().into(),
                ip: b.0,
                action: EnforceAction::Block,
            };
            if self.enforcement_tx.try_send(cmd).is_err() {
                rejected += 1;
                if rejected & 0x3FF == 1 {
                    warn!(ip=%b.0, rejected, "enforcement queue full; dropping {} block commands (sampled warn)", rejected);
                }
            }
        }

        self.metrics.record_batch(ramshield_metrics::BatchRecord {
            ts_ms: now / 1_000_000,
            events: total_events as u32,
            unique_ips: unique_ips as u32,
            promoted,
            cold_skipped,
            promoted_events,
            cold_skipped_events,
            blocks: block_count,
            hot_subnets: hot_subnets as u32,
        });

        debug!(
            "batch flush: {} events, {} unique IPs, {} hot subnets",
            total_events, unique_ips, hot_subnets,
        );
    }

    /// ponytail: returns the 4-tuple `(ewma_rps, threat, should_block, was_blocked)`.
    /// `was_blocked` is set true if the IP already had a BlockState, so callers can
    /// skip the extra store.get() they used to do.
    /// `sk`: pre-computed subnet key (avoids redundant `subnet_key_u128` call).
    fn merge_record(
        &self,
        ip: IpAddr,
        agg: &IpAgg,
        det: &DetectionConfig,
        ram_lim: usize,
        now: u64,
        sk: Option<SubnetKey>,
    ) -> (f64, f32, bool, bool) {
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
        let ((was_blocked, (ewma_rps, threat, block)), _stored) = self.store.update_ip(
            ip,
            IpRecord {
                ip,
                request_count: 0,
                ewma_rps: 0.0,
                cusum_s: 0.0,
                baseline_rps: 0.0,
                prev_sample_hot: false,
                sample_count: 0,
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

                // P1: CUSUM companion (Page 1954) — catches sustained sub-threshold
                // drift that absolute-EWMA can't see by construction.
                let baseline = if rec.baseline_rps == 0.0 {
                    rec.baseline_rps = rec.ewma_rps;
                    rec.ewma_rps
                } else {
                    rec.baseline_rps = ewma_alpha_slow() * rec.ewma_rps
                        + (1.0 - ewma_alpha_slow()) * rec.baseline_rps;
                    rec.baseline_rps
                };
                rec.sample_count = rec.sample_count.saturating_add(1);
                if rec.sample_count >= CUSUM_WARMUP_SAMPLES {
                    let k = cusum_allowance(det_thr);
                    rec.cusum_s =
                        cusum_step_capped(rec.cusum_s, inst_rps, baseline + k, det_thr as f64);
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
                let block = hot || cusum_fired(rec.cusum_s, det_thr) || pulse_fired;
                (was_blocked, (ewma_rps, threat, block))
            },
        );
        self.store.update_subnet_index(ip, sk, false);
        // block emitted even when already blocked: caller relies on the
        // enforcement dedup to refresh TTL (semantics preserved from pre-fix).
        (ewma_rps, threat, block, was_blocked)
    }

    /// Subnet-scale batch block — reads subnet_table only, not full store key scan.
    fn subnet_batch_loop(self: Arc<Self>) {
        let tick = std::time::Duration::from_millis(500);
        // Bloom clear cadence: 8s. Advisory cache resets faster than the slowest
        // legitimate IP's revisit window, so cold-skip stays effective.
        // Without clear, every bit becomes set within hours and cold-skip dies.
        let bloom_clear_ns: u64 = 8 * 1_000_000_000;
        let mut last_bloom_clear_ns = now_ns();
        // P1-6: periodic eviction of expired entries. Without this sweep,
        // expired entries (~136 B each) stay in DashMap indefinitely, counted
        // in ram_bytes, causing CapacityExceeded despite actual working-set
        // being well under the limit.
        let evict_interval_ns: u64 = 60 * 1_000_000_000;
        let mut last_evict_ns = now_ns();
        loop {
            if self.shutdown.load(Ordering::Acquire) {
                info!("Subnet batch loop shutting down");
                break;
            }
            std::thread::sleep(tick);
            if now_ns().saturating_sub(last_bloom_clear_ns) >= bloom_clear_ns {
                // ponytail: atomic swap — no lock held during clear.
                // Old bloom is reclaimed when last reader releases its guard.
                self.bloom.store(Arc::new(BloomFilter::new(
                    self.config.load().detection.bloom_bits,
                )));
                last_bloom_clear_ns = now_ns();
            }
            // P1-6: sweep expired entries every 60s.
            if now_ns().saturating_sub(last_evict_ns) >= evict_interval_ns {
                let evicted = self.store.evict_expired();
                if evicted > 0 {
                    debug!("evict_expired: removed {} expired entries", evicted);
                }
                last_evict_ns = now_ns();
            }
            // P1-10 fix: prune subnet_table when > 100K entries. This must
            // run INSIDE the loop — the original placement after `break`
            // made it a shutdown-only statement, so the table grew without
            // bound under spoofed-subnet floods until the host OOMed. Runs
            // before the batch_block_enabled `continue` so pruning cannot
            // be skipped when batch blocking is off. DashMap::len() is a
            // per-shard sum — cheap enough for the 500ms tick.
            let st = self.store.subnet_table();
            if st.len() > 100_000 {
                // P2 fix (F6): the old predicate (total_rps==0 &&
                // unique_ips()==0) is dead against the very attack that
                // motivated the prune — a spoofed-subnet flood sends one
                // burst per /24 then goes silent; total_rps only zeroes on a
                // NEW merge rollover >2s later, so flooded entries never
                // match and the table grows toward 16.7M /24s while every
                // 500ms full-iter (prune + hot()) slows down. Evict on
                // staleness instead: silent >8s = 4 gate-windows = functionally
                // zero for the 2s dual gate, so a live swarm can't be pruned.
                let now = now_ns();
                let stale_ns = 8_000_000_000;
                let mut candidates: Vec<_> = st
                    .iter()
                    .filter_map(|e| {
                        let r = e.value();
                        if now.saturating_sub(r.last_updated_ns) > stale_ns {
                            Some(*e.key())
                        } else {
                            None
                        }
                    })
                    .collect();
                candidates.truncate(st.len().saturating_sub(80_000)); // evict down to 80K
                for key in candidates {
                    st.remove(&key);
                }
            }
            self.subnet_batch_scan();
        }
    }

    /// One pass of the subnet dual gate + batch-block emit. Split out of
    /// `subnet_batch_loop` so tests can fire the scan deterministically.
    fn subnet_batch_scan(&self) {
        let cfg = self.config.load();
        if !cfg.detection.batch_block_enabled {
            return;
        }
        // Dual gate: unique-IP swarm signal AND raw event volume. Either
        // alone mis-fires (single flood IP trips volume; slow drip from
        // many IPs trips uniqueness).
        let ip_threshold = cfg.detection.subnet_batch_threshold as u64;
        let ev_threshold = cfg.detection.subnet_batch_min_events;

        let hot: Vec<(SubnetKey, u64, u64, String)> = self
            .store
            .subnet_table()
            .iter()
            .filter_map(|e| {
                let r = e.value();
                // IPv6 plan Task 2 (G1): v4 uniques come from the 256-bit
                // host bitmap (free inside the iter shard lock); a v6 /64
                // has no bitmap — exact count is subnet_index cardinality
                // (separate DashMap, no lock nesting).
                let uniq = if r.network.family() == 4 {
                    r.unique_ips()
                } else {
                    // Windowed: the raw index counts LIFETIME members (never
                    // pruned on unblock) — a cooled /64 with 60 historical
                    // hosts plus a small fresh burst would pass the gate and
                    // batch-block ~55 innocent IPs (review P1-2).
                    let now = now_ns();
                    self.store
                        .subnet_member_count_windowed(*e.key(), 2 * 1_000_000_000, now)
                };
                if uniq >= ip_threshold && r.total_rps >= ev_threshold {
                    Some((*e.key(), uniq, r.total_rps, r.network.to_string()))
                } else {
                    None
                }
            })
            .collect();

        for (sk, uniq, count, cidr) in hot {
            warn!(
                "Batch block subnet {} ({} IPs / {} events in window)",
                cidr, uniq, count
            );
            info!("Batch blocking subnet key {:#x}", sk);

            // O(1) lookup for IPs in the hot subnet instead of full scan
            let now = now_ns();
            let ips_in_subnet = self
                .store
                .get_ips_in_subnet_windowed(sk, 2 * 1_000_000_000, now);
            for key in ips_in_subnet {
                if let Some(e) = self.store.inner().get(&key)
                    && let Value::IpRecord(ref r) = e.value().value
                {
                    if matches!(r.block_state, BlockState::Blocked { .. }) {
                        continue;
                    }

                    let cmd = EnforceCommand {
                        decision_id: Uuid::new_v4(),
                        policy_version: 1,
                        source: "detection".into(),
                        actor: "system".into(),
                        timestamp_utc: (now_ns() / 1_000_000_000) as i64,
                        // Subnet blocks cover up to 253 hosts of shared
                        // egress — short TTL, re-fires on continued abuse.
                        ttl_seconds: cfg.detection.subnet_burst_ttl_secs,
                        reason: "subnet_burst".into(),
                        ip: r.ip,
                        action: EnforceAction::Block,
                    };
                    if self.enforcement_tx.try_send(cmd).is_err() {
                        warn!(ip=%r.ip, "enforcement queue full; subnet block rejected");
                    }
                    self.metrics
                        .record_block_ip(&r.ip, "subnet_batch", "detection");
                    self.metrics.blocks_subnet.fetch_add(1, Ordering::Relaxed);
                }
            }
            self.store.reset_subnet_window(sk);
        }
    }
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use ramshield_config::Config;
    use std::net::Ipv4Addr;

    fn engine() -> Arc<DetectionEngine> {
        let cfg = Config::default().into_handle();
        let store = Arc::new(Store::new(16));
        let metrics = Arc::new(Metrics::new());
        let (etx, _erx) = mpsc::channel(64);
        let shutdown = Arc::new(AtomicBool::new(false));
        Arc::new(DetectionEngine::new(store, cfg, etx, metrics, shutdown))
    }

    /// Item 3 regression: worker-local merge must equal the old shared-map
    /// semantics — same-IP counts/statuses summed, window timestamps min/max,
    /// local buffer emptied into shared.
    #[test]
    fn local_merge_preserves_cross_worker_semantics() {
        let eng = engine();
        let mut a: HashMap<IpAddr, IpAgg> = HashMap::new();
        let mut b: HashMap<IpAddr, IpAgg> = HashMap::new();
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        a.entry(ip).or_default().absorb(&ConnectionEvent {
            ip,
            timestamp_ns: 500,
            bytes: 10,
            status_code: 404,
            proto_fingerprint: 7,
        });
        b.entry(ip).or_default().absorb(&ConnectionEvent {
            ip,
            timestamp_ns: 200, // earlier than a's first
            bytes: 15,
            status_code: 200,
            proto_fingerprint: 0,
        });
        eng.merge_local(&mut a);
        eng.merge_local(&mut b);
        let agg = eng.pre_aggs.get(&ip).unwrap();
        assert_eq!(agg.count, 2);
        assert_eq!(agg.bytes, 25);
        assert_eq!(agg.first_ts_ns, 200, "window min across workers");
        assert_eq!(agg.last_ts_ns, 500, "window max across workers");
        assert_eq!(agg.status_dist[3], 1, "4xx from worker a");
        assert_eq!(agg.status_dist[1], 1, "2xx from worker b");
        assert_eq!(agg.proto_fp, 7, "first non-zero fingerprint wins");
        assert!(a.is_empty(), "local buffer is drained into shared");
    }

    /// P1 regression (F1): with N workers, the old iter_mut+take+clear flush
    /// silently erased events inserted during the walk. Invariant:
    /// ingested + left-in-pre_aggs == sent, exactly, under concurrent flush.
    #[test]
    fn concurrent_flush_never_loses_events() {
        let cfg = Config::default().into_handle();
        let store = Arc::new(Store::new(16));
        let metrics = Arc::new(Metrics::new());
        let (etx, _erx) = mpsc::channel(64);
        let eng = Arc::new(DetectionEngine::new(
            store,
            cfg,
            etx,
            metrics.clone(),
            Arc::new(AtomicBool::new(false)),
        ));

        const SENDERS: u64 = 8;
        const PER_SENDER: u64 = 5_000;
        let stop = Arc::new(AtomicBool::new(false));

        let flushers: Vec<_> = (0..4)
            .map(|_| {
                let e = eng.clone();
                let s = stop.clone();
                std::thread::spawn(move || {
                    while !s.load(Ordering::Relaxed) {
                        e.flush_pre_aggs_to_store();
                        std::thread::sleep(std::time::Duration::from_micros(200));
                    }
                })
            })
            .collect();

        let feeds: Vec<_> = (0..SENDERS)
            .map(|w| {
                let e = eng.clone();
                std::thread::spawn(move || {
                    for i in 0..PER_SENDER {
                        let ip: IpAddr = format!("10.{}.0.{}", w, i % 251).parse().unwrap();
                        e.absorb_shared(ConnectionEvent {
                            ip,
                            timestamp_ns: i * 1_000_000,
                            bytes: 100,
                            status_code: 200,
                            proto_fingerprint: 0,
                        });
                    }
                })
            })
            .collect();
        for f in feeds {
            f.join().unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        for f in flushers {
            f.join().unwrap();
        }
        // Final drain (single-flusher now uncontended).
        while !eng.pre_aggs.is_empty() {
            eng.flush_pre_aggs_to_store();
        }

        let sent = SENDERS * PER_SENDER;
        let ingested = metrics.events_ingested.load(Ordering::Relaxed);
        let left: u64 = eng.pre_aggs.iter().map(|a| a.value().count as u64).sum();
        assert_eq!(
            ingested + left,
            sent,
            "F1 loss: ingested={ingested} left={left} sent={sent}"
        );
    }

    #[test]
    fn flush_promotes_hot_ip() {
        let eng = engine();
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let events: Vec<_> = (0..20)
            .map(|i| ConnectionEvent {
                ip,
                timestamp_ns: i,
                bytes: 64,
                status_code: 200,
                proto_fingerprint: 0,
            })
            .collect();
        eng.flush_events(&events);
        assert!(eng.store.get(&ip).is_some());
    }

    #[test]
    fn cold_ip_not_stored() {
        let eng = engine();
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        eng.flush_events(&[ConnectionEvent {
            ip,
            timestamp_ns: 1,
            bytes: 1,
            status_code: 200,
            proto_fingerprint: 0,
        }]);
        assert!(eng.store.get(&ip).is_none());
    }

    #[test]
    fn flush_preserves_status_dist() {
        // The old reconstruct-events path zeroed status_code, so 5xx never
        // reached threat scoring. One assert that the real distribution survives.
        let eng = engine();
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 9, 9));
        let events: Vec<_> = (0..20)
            .map(|i| ConnectionEvent {
                ip,
                timestamp_ns: i,
                bytes: 64,
                status_code: 500,
                proto_fingerprint: 0,
            })
            .collect();
        eng.flush_events(&events);
        match eng.store.get(&ip) {
            Some(Value::IpRecord(r)) => assert!(
                r.status_dist[4] >= 20,
                "5xx bucket lost: {:?}",
                r.status_dist
            ),
            other => panic!("expected IpRecord, got {other:?}"),
        }
    }

    #[test]
    fn v6_events_aggregate_and_promote() {
        let eng = engine();
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        let events: Vec<_> = (0..20)
            .map(|i| ConnectionEvent {
                ip,
                timestamp_ns: i,
                bytes: 64,
                status_code: 200,
                proto_fingerprint: 0,
            })
            .collect();
        eng.flush_events(&events);
        assert!(eng.store.get(&ip).is_some());
        // v6 /64 landed in subnet table
        let sk = subnet_key_u128(ip).unwrap();
        assert!(eng.store.subnet_table().contains_key(&sk));
    }

    /// IPv6 plan Task 2 (G1): the dual gate must fire for a v6 /64 swarm.
    /// v4 counts uniques via the 256-bit host bitmap; a /64 has no bitmap —
    /// exactness lives in `subnet_index` cardinality (plan D1).
    #[test]
    fn v6_subnet_swarm_blocks_via_index_cardinality() {
        use tokio::sync::mpsc;
        let mut cfg = Config::default();
        // Default window threshold (500 ev) exceeds this synthetic swarm;
        // without promotion no host reaches the store, so the reverse index
        // the gate reads stays empty. Same knob v4 tests tune implicitly.
        cfg.detection.subnet_window_threshold = 1;
        let cfg = cfg.into_handle();
        let store = Arc::new(Store::new(16));
        let metrics = Arc::new(Metrics::new());
        let (etx, mut erx) = mpsc::channel(64);
        let eng = Arc::new(DetectionEngine::new(
            store,
            cfg,
            etx,
            metrics,
            Arc::new(AtomicBool::new(false)),
        ));
        // 60 distinct hosts in 2001:db8:abcd::/64, 3 events each =
        // 60 uniq / 180 events >= (50, 100) dual gate.
        let hosts: Vec<IpAddr> = (1..=60u16)
            .map(|o| {
                IpAddr::V6(std::net::Ipv6Addr::new(
                    0x2001, 0xdb8, 0xabcd, 0, 0, 0, 0, o,
                ))
            })
            .collect();
        let events: Vec<_> = hosts
            .iter()
            .flat_map(|ip| {
                let base = now_ns();
                (0..3u64).map(move |i| ev_at(*ip, base + i))
            })
            .collect();
        eng.flush_events(&events);
        // Drain the flush's per-IP blocks (inst_rps is huge in synthetic
        // tests) so what remains is only the scan's subnet_burst output.
        while erx.try_recv().is_ok() {}
        eng.subnet_batch_scan();
        let cmds: Vec<_> = {
            let mut v = Vec::new();
            while let Ok(c) = erx.try_recv() {
                v.push(c);
            }
            v
        };
        let v6_blocks: Vec<_> = cmds
            .iter()
            .filter(|c| c.ip.is_ipv6() && c.reason == "subnet_burst")
            .collect();
        assert_eq!(
            v6_blocks.len(),
            60,
            "v6 /64 swarm must batch-block every member IP, got {v6_blocks:?}",
        );
    }

    /// P1-2 regression: a cooled-off /64 (60 historical members in the
    /// lifetime reverse index) plus a small fresh burst (5 hosts, 15 events)
    /// must NOT pass the dual gate and batch-block ~55 innocent hosts.
    /// Pre-fix: v6 gate read lifetime cardinality (60 >= 50) and the block
    /// leg swept the same stale index. Post-fix: both are window-scoped by
    /// `last_seen_ns`, so only the 5 fresh hosts count and get blocked.
    #[test]
    fn v6_cooled_subnet_does_not_block_stale_members() {
        use tokio::sync::mpsc;
        let mut cfg = Config::default();
        cfg.detection.subnet_window_threshold = 1;
        let cfg = cfg.into_handle();
        let store = Arc::new(Store::new(16));
        let metrics = Arc::new(Metrics::new());
        let (etx, mut erx) = mpsc::channel(64);
        let eng = Arc::new(DetectionEngine::new(
            store.clone(),
            cfg,
            etx,
            metrics,
            Arc::new(AtomicBool::new(false)),
        ));
        let v6 = |o: u16| {
            IpAddr::V6(std::net::Ipv6Addr::new(
                0x2001, 0xdb8, 0xbeef, 0, 0, 0, 0, o,
            ))
        };
        // Historical members: promoted with near-zero timestamps, so their
        // last_seen_ns is a long time before `now_ns()`.
        let old: Vec<IpAddr> = (1..=60u16).map(v6).collect();
        let old_events: Vec<_> = old
            .iter()
            .flat_map(|ip| (0..2u64).map(move |i| ev_at(*ip, i)))
            .collect();
        eng.flush_events(&old_events);
        while erx.try_recv().is_ok() {}
        // Assert they actually landed in the reverse index (else the test
        // is not exercising the cooling path).
        assert!(
            store
                .subnet_table()
                .iter()
                .map(|e| e.value().total_rps)
                .sum::<u64>()
                > 0,
            "historical burst must populate the subnet table"
        );
        // Fresh burst: 5 hosts just now, 3 events each = 15 events total.
        let fresh: Vec<IpAddr> = (101..=105u16).map(v6).collect();
        let base = now_ns();
        let fresh_events: Vec<_> = fresh
            .iter()
            .flat_map(|ip| (0..3u64).map(move |i| ev_at(*ip, base + i)))
            .collect();
        eng.flush_events(&fresh_events);
        while erx.try_recv().is_ok() {}
        eng.subnet_batch_scan();
        let cmds: Vec<_> = {
            let mut v = Vec::new();
            while let Ok(c) = erx.try_recv() {
                v.push(c);
            }
            v
        };
        let v6_blocks: Vec<_> = cmds
            .iter()
            .filter(|c| c.ip.is_ipv6() && c.reason == "subnet_burst")
            .collect();
        // Exactly the fresh hosts may be blocked — never the 60 stale ones.
        assert!(
            v6_blocks.len() <= 5,
            "stale members must not be batch-blocked, got {} blocks: {:?}",
            v6_blocks.len(),
            v6_blocks
        );
        assert!(
            v6_blocks.iter().all(|c| fresh.contains(&c.ip)),
            "only window-fresh hosts may be blocked, got {:?}",
            v6_blocks
        );
        // And the gate must not have fired on the lifetime count alone.
        assert!(
            v6_blocks.len() < 50,
            "dual gate must not pass on 60 lifetime members + 5 fresh",
        );
    }

    fn ev_at(ip: IpAddr, ts: u64) -> ConnectionEvent {
        ConnectionEvent {
            ip,
            timestamp_ns: ts,
            bytes: 64,
            status_code: 200,
            proto_fingerprint: 0,
        }
    }

    /// F1 regression: a single IP bursting 10 events must NOT batch-block its /24.
    #[test]
    fn single_ip_burst_does_not_block_subnet() {
        let eng = engine();
        let ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7));
        let events: Vec<_> = (0..10).map(|i| ev_at(ip, i)).collect();
        eng.flush_events(&events);
        let sk = subnet_key_u128(ip).unwrap();
        // DashMap guard must not be held across flush_events (write to same shard deadlocks)
        let uniq = {
            let rec = eng.store.subnet_table().get(&sk).unwrap();
            rec.unique_ips()
        };
        assert_eq!(uniq, 1, "50 re-reports of ONE ip stay one distinct host");
        // window counters accumulate, but the loop's dual gate (50 IPs AND 100
        // events) can't fire from one IP no matter how hard it hammers.
        for _ in 0..50 {
            eng.flush_events(&(0..200).map(|i| ev_at(ip, i)).collect::<Vec<_>>());
        }
        eprintln!("loop done");
        let r = eng.store.subnet_table().get(&sk).unwrap();
        assert_eq!(
            r.unique_ips(),
            1,
            "single-IP traffic never satisfies the unique gate no matter the volume"
        );
    }

    /// F1: volume alone is insufficient — one flood IP at high event count stays unblocked.
    #[test]
    fn raw_volume_alone_insufficient_for_batch_block() {
        let eng = engine();
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));
        // 5000 events, ONE unique IP — old code would block this /24 instantly.
        eng.flush_events(&(0..5000).map(|i| ev_at(ip, i)).collect::<Vec<_>>());
        let sk = subnet_key_u128(ip).unwrap();
        let rec = eng.store.subnet_table().get(&sk).unwrap();
        assert_eq!(
            rec.unique_ips(),
            1,
            "one IP must count as one distinct source"
        );
        assert_eq!(rec.total_rps, 5000); // volume tracked but gated by uniqueness too
    }

    /// F1: swarm signal — many unique IPs × moderate volume satisfies both gates.
    #[test]
    fn distributed_swarm_satisfies_dual_gate() {
        let eng = engine();
        // 60 IPs in same /24, 3 events each = 60 uniq / 180 events ≥ (50, 100)
        let events: Vec<_> = (0..60)
            .flat_map(|o| {
                let ip = IpAddr::V4(Ipv4Addr::new(45, 148, 10, o as u8 + 1));
                (0..3).map(move |i| ev_at(ip, i))
            })
            .collect();
        eng.flush_events(&events);
        let any_ip = IpAddr::V4(Ipv4Addr::new(45, 148, 10, 5));
        let sk = subnet_key_u128(any_ip).unwrap();
        let rec = eng.store.subnet_table().get(&sk).unwrap();
        assert_eq!(rec.unique_ips(), 60, "60 distinct hosts in bitmap");
        assert_eq!(rec.total_rps, 180);
        // dual-gate predicate (same as subnet_batch_loop) now true:
        assert!(rec.unique_ips() >= 50 && rec.total_rps >= 100);
    }

    /// F1: window rollover de-arms — quiet subnet resets both counters.
    #[test]
    fn window_rollover_resets_counters() {
        let store = Store::new(16);
        let t0 = 1_000_000_000;
        let mk = |o: u8| IpAddr::V4(Ipv4Addr::new(192, 0, 2, o));
        let any = mk(9);
        let net = crate::IpNetwork::of_ip(any);
        let sk = subnet_key_u128(any).unwrap();
        // 60 distinct hosts in one window
        let hosts: Vec<std::net::IpAddr> = (1..=60u8).map(mk).collect();
        store.merge_subnet_window(sk, net, 480, Some(&hosts), t0);
        assert_eq!(store.subnet_table().get(&sk).unwrap().unique_ips(), 60);
        // next window (>2s later): fresh attacker or benign traffic starts clean
        store.merge_subnet_window(sk, net, 3, Some(&[mk(200)]), t0 + 3_000_000_000);
        let rec = store.subnet_table().get(&sk).unwrap();
        assert_eq!(
            rec.unique_ips(),
            1,
            "stale swarm signal must not survive rollover"
        );
        assert_eq!(rec.total_rps, 3);
    }

    /// P0 regression: bloom must clear periodically — without clear, every bit
    /// becomes set within hours and cold-skip stops skipping anything,
    /// ballooning the store to O(total_ips_ever_seen).
    #[test]
    fn bloom_clear_resets_all_bits() {
        let mut bf = BloomFilter::new(1024);
        let ip: IpAddr = "192.168.0.1".parse().unwrap();
        bf.insert(ip);
        assert!(bf.contains(ip));
        bf.clear();
        assert!(!bf.contains(ip), "clear() must reset the filter to empty");
    }

    /// P0 regression: detection flush must record the per-flush promoted count
    /// (not the total store size) in the metrics counter. Without this fix the
    /// dashboard shows the cumulative store size, which is unrelated to
    /// per-window throughput and makes the metric meaningless.
    #[test]
    fn flush_records_per_flush_promoted_count() {
        use ramshield_config::Config;
        let cfg = Config::default();
        let store = Arc::new(Store::new(16));
        let metrics = Arc::new(Metrics::new());
        let (etx, _erx) = mpsc::channel(64);
        let shutdown = Arc::new(AtomicBool::new(false));
        // Tighten thresholds so the synthetic traffic is hot enough to
        // promote without flapping into blocks.
        let mut cfg = cfg;
        cfg.detection.promote_min_events = 2;
        cfg.detection.subnet_window_threshold = 1;
        let eng = Arc::new(DetectionEngine::new(
            store.clone(),
            cfg.into_handle(),
            etx,
            metrics,
            shutdown,
        ));
        // 3 distinct /24s, each with a hot IP, in a SINGLE flush.
        let events: Vec<_> = (0..3u8)
            .flat_map(|n| {
                let ip = IpAddr::V4(Ipv4Addr::new(10, 20, 30, n + 1));
                (0..5).map(move |i| ConnectionEvent {
                    ip,
                    timestamp_ns: i,
                    bytes: 64,
                    status_code: 200,
                    proto_fingerprint: 0,
                })
            })
            .collect();
        eng.flush_events(&events);
        let promoted = eng.store.traffic.promoted_ips.load(Ordering::Relaxed);
        // promote_min_events=2, 3 IPs each with 5 events, all get promoted.
        // The metric should report 3, not store.len() which could be larger
        // or smaller depending on prior state.
        assert_eq!(
            promoted, 3,
            "promoted_ips counter must reflect this flush's promoted count, not store.len()"
        );
    }
}
