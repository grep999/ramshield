//! Detection engine — batch-first, subnet-scale diagnosis.
//! Unified: engine pipeline from src (BloomFilter, pre-aggs, workers) +
//! crate batch.rs aggregation (IPv6 /64 keys, IpNetwork metadata).

pub mod batch;
pub mod rate_tracker;

use anyhow::Result;
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
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

// ── Bloom filter — 2-hash, no false negatives for inserted IPs ───────────────
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

    pub fn slots(ip: &IpAddr) -> (usize, usize) {
        let mut h = DefaultHasher::new();
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
const fn status_bucket(code: u16) -> u8 {
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

pub struct DetectionEngine {
    store: Arc<Store>,
    config: ConfigHandle,
    metrics: Arc<Metrics>,
    event_tx: Sender<ConnectionEvent>,
    event_rx: Arc<Receiver<ConnectionEvent>>,
    enforcement_tx: mpsc::Sender<EnforceCommand>,
    bloom: Arc<RwLock<BloomFilter>>,
    shutdown: Arc<AtomicBool>,
    /// Pre-aggregation buffer — DashMap is internally thread-safe, no Arc needed
    pre_aggs: DashMap<IpAddr, IpAgg>,
    last_pre_aggs_flush_ns: AtomicU64,
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
        // 16k cap ≈ 1MB RSS, fills in 16ms at 1M eps attack rate — keeps
        // the batch processor honest. 256k was ~16MB of dead buffer that
        // delayed inevitable drops and let legitimate events back up
        // head-of-line.
        // ponytail: hardcoded; lift to Config.detection.batch_channel_capacity
        // when traffic profiles diverge.
        let (tx, rx) = bounded::<ConnectionEvent>(16_000);
        let shard_count = (bloom_bits / 1024).max(1).next_power_of_two();
        Self {
            store,
            config,
            metrics,
            event_tx: tx,
            event_rx: Arc::new(rx),
            enforcement_tx,
            bloom: Arc::new(RwLock::new(BloomFilter::new(bloom_bits))),
            shutdown,
            pre_aggs: DashMap::with_shard_amount(shard_count),
            last_pre_aggs_flush_ns: AtomicU64::new(now_ns()),
        }
    }

    pub fn event_sender(&self) -> Sender<ConnectionEvent> {
        self.event_tx.clone()
    }

    /// Submit many events in one channel send (amortises IPC / edge overhead).
    pub fn submit_batch(&self, events: Vec<ConnectionEvent>) -> Result<()> {
        for ev in events {
            self.event_tx
                .send(ev)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }
        Ok(())
    }

    fn pre_aggs_needs_flush_due_to_timeout(&self, interval_ms: u64) -> bool {
        let last_flush = self.last_pre_aggs_flush_ns.load(Ordering::Relaxed);
        now_ns().saturating_sub(last_flush) >= interval_ms * 1_000_000
    }

    fn process_event_into_pre_aggs(&self, ev: ConnectionEvent) {
        let mut entry = self.pre_aggs.entry(ev.ip).or_default();
        let agg = entry.value_mut();
        agg.count += 1;
        if agg.count == 1 {
            agg.first_ts_ns = ev.timestamp_ns;
        }
        agg.bytes += ev.bytes;
        agg.last_ts_ns = ev.timestamp_ns;
        // ponytail: table lookup replaces per-event /100 - 600B const table.
        if ev.status_code < 600 {
            let b = STATUS_BUCKET[ev.status_code as usize];
            if b != 255 {
                agg.status_dist[b as usize] += 1;
            }
        }
    }

    fn flush_pre_aggs_to_store(&self) {
        self.last_pre_aggs_flush_ns
            .store(now_ns(), Ordering::Relaxed);

        if self.pre_aggs.is_empty() {
            return;
        }

        // DashMap has no drain() — take ownership of each value via
        // iter_mut() + mem::take, avoiding the per-IP clone of the
        // `IpAgg` (~56 B) that the old `iter().map(|e| e.value().clone())`
        // path produced. At 1M unique IPs/s the clone churn hit ~50 MB/s
        // and held every shard lock for the full iter walk.
        let mut aggs: Vec<(IpAddr, IpAgg)> = Vec::with_capacity(self.pre_aggs.len());
        for mut e in self.pre_aggs.iter_mut() {
            aggs.push((*e.key(), std::mem::take(e.value_mut())));
        }
        self.pre_aggs.clear();

        let total_events: u64 = aggs.iter().map(|a| a.1.count as u64).sum();
        self.metrics.inc_ingested(total_events);

        let subnet_counts = subnet_counts_of(&aggs);
        self.flush_batch(&aggs, &subnet_counts, &HashMap::new(), total_events);
    }

    pub fn spawn_workers(self: Arc<Self>, _n: usize) {
        let det = self.config.load().detection.clone();
        info!(
            "Detection: batch processor (max {} events / {} ms window)",
            det.batch_max_events, det.batch_window_ms
        );

        // Dedicated OS thread — blocking recv, no Tokio spin (Disruptor / LMAX pattern).
        {
            let eng = self.clone();
            std::thread::Builder::new()
                .name("rs-batch".into())
                .spawn(move || eng.batch_processor_loop())
                .expect("spawn batch processor");
        }

        let eng = self.clone();
        std::thread::Builder::new()
            .name("rs-subnet".into())
            .spawn(move || eng.subnet_batch_loop())
            .expect("spawn subnet batch loop");
    }

    fn batch_processor_loop(&self) {
        let rx = self.event_rx.clone();

        loop {
            if self.shutdown.load(Ordering::Acquire) {
                info!("Batch processor shutting down");
                break;
            }

            // Load config once per iteration (interval is fixed for process lifetime).
            let cfg = self.config.load();
            let window = Duration::from_millis(cfg.detection.batch_window_ms);
            let max = cfg.detection.batch_max_events;

            // Drain events from channel into pre_aggs
            match rx.recv_timeout(window) {
                Ok(ev) => self.process_event_into_pre_aggs(ev),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }

            // Drain remaining events up to batch_max_events
            for _ in 0..max.saturating_sub(1) {
                match rx.try_recv() {
                    Ok(ev) => self.process_event_into_pre_aggs(ev),
                    Err(_) => break,
                }
            }

            // Flush pre_aggs to main store when size or timeout threshold hit
            if self.pre_aggs.len() >= cfg.detection.pre_aggs_max_size
                || self
                    .pre_aggs_needs_flush_due_to_timeout(cfg.detection.pre_aggs_flush_interval_ms)
            {
                self.flush_pre_aggs_to_store();
            }
        }
    }

    /// Test/IPC entry: aggregate raw events, then flush.
    pub fn flush_events(&self, events: &[ConnectionEvent]) {
        let a = aggregate(events);
        let aggs: Vec<(IpAddr, IpAgg)> = a.ips.into_iter().collect();
        self.flush_batch(&aggs, &a.subnets, &a.networks, events.len() as u64);
    }

    /// Single pass over aggregates: promote, merge, emit blocks. No store access for cold IPs.
    fn flush_batch(
        &self,
        ip_aggs: &[(IpAddr, IpAgg)],
        subnet_counts: &HashMap<SubnetKey, (u32, Vec<IpAddr>)>,
        networks: &HashMap<SubnetKey, IpNetwork>,
        total_events: u64,
    ) {
        let cfg = self.config.load();
        let det = &cfg.detection;
        let ram_lim = cfg.engine.ram_limit_mb * 1024 * 1024;
        let now = now_ns();

        // Incremental counters for forecasting (no full-store scan).
        let subnet_vals: Vec<u64> = subnet_counts.values().map(|&(ev, _)| ev as u64).collect();
        self.store
            .traffic
            .record_flush(total_events, ip_aggs.len() as u64, &subnet_vals);

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
            let bloom_hit = self
                .bloom
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_hashed(a, b);

            if agg.count < det.promote_min_events && !subnet_hot && !bloom_hit {
                cold_skipped += 1;
                cold_skipped_events += agg.count;
                continue;
            }

            // ponytail: merge_record does the single store lookup (is_blocked check
            // was a second DashMap hit on the same key).
            let (ewma_rps, threat, should_block, _was_blocked) =
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

            if should_block || is_exceeded(ewma_rps, det.rps_threshold) {
                self.bloom
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert_hashed(a, b);
                blocks.push((ip, BlockReason::HighRps, det.block_ttl_secs));
            }
        }

        threat_sample.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        threat_sample.truncate(128);
        self.store.traffic.push_threat_samples(threat_sample);
        self.store
            .traffic
            .promoted_ips
            .store(self.store.len() as u64, Ordering::Relaxed);

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
        let existing = self.store.get(&ip);
        let was_blocked = match &existing {
            Some(Value::IpRecord(r)) => matches!(r.block_state, BlockState::Blocked { .. }),
            _ => false,
        };
        let mut rec = match existing {
            Some(Value::IpRecord(r)) => r,
            _ => IpRecord {
                ip,
                request_count: 0,
                ewma_rps: 0.0,
                cusum_s: 0.0,
                baseline_rps: 0.0,
                prev_sample_hot: false,
                sample_count: 0,
                pulse_samples_in_window: 0,
                pulse_window_start_ns: 0,
                first_seen_ns: agg.first_ts_ns,
                last_seen_ns: agg.last_ts_ns,
                bytes_in: 0,
                status_dist: [0; 5],
                proto_fingerprint: agg.proto_fp,
                threat_score: 0.0,
                block_state: BlockState::Clean,
            },
        };

        rec.request_count = rec.request_count.saturating_add(agg.count as u64);
        rec.last_seen_ns = agg.last_ts_ns;
        rec.bytes_in = rec.bytes_in.saturating_add(agg.bytes);
        for i in 0..5 {
            rec.status_dist[i] = rec.status_dist[i].saturating_add(agg.status_dist[i]);
        }

        // P1: true instantaneous rate from the batch's own time span — NOT the
        // cumulative count/elapsed-since-first-seen (sawtooth after window
        // halving poisoned the EWMA sample).
        let span_ns = agg.last_ts_ns.saturating_sub(agg.first_ts_ns);
        let inst_rps = if span_ns > 0 {
            agg.count as f64 / (span_ns as f64 / 1e9)
        } else {
            // whole batch inside one clock tick: assume 1s granularity floor
            agg.count as f64
        };
        rec.ewma_rps = ewma(rec.ewma_rps, inst_rps);

        // P1: CUSUM companion (Page 1954) — catches sustained sub-threshold
        // drift that absolute-EWMA can't see by construction.
        let baseline = if rec.baseline_rps == 0.0 {
            rec.baseline_rps = rec.ewma_rps;
            rec.ewma_rps
        } else {
            rec.baseline_rps =
                ewma_alpha_slow() * rec.ewma_rps + (1.0 - ewma_alpha_slow()) * rec.baseline_rps;
            rec.baseline_rps
        };
        rec.sample_count = rec.sample_count.saturating_add(1);
        if rec.sample_count >= CUSUM_WARMUP_SAMPLES {
            let k = cusum_allowance(det.rps_threshold);
            rec.cusum_s = cusum_step_capped(
                rec.cusum_s,
                inst_rps,
                baseline + k,
                det.rps_threshold as f64,
            );
        }

        let rps_score = (rec.ewma_rps / det.rps_threshold as f64).min(1.0);
        let total: u32 = rec.status_dist.iter().sum();
        let err_frac = rec.status_dist[4] as f64 / total.max(1) as f64;
        rec.threat_score = (rps_score * 0.7 + err_frac * 0.3).min(1.0) as f32;

        let window_ns = det.rate_window_secs * 1_000_000_000;
        if now.saturating_sub(rec.first_seen_ns) > window_ns {
            rec.request_count /= 2;
            rec.first_seen_ns = now;
        }

        let ewma_rps = rec.ewma_rps;
        let threat = rec.threat_score;
        let over_threshold = is_exceeded(ewma_rps, det.rps_threshold);
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
            det.pulse_window_secs,
            det.pulse_threshold_samples,
        );
        rec.pulse_samples_in_window = pulse_count;
        rec.pulse_window_start_ns = pulse_start;
        let block = hot || cusum_fired(rec.cusum_s, det.rps_threshold) || pulse_fired;

        if let Err(e) = self.store.insert(ip, Value::IpRecord(rec), None, ram_lim) {
            warn!("Failed to insert IP record for {}: {}", ip, e);
        }
        self.store.update_subnet_index(ip, sk, false);
        (ewma_rps, threat, block, was_blocked)
    }

    /// Subnet-scale batch block — reads subnet_table only, not full store key scan.
    fn subnet_batch_loop(self: Arc<Self>) {
        let tick = std::time::Duration::from_millis(500);
        loop {
            if self.shutdown.load(Ordering::Acquire) {
                info!("Subnet batch loop shutting down");
                break;
            }
            std::thread::sleep(tick);
            let cfg = self.config.load();
            if !cfg.detection.batch_block_enabled {
                continue;
            }
            // Dual gate: unique-IP swarm signal AND raw event volume. Either
            // alone mis-fires (single flood IP trips volume; slow drip from
            // many IPs trips uniqueness).
            let ip_threshold = cfg.detection.subnet_batch_threshold as u64;
            let ev_threshold = cfg.detection.subnet_batch_min_events;

            let hot: Vec<(SubnetKey, u64, u64, [u8; 3])> = self
                .store
                .subnet_table()
                .iter()
                .filter_map(|e| {
                    let r = e.value();
                    if r.unique_ips() >= ip_threshold && r.total_rps >= ev_threshold {
                        Some((*e.key(), r.unique_ips(), r.total_rps, r.prefix))
                    } else {
                        None
                    }
                })
                .collect();

            for (sk, uniq, count, prefix) in hot {
                warn!(
                    "Batch block subnet {:?}.{}.{} ({} IPs / {} events in window)",
                    prefix[0], prefix[1], prefix[2], uniq, count
                );
                info!("Batch blocking subnet key {:#x}", sk);

                // O(1) lookup for IPs in the hot subnet instead of full scan
                let ips_in_subnet = self.store.get_ips_in_subnet(sk);
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
}
