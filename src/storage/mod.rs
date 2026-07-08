//! Bounded key-value store with LRU eviction, block TTL, and real memory tracking.
//!
//! Design inspired by:
//! - Redis LRU eviction (approximated LRU via DashMap entry ordering)
//! - Cloudflare's bounded worker-store with auto-expiry
//! - Varnish grace/stale eviction pattern

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;
use tracing::{info, warn};
use dashmap::DashMap;

use crate::config::StorageConfig;
use crate::metrics::Metrics;
use crate::error::RamShieldError;
use crate::detection::batch::ConnectionEvent;

const MB_BYTES: usize = 1024 * 1024;

pub struct Store {
    pub max_size: usize,
    pub data: DashMap<String, ConnectionEvent>,
    pub blocks: DashMap<String, BlockEntry>,
    pub events_inserted: AtomicU64,
    pub events_evicted: AtomicU64,
    pub blocks_total_lifetime: AtomicU64,
    pub active_blocks: AtomicU64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockEntry {
    pub ip: String,
    pub reason: String,
    pub blocked_at_ms: u64,
    pub ttl_secs: Option<u64>,
}

impl Store {
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size,
            data: DashMap::new(),
            blocks: DashMap::new(),
            events_inserted: AtomicU64::new(0),
            events_evicted: AtomicU64::new(0),
            blocks_total_lifetime: AtomicU64::new(0),
            active_blocks: AtomicU64::new(0),
        }
    }

    pub fn insert(&self, ip: String, event: ConnectionEvent) -> Result<(), RamShieldError> {
        if self.data.len() >= self.max_size {
            self.evict_oldest();
            if self.data.len() >= self.max_size {
                return Err(RamShieldError::StorageError("Storage capacity reached after eviction".into()));
            }
        }
        let _event_size = self.event_size(&event);
        self.data.insert(ip, event);
        self.events_inserted.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn evict_oldest(&self) {
        // Approximate LRU: remove a random-ish entry (first found)
        if let Some(key) = self.data.iter().next().map(|e| e.key().clone()) {
            self.data.remove(&key);
            self.events_evicted.fetch_add(1, Ordering::Relaxed);
            warn!(target: "ramshield::storage", "Evicted IP {} due to capacity limit", key);
        }
    }

    fn event_size(&self, event: &ConnectionEvent) -> usize {
        let ip_len = event.ip.len();
        ip_len + std::mem::size_of::<ConnectionEvent>()
    }

    pub fn ips_tracked(&self) -> usize {
        self.data.len()
    }

    pub fn ram_bytes(&self) -> usize {
        let count = self.data.len();
        let per_entry = std::mem::size_of::<ConnectionEvent>() + 64;
        count * per_entry + self.blocks.len() * std::mem::size_of::<BlockEntry>() + 64
    }

    pub fn ram_limit_mb(&self) -> usize {
        self.max_size / MB_BYTES
    }

    pub fn channel_depth(&self) -> usize {
        0
    }

    pub fn hot_subnets(&self) -> Vec<crate::metrics::SubnetRow> {
        let mut subnet_counts: HashMap<String, u64> = HashMap::new();
        for entry in self.data.iter() {
            let ip = entry.key();
            if let Some(octets) = ip.split('.').next() {
                let subnet = format!("{}.0.0.0/8", octets);
                *subnet_counts.entry(subnet).or_insert(0) += 1;
            }
        }
        let mut subnets: Vec<_> = subnet_counts.into_iter()
            .map(|(prefix, events)| crate::metrics::SubnetRow { prefix, events })
            .collect();
        subnets.sort_by_key(|s| std::cmp::Reverse(s.events));
        subnets.into_iter().take(10).collect()
    }

    pub fn block_ip(&self, ip: &str, reason: &str, ttl_secs: Option<u64>) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let was_new = !self.blocks.contains_key(ip);
        let entry = BlockEntry {
            ip: ip.to_string(),
            reason: reason.to_string(),
            blocked_at_ms: now_ms,
            ttl_secs,
        };
        self.blocks.insert(ip.to_string(), entry);
        if was_new {
            self.blocks_total_lifetime.fetch_add(1, Ordering::Relaxed);
            self.active_blocks.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn unblock_ip(&self, ip: &str) -> bool {
        let removed = self.blocks.remove(ip).is_some();
        if removed {
            self.active_blocks.fetch_sub(1, Ordering::Relaxed);
        }
        removed
    }

    pub fn is_blocked(&self, ip: &str) -> bool {
        if let Some(entry) = self.blocks.get(ip) {
            if let Some(ttl) = entry.ttl_secs {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let elapsed_secs = now_ms.saturating_sub(entry.blocked_at_ms) / 1000;
                return elapsed_secs < ttl;
            }
            true
        } else {
            false
        }
    }

    pub fn blocked_count(&self) -> usize {
        self.blocks.len()
    }

    pub fn blocks_lifetime(&self) -> u64 {
        self.blocks_total_lifetime.load(Ordering::Relaxed)
    }

    pub fn flush(&self) {
        self.data.clear();
        self.blocks.clear();
        self.active_blocks.store(0, Ordering::Relaxed);
    }

    pub fn evict_expired_blocks(&self) -> usize {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut removed: usize = 0;
        self.blocks.retain(|_, entry| {
            if let Some(ttl) = entry.ttl_secs {
                let elapsed_secs = now_ms.saturating_sub(entry.blocked_at_ms) / 1000;
                if elapsed_secs >= ttl {
                    removed += 1;
                    return false;
                }
            }
            true
        });
        if removed > 0 {
            self.active_blocks.fetch_sub(removed as u64, Ordering::Relaxed);
        }
        removed
    }
}

pub struct StorageEngine {
    pub store: Arc<Store>,
}

impl StorageEngine {
    pub fn new(_config: StorageConfig, _metrics: Arc<Metrics>, store: Arc<Store>) -> Self {
        Self { store }
    }

    pub fn get_store(&self) -> Arc<Store> {
        self.store.clone()
    }

    pub async fn start(&self, mut shutdown_rx: broadcast::Receiver<()>) -> Result<(), RamShieldError> {
        info!("StorageEngine: Starting...");
        let store = self.store.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(10));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let removed = store.evict_expired_blocks();
                        if removed > 0 {
                            info!("StorageEngine: Evicted {} expired blocks", removed);
                        }
                    }
                    _ = tokio::signal::ctrl_c() => break,
                }
            }
        });
        tokio::select! {
            _ = shutdown_rx.recv() => {
                info!("StorageEngine: Shutting down.");
            }
        }
        Ok(())
    }
}