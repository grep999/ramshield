use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use tokio::sync::broadcast;
use tracing::{error, info, warn};
use tokio::sync::mpsc;

use crate::config::DetectionConfig;
use crate::metrics::Metrics;
use crate::storage::Store;
use crate::error::RamShieldError;

use dashmap::DashMap;

pub mod batch;
pub mod rate_tracker;

pub use batch::{Batch, BloomFilter, ConnectionEvent};
pub use rate_tracker::RateTracker;

pub struct DetectionEngine {
    config: Arc<DetectionConfig>,
    metrics: Arc<Metrics>,
    storage: Arc<Store>,
    event_tx: mpsc::Sender<ConnectionEvent>,
    event_rx: Mutex<Option<mpsc::Receiver<ConnectionEvent>>>,
    bloom_filter: Mutex<BloomFilter>,
    ip_trackers: Arc<DashMap<String, Mutex<RateTracker>>>,
    shutdown_flag: Arc<AtomicBool>,
}

impl DetectionEngine {
    pub fn new(config: DetectionConfig, metrics: Arc<Metrics>, storage: Arc<Store>) -> Self {
        let (event_tx, event_rx) = mpsc::channel(8192);
        let bloom_filter = Mutex::new(BloomFilter::new(1024));
        let ip_trackers = Arc::new(DashMap::new());
        Self {
            config: Arc::new(config),
            metrics,
            storage,
            event_tx,
            event_rx: Mutex::new(Some(event_rx)),
            bloom_filter,
            ip_trackers,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn event_sender(&self) -> mpsc::Sender<ConnectionEvent> {
        self.event_tx.clone()
    }

    pub fn shutdown_flag(&self) -> Arc<AtomicBool> {
        self.shutdown_flag.clone()
    }

    pub fn submit_event(&self, event: ConnectionEvent) -> Result<(), RamShieldError> {
        let ip_str = event.ip.clone();
        let current_ms = event.timestamp_ns / 1_000_000;

        {
            let mut bf = self.bloom_filter.lock()
                .map_err(|e| RamShieldError::DetectionError(format!("BloomFilter error: {}", e)))?;
            if bf.contains(&ip_str) {
                let rt_entry = self.ip_trackers.entry(ip_str.clone())
                    .or_insert_with(|| Mutex::new(RateTracker::new(self.config.rps_threshold)));
                let mut rt_guard = rt_entry.lock()
                    .map_err(|e| RamShieldError::DetectionError(format!("RateTracker lock: {}", e)))?;
                rt_guard.record_request(event.timestamp_ns, current_ms);
                if rt_guard.should_block(current_ms) {
                    self.metrics.inc_rejected(1);
                    return Err(RamShieldError::DetectionError(
                        format!("Blocked event from known IP {}: rate exceeded", ip_str)));
                }
            } else {
                bf.add(&ip_str);
            }
        }

        self.metrics.inc_requests();
        match self.event_tx.try_send(event) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.metrics.inc_rejected(1);
                Err(RamShieldError::DetectionError("Event channel full — backpressure".into()))
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err(RamShieldError::DetectionError("Event channel closed".into()))
            }
        }
    }

    pub async fn start(&self, _shutdown_rx: broadcast::Receiver<()>) -> Result<(), RamShieldError> {
        info!("DetectionEngine: Starting batch processor loop...");

        let mut event_rx = self.event_rx.lock()
            .map_err(|e| RamShieldError::DetectionError(format!("event_rx lock: {}", e)))?
            .take()
            .ok_or_else(|| RamShieldError::DetectionError("Event receiver already taken".into()))?;

        let metrics = self.metrics.clone();
        let storage = self.storage.clone();
        let config = self.config.clone();
        let ip_trackers = self.ip_trackers.clone();
        let shutdown = self.shutdown_flag.clone();

        std::thread::Builder::new()
            .name("batch-processor".into())
            .spawn(move || {
                let mut batch = Batch::new();
                let max_events = config.batch_max_events;
                let block_duration_ms = config.block_duration_ms;

                while !shutdown.load(Ordering::Acquire) {
                    match event_rx.try_recv() {
                        Ok(event) => {
                            batch.add_event(event);
                            if batch.is_full(max_events) {
                                Self::flush_batch(&mut batch, &metrics, &storage, &storage,
                                    &ip_trackers, &config, block_duration_ms);
                            }
                        }
                        Err(mpsc::error::TryRecvError::Empty) => {
                            if !batch.is_empty() {
                                // Flush partial batch after 1ms idle
                                std::thread::sleep(std::time::Duration::from_millis(1));
                                Self::flush_batch(&mut batch, &metrics, &storage, &storage,
                                    &ip_trackers, &config, block_duration_ms);
                            } else {
                                std::thread::sleep(std::time::Duration::from_millis(1));
                            }
                        }
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            info!("DetectionEngine: Channel disconnected, shutting down.");
                            break;
                        }
                    }
                }

                info!("DetectionEngine: Batch processor thread shutting down.");
                if !batch.is_empty() {
                    Self::flush_batch(&mut batch, &metrics, &storage, &storage,
                        &ip_trackers, &config, block_duration_ms);
                }
            })
            .map_err(|e| RamShieldError::DetectionError(
                format!("Failed to spawn batch-processor thread: {}", e)))?;

        Ok(())
    }

    fn flush_batch(
        batch: &mut Batch,
        metrics: &Arc<Metrics>,
        storage: &Arc<Store>,
        store: &Store,
        ip_trackers: &Arc<DashMap<String, Mutex<RateTracker>>>,
        config: &Arc<DetectionConfig>,
        block_duration_ms: u64,
    ) {
        let event_count = batch.event_count.load(std::sync::atomic::Ordering::Relaxed) as usize;
        let mut unique_ips = std::collections::HashSet::new();
        let mut batch_blocks = 0u32;

        metrics.inc_ingested(batch.event_count.load(std::sync::atomic::Ordering::Relaxed));
        metrics.inc_batches();

        for event in batch.events.drain(..) {
            let ip = event.ip.clone();
            let current_ms = event.timestamp_ns / 1_000_000;
            unique_ips.insert(ip.clone());

            let entry = ip_trackers.entry(ip.clone()).or_insert_with(|| {
                let mut rt = RateTracker::new(config.rps_threshold);
                rt.ewma_alpha = config.ewma_alpha;
                rt.threat_threshold = config.threat_threshold;
                Mutex::new(rt)
            });

            if let Ok(mut tracker) = entry.lock() {
                tracker.record_request(event.timestamp_ns, current_ms);

                if tracker.should_block(current_ms) && !store.is_blocked(&ip) {
                    tracker.block(block_duration_ms, current_ms);
                    store.block_ip(&ip, "rate_limit", Some(block_duration_ms / 1000));
                    metrics.inc_blocks();
                    metrics.record_block(&ip, "rate_limit", "detection");
                    batch_blocks += 1;
                    warn!("Blocking IP {} — RPS {:.0} (threshold {})", ip, tracker.ewma_rps, config.rps_threshold);
                }
            } else {
                error!("Failed to lock RateTracker for IP {}", ip);
            }

            if let Err(e) = storage.insert(ip.clone(), event) {
                metrics.inc_rejected(1);
                if !format!("{}", e).contains("capacity reached") {
                    error!("Failed to store event for IP {}: {}", ip, e);
                }
            }
        }

        metrics.record_batch(crate::metrics::BatchRecord {
            ts_ms: crate::metrics::now_secs(),
            events: event_count as u32,
            unique_ips: unique_ips.len() as u32,
            promoted: 0,
            cold_skipped: 0,
            promoted_events: 0,
            cold_skipped_events: 0,
            blocks: batch_blocks,
            hot_subnets: 0,
        });

        batch.event_count.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DetectionConfig;

    fn make_engine() -> DetectionEngine {
        let cfg = DetectionConfig::default();
        let metrics = Arc::new(Metrics::new());
        let store = Arc::new(Store::new(1024));
        DetectionEngine::new(cfg, metrics, store)
    }

    #[test]
    fn submit_event_accepts_single_event() {
        let engine = make_engine();
        let event = ConnectionEvent {
            ip: "1.2.3.4".to_string(),
            timestamp_ns: 1000,
            bytes: 512,
            status_code: 200,
            proto_fingerprint: 0,
        };
        assert!(engine.submit_event(event).is_ok());
    }

    #[test]
    fn event_sender_clones() {
        let engine = make_engine();
        let _sender = engine.event_sender();
    }
}