//! Detection engine — batch-first, subnet-scale diagnosis.
//! Unified: engine pipeline from src (BloomFilter, pre-aggs, workers) +
//! crate batch.rs aggregation (IPv6 /64 keys, IpNetwork metadata).

pub mod batch;
pub mod rate_tracker;

mod bloom;
mod flush;
mod merge;
mod mitigation;
mod pre_aggs;
mod subnet_scan;
mod workers;

use ahash::AHashMap as HashMap;
use arc_swap::ArcSwap;
use batch::{IpAgg, aggregate};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use dashmap::DashMap;
use ramshield_config::{ConfigHandle, DetectionConfig};
use ramshield_metrics::Metrics;
use ramshield_storage::{
    BlockState, IpRecord, SUBNET_WINDOW_NS, Store, SubnetKey, subnet_key_u128,
};
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
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

// ── Bloom filter — 2-hash, no false negatives for inserted IPs ───────────────
// No Clone: atomic words are mutated in place through the shared Arc.
pub struct BloomFilter {
    // Atomic words: insert runs through the SHARED Arc (flush thread) while
    // the 8 s epoch clear (subnet_batch_loop thread) swaps the Arc. No
    // clone + store per flush; a write racing a clear lands in the
    // discarded generation = one missed revisit = the gate opens once more.
    // Advisory, so that is the intended failure mode.
    bits: Vec<AtomicU64>,
    size: usize,
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
    /// P2: CGNAT graduated mitigation guard, clamped tier dispatch.
    cgnat_guard: ramshield_cgnat::CgnatGuard,
    /// P2: Shared memory rule table for proxy lookups (<15ns).
    shm_table: Arc<ramshield_cgnat::ShmTableManager>,
    /// (ip, reason) admission gate: key -> last-admitted wall-clock ns.
    /// A re-emit for a key inside its cooldown (ttl/2, or 30s for permanent
    /// blocks) is suppressed: the enforcement layer keeps the block alive
    /// and re-admission at the cooldown boundary is the TTL refresh — the
    /// same mitigation no longer re-does WAL append + store write + XDP
    /// apply every flush window. Queue rejection removes the key so the
    /// next window retries instead of suppressing forever.
    pending_mitigations: DashMap<(IpAddr, BlockReason), u64, ahash::RandomState>,
}

/// Releases the single-flusher gate even on early return/panic.
struct FlushGuard<'a>(&'a AtomicBool);
impl Drop for FlushGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl DetectionEngine {
    /// Test-only panic constructor. Production code must use [`Self::try_new`].
    #[cfg(test)]
    pub fn new(
        store: Arc<Store>,
        config: ConfigHandle,
        enforcement_tx: mpsc::Sender<EnforceCommand>,
        metrics: Arc<Metrics>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self::try_new(store, config, enforcement_tx, metrics, shutdown)
            .expect("detection SHM initialization failed in test")
    }

    /// Fallible constructor used by production boot and tests.
    ///
    /// SHM open failures surface as `Err` so callers can fail closed without a panic.
    pub fn try_new(
        store: Arc<Store>,
        config: ConfigHandle,
        enforcement_tx: mpsc::Sender<EnforceCommand>,
        metrics: Arc<Metrics>,
        shutdown: Arc<AtomicBool>,
    ) -> std::io::Result<Self> {
        Self::try_new_with_shm_path(
            store,
            config,
            enforcement_tx,
            metrics,
            shutdown,
            &ramshield_cgnat::ShmTableManager::default_path(),
        )
    }

    /// Like [`Self::try_new`] but opens the SHM table at `shm_path`.
    ///
    /// Used by tests to prove initialization failures propagate as `Err`.
    pub fn try_new_with_shm_path(
        store: Arc<Store>,
        config: ConfigHandle,
        enforcement_tx: mpsc::Sender<EnforceCommand>,
        metrics: Arc<Metrics>,
        shutdown: Arc<AtomicBool>,
        shm_path: &std::path::Path,
    ) -> std::io::Result<Self> {
        let bloom_bits = config.load().detection.bloom_bits;
        // Patch A: publish capacity up front so bloom_fp_ppm has a
        // denominator before the first flush (otherwise the gauge reads 0
        // and the FP estimate is silently disabled).
        metrics.set_bloom_bits(bloom_bits as u64);
        // 64k cap ≈ 4MB RSS, fills in ~64ms at 1M eps — keeps the batch
        // processor honest without megabytes of dead head-of-line buffer.
        // Exported so IPC telemetry can't drift from the real size (F3).
        // ponytail: hardcoded; lift to Config.detection.batch_channel_capacity
        // when traffic profiles diverge.
        let (tx, rx) = bounded::<ConnectionEvent>(CHANNEL_CAPACITY as usize);
        let shm_table = Arc::new(ramshield_cgnat::ShmTableManager::open_or_create(shm_path)?);
        // pre_aggs writers = batch workers (≤ cores), so 64 shards removes
        // every realistic cross-thread collision. The old derivation
        // (bloom_bits/1024) sized the shard array off an UNRELATED
        // structure: prod 1M-bit bloom -> 1024 shards, stress 8M -> 8192 —
        // every shard lookup chased a huge array of cache lines it could
        // never reuse (TLB tax per event). Shards should track worker
        // count, not bloom size. ponytail: revisit if workers ever > 64.
        Ok(Self {
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
            // P2: one SHM open shared by proxy and CGNAT guard.
            shm_table: shm_table.clone(),
            cgnat_guard: ramshield_cgnat::CgnatGuard::new(),
            pending_mitigations: DashMap::with_hasher_and_shard_amount(
                ahash::RandomState::new(),
                32,
            ),
        })
    }

    pub fn event_sender(&self) -> Sender<ConnectionEvent> {
        self.event_tx.clone()
    }

    /// P1-8: real ingest-channel depth for the dashboard/healthz backpressure
    /// signal (was a hardcoded 0 stub). tokio mpsc len() is O(1) atomic.
    pub fn event_queue_depth(&self) -> usize {
        self.event_rx.len()
    }
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

#[cfg(test)]
mod tests;
