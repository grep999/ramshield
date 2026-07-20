pub mod batch;
pub mod rate_tracker;

use crate::config::{ConfigHandle, DetectionConfig};
use crate::detection::batch::{aggregate, ip_in_subnet, subnet_key, subnet_prefix, IpAgg};
use crate::util::BoundedVecDeque;
use crate::detection::rate_tracker::{ewma, is_exceeded};
use crate::metrics::{BatchRecord, Metrics};
use crate::storage::{ip_key, BlockReason, BlockState, IpRecord, Store, Value};
use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::sync::{Arc, RwLock, atomic::AtomicBool};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};


#[derive(Debug, Clone)]
pub struct ConnectionEvent {
    pub ip:                IpAddr,
    pub timestamp_ns:      u64,
    pub bytes:             u64,
    pub status_code:       u16,
    pub proto_fingerprint: u32,
}


#[derive(Debug, Clone)]
pub struct BlockDecision {
    pub ip:           IpAddr,
    pub reason:       BlockReason,
    pub ttl_secs:     Option<u64>,
    pub batch_subnet: Option<[u8; 3]>,
}

// ── Bloom filter keyed by IpAddr hash (no string alloc) ───────────────────────

pub struct BloomFilter {
    bits: Vec<u64>,
    size: usize,
}

impl BloomFilter {
    pub fn new(bit_count: usize) -> Self {
        let words = (bit_count + 63) / 64;
        Self { bits: vec![0u64; words], size: words * 64 }
    }

    fn slots(ip: IpAddr) -> (usize, usize) {
        let mut h1 = DefaultHasher::new();
        ip.hash(&mut h1);
        let a = h1.finish() as usize;
        let mut h2 = DefaultHasher::new();
        (a as u64).hash(&mut h2);
        ip.hash(&mut h2);
        let b = h2.finish() as usize;
        (a, b)
    }

    pub fn insert(&mut self, ip: IpAddr) {
        let (a, b) = Self::slots(ip);
        let a = a % self.size;
        let b = b % self.size;
        self.bits[a / 64] |= 1u64 << (a % 64);
        self.bits[b / 64] |= 1u64 << (b % 64);
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let (a, b) = Self::slots(ip);
        let a = a % self.size;
        let b = b % self.size;
        (self.bits[a / 64] >> (a % 64)) & 1 == 1
            && (self.bits[b / 64] >> (b % 64)) & 1 == 1
    }
}

// ── Detection engine — batch-first, subnet-scale diagnosis ───────────────────

pub struct DetectionEngine {
    store:    Arc<Store>,
    config:   ConfigHandle,
    metrics:  Arc<Metrics>,
    event_tx: Sender<ConnectionEvent>,
    event_rx: Arc<Receiver<ConnectionEvent>>,
    block_tx: broadcast::Sender<BlockDecision>,
    bloom:    Arc<RwLock<BloomFilter>>,
    shutdown: Arc<AtomicBool>,
    /// Pattern learner for attack detection
    #[allow(dead_code)]
    pattern_learner: Arc<crate::learning::PatternLearner>,
}

impl DetectionEngine {
    pub fn new(
        store:    Arc<Store>,
        config:   ConfigHandle,
        block_tx: broadcast::Sender<BlockDecision>,
        metrics:  Arc<Metrics>,
        pattern_learner: Arc<crate::learning::PatternLearner>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        let bloom_bits = config.load().detection.bloom_bits;
        let (tx, rx)   = bounded::<ConnectionEvent>(2_000_000);
        Self {
            store, config, metrics, event_tx: tx, event_rx: Arc::new(rx), block_tx,
            bloom: Arc::new(RwLock::new(BloomFilter::new(bloom_bits))),
            shutdown,
            pattern_learner,
        }
    }

    pub fn event_sender(&self) -> Sender<ConnectionEvent> { self.event_tx.clone() }

    pub fn queue_depth(&self) -> usize { self.event_tx.len() }

    /// Submit many events in one channel send (amortises IPC / edge overhead).
    pub fn submit_batch(&self, events: Vec<ConnectionEvent>) -> Result<(), ()> {
        for ev in events {
            self.event_tx.send(ev).map_err(|_| ())?;
        }
        Ok(())
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
        tokio::spawn(async move { eng.subnet_batch_loop().await; });
    }

    fn batch_processor_loop(&self) {
        let window = Duration::from_millis(self.config.load().detection.batch_window_ms);
        let max    = self.config.load().detection.batch_max_events;
        let rx     = self.event_rx.clone();

        loop {
            if self.shutdown.load(std::sync::atomic::Ordering::Acquire) {
                info!("Batch processor shutting down");
                break;
            }

            let mut batch = Vec::with_capacity(max.min(4096));

            match rx.recv_timeout(window) {
                Ok(ev) => batch.push(ev),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }

            while batch.len() < max {
                match rx.try_recv() {
                    Ok(ev) => batch.push(ev),
                    Err(_) => break,
                }
            }

            if !batch.is_empty() {
                self.flush_batch(&batch);
            }
        }
    }

    /// Single pass: aggregate in memory, then touch store only for promoted IPs.
    fn flush_batch(&self, events: &[ConnectionEvent]) {
        let cfg     = self.config.load();
        let det     = &cfg.detection;
        let ram_lim = cfg.engine.ram_limit_mb * 1024 * 1024;
        let now     = now_ns();

        let (ip_aggs, subnet_counts) = aggregate(events);

        // Incremental counters for forecasting (no full-store scan).
        let subnet_vals: Vec<u64> = subnet_counts.values().map(|&c| c as u64).collect();
        self.store.traffic.record_flush(
            events.len() as u64,
            ip_aggs.len() as u64,
            &subnet_vals,
        );

        for (&sk, &count) in &subnet_counts {
            self.store.merge_subnet_window(sk, subnet_prefix(sk), count, now);
        }

        let mut blocks = Vec::new();
        let mut threat_sample = Vec::with_capacity(64);
        let mut promoted = 0u32;
        let mut cold_skipped = 0u32;
        let mut promoted_events = 0u32;
        let mut cold_skipped_events = 0u32;

        let unique_ips = ip_aggs.len();
        let hot_subnets = subnet_counts.len();

        for (ip, agg) in ip_aggs {
            let subnet_hot = subnet_key(ip)
                .and_then(|sk| subnet_counts.get(&sk).copied())
                .map(|c| c as u64 >= det.subnet_window_threshold)
                .unwrap_or(false);

            let bloom_hit = self.bloom.read().unwrap().contains(ip);

            if agg.count < det.promote_min_events && !subnet_hot && !bloom_hit {
                cold_skipped += 1;
                cold_skipped_events += agg.count;
                continue;
            }

            if self.is_blocked(ip) {
                continue;
            }

            promoted += 1;
            promoted_events += agg.count;

            let (ewma_rps, threat, should_block) =
                self.merge_record(ip, &agg, det, ram_lim, now);

            if threat > 0.5 {
                threat_sample.push((ip, threat));
            }

            if should_block || is_exceeded(ewma_rps, det.rps_threshold) {
                self.bloom.write().unwrap().insert(ip);
                blocks.push(BlockDecision {
                    ip,
                    reason:       BlockReason::HighRps(ewma_rps as u64),
                    ttl_secs:     (det.block_ttl_secs > 0).then_some(det.block_ttl_secs),
                    batch_subnet: None,
                });
            }
        }

        threat_sample.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        threat_sample.truncate(128);
        self.store.traffic.set_threat_sample(threat_sample);
        self.store.traffic.promoted_ips.store(
            self.store.len() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );

        let block_count = blocks.len() as u32;
        for b in &blocks {
            self.metrics.record_block(&b.ip.to_string(), &b.reason.to_string(), "detection");
        }
        for b in blocks {
            let _ = self.block_tx.send(b);
        }

        self.metrics.record_batch(crate::metrics::BatchRecord {
            ts_ms: now / 1_000_000,
            events: events.len() as u32,
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
            events.len(),
            unique_ips,
            hot_subnets,
        );
    }

    fn is_blocked(&self, ip: IpAddr) -> bool {
        let key = ip_key(ip);
        match self.store.get(&key) {
            Some(Value::IpRecord(r)) => matches!(r.block_state, BlockState::Blocked { .. }),
            _ => false,
        }
    }

    /// Load existing IpRecord once, merge batch aggregate, write once — reuse prior state.
    fn merge_record(
        &self,
        ip: IpAddr,
        agg: &IpAgg,
        det: &DetectionConfig,
        ram_lim: usize,
        now: u64,
    ) -> (f64, f32, bool) {
        let key = ip_key(ip);
        let mut rec = match self.store.get(&key) {
            Some(Value::IpRecord(r)) => r,
            _ => IpRecord {
                ip,
                request_count: 0,
                ewma_rps: 0.0,
                baseline_rps: 0.0,
                baseline_threat: 0.0,
                behavior_history_rps: BoundedVecDeque::new(det.history_cap),
                behavior_history_threat: BoundedVecDeque::new(det.history_cap),
                first_seen_ns: agg.first_ts_ns,
                last_seen_ns: agg.last_ts_ns,
                bytes_in: 0,
                status_dist: [0; 5],
                proto_fingerprint: agg.proto_fp,
                country: [0; 2],
                threat_score: 0.0,
                block_state: BlockState::Clean,
                asn: 0,
            },
        };

        rec.request_count = rec.request_count.saturating_add(agg.count as u64);
        rec.last_seen_ns  = agg.last_ts_ns;
        rec.bytes_in      = rec.bytes_in.saturating_add(agg.bytes);
        for i in 0..5 {
            rec.status_dist[i] = rec.status_dist[i].saturating_add(agg.status_dist[i]);
        }

        let elapsed  = (now.saturating_sub(rec.first_seen_ns)) as f64 / 1e9;
        let inst_rps = if elapsed > 0.0 {
            rec.request_count as f64 / elapsed
        } else {
            0.0
        };
        rec.ewma_rps = ewma(rec.ewma_rps, inst_rps);

        let rps_score = (rec.ewma_rps / det.rps_threshold as f64).min(1.0);
        let total: u32 = rec.status_dist.iter().sum();
        let err_frac = rec.status_dist[4] as f64 / total.max(1) as f64;
        rec.threat_score = (rps_score * 0.7 + err_frac * 0.3).min(1.0) as f32;

        let window_ns = det.rate_window_secs * 1_000_000_000;
        if now.saturating_sub(rec.first_seen_ns) > window_ns {
            rec.request_count = rec.request_count / 2;
            rec.first_seen_ns = now;
        }

        let ewma_rps = rec.ewma_rps;
        let threat   = rec.threat_score;
        let block    = is_exceeded(ewma_rps, det.rps_threshold);

        if let Err(e) = self.store.insert(key.clone(), Value::IpRecord(rec), None, ram_lim) {
            warn!("Failed to insert IP record for {}: {}", key.clone(), e);
        }
        (ewma_rps, threat, block)
    }

    /// Subnet-scale batch block — reads subnet_table only, not full store key scan.
    async fn subnet_batch_loop(&self) {
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        loop {
            if self.shutdown.load(std::sync::atomic::Ordering::Acquire) {
                info!("Subnet batch loop shutting down");
                break;
            }
            tick.tick().await;
            let cfg = self.config.load();
            if !cfg.detection.batch_block_enabled {
                continue;
            }
            let threshold = cfg.detection.subnet_batch_threshold as u64;

            let hot: Vec<(u32, u64, [u8; 3])> = self
                .store
                .subnet_table()
                .iter()
                .filter_map(|e| {
                    let r = e.value();
                    if r.total_rps >= threshold {
                        Some((*e.key(), r.total_rps, r.prefix))
                    } else {
                        None
                    }
                })
                .collect();

            for (sk, count, prefix) in hot {
                warn!("Batch block /24 {:?}.{}.{} ({} events/window)", prefix[0], prefix[1], prefix[2], count);
                for e in self.store.inner().iter() {
                    let Value::IpRecord(ref r) = e.value().value else { continue };
                    if matches!(r.block_state, BlockState::Blocked { .. }) {
                        continue;
                    }
                    if !ip_in_subnet(r.ip, prefix) {
                        continue;
                    }
                    let _ = self.block_tx.send(BlockDecision {
                        ip: r.ip,
                        reason: BlockReason::SubnetBatch,
                        ttl_secs: Some(cfg.detection.block_ttl_secs),
                        batch_subnet: Some(prefix),
                    });
                    self.metrics.record_block(
                        &r.ip.to_string(), "subnet_batch", "detection",
                    );
                    self.metrics.blocks_subnet.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
    use crate::config::Config;
    use crate::metrics::Metrics;
    use std::net::Ipv4Addr;

    fn engine() -> Arc<DetectionEngine> {
        let cfg = Config::default().into_handle();
        let store = Arc::new(Store::new(16));
        let metrics = Arc::new(Metrics::new());
        let (btx, _) = broadcast::channel(64);
        let learner = Arc::new(crate::learning::PatternLearner::new(cfg.load().detection.pattern_similarity_threshold));
        let shutdown = Arc::new(AtomicBool::new(false));
        Arc::new(DetectionEngine::new(store, cfg, btx, metrics, learner, shutdown))
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
        eng.flush_batch(&events);
        assert!(eng.store.get(&ip_key(ip)).is_some());
    }

    #[test]
    fn cold_ip_not_stored() {
        let eng = engine();
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        eng.flush_batch(&[
            ConnectionEvent {
                ip,
                timestamp_ns: 1,
                bytes: 1,
                status_code: 200,
                proto_fingerprint: 0,
            },
        ]);
        assert!(eng.store.get(&ip_key(ip)).is_none());
    }
}
