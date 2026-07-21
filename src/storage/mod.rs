pub mod blob_store;
pub mod ttl_wheel;
pub mod wal;

use crate::error::{Result, RsError};
use crate::util::BoundedVecDeque;
use ahash::AHashMap;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

pub const INLINE_MAX: usize = 64;

/// Incremental traffic counters — updated on batch flush, read by forecasting
/// without scanning the full store (Kafka-style consumer lag / Prometheus counters).
#[derive(Debug)]
pub struct TrafficCounters {
    pub events_last_second: AtomicU64,
    pub unique_ips_window: AtomicU64,
    pub promoted_ips: AtomicU64,
    /// Subnet event counts from the latest flush window (for entropy at scale).
    pub subnet_window: Mutex<Vec<u64>>,
    /// High-threat IPs from latest flush (bounded sample for preemptive block).
    pub threat_sample: Mutex<Vec<(IpAddr, f32)>>,
}

impl TrafficCounters {
    pub fn new() -> Self {
        Self {
            events_last_second: AtomicU64::new(0),
            unique_ips_window: AtomicU64::new(0),
            promoted_ips: AtomicU64::new(0),
            subnet_window: Mutex::new(Vec::with_capacity(256)),
            threat_sample: Mutex::new(Vec::with_capacity(128)),
        }
    }

    pub fn record_flush(&self, total_events: u64, unique_ips: u64, subnet_counts: &[u64]) {
        self.events_last_second
            .store(total_events, Ordering::Relaxed);
        self.unique_ips_window.store(unique_ips, Ordering::Relaxed);
        if let Ok(mut v) = self.subnet_window.lock() {
            v.clear();
            v.extend_from_slice(subnet_counts);
        }
    }

    pub fn set_threat_sample(&self, sample: Vec<(IpAddr, f32)>) {
        if let Ok(mut v) = self.threat_sample.lock() {
            *v = sample;
        }
    }
}

impl Default for TrafficCounters {
    fn default() -> Self {
        Self::new()
    }
}

/// Stable store key for an IP — computed once per flush, not per event.
#[inline]
pub fn ip_key(ip: IpAddr) -> String {
    ip.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    Counter(u64),
    Float(f64),
    /// Small payloads stored as Vec<u8>.
    /// Note: we intentionally avoid [u8; 64] because serde only auto-derives
    /// fixed arrays up to [T; 32]. Vec<u8> is serde-compatible at any size.
    Inline(Vec<u8>),
    Blob(Vec<u8>),
    IpRecord(IpRecord),
    SubnetRecord(SubnetRecord),
}

impl Value {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() <= INLINE_MAX {
            Value::Inline(bytes.to_vec())
        } else {
            Value::Blob(bytes.to_vec())
        }
    }

    pub fn heap_bytes(&self) -> usize {
        match self {
            Value::Inline(v) => v.len(),
            Value::Blob(v) => v.len(),
            Value::IpRecord(_) => std::mem::size_of::<IpRecord>(),
            Value::SubnetRecord(_) => std::mem::size_of::<SubnetRecord>(),
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpRecord {
    pub ip: IpAddr,
    pub request_count: u64,
    pub ewma_rps: f64,
    pub baseline_rps: f64,    // New field
    pub baseline_threat: f32, // New field
    pub behavior_history_rps: BoundedVecDeque<f64>,
    pub behavior_history_threat: BoundedVecDeque<f32>,
    pub first_seen_ns: u64,
    pub last_seen_ns: u64,
    pub bytes_in: u64,
    pub status_dist: [u32; 5],
    pub proto_fingerprint: u32,
    pub country: [u8; 2],
    pub threat_score: f32,
    pub block_state: BlockState,
    pub asn: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BlockState {
    Clean,
    Suspicious,
    Blocked { reason: BlockReason, since_ns: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BlockReason {
    HighRps(u64),
    SubnetBatch,
    ForecastAnomaly,
    EntropyAnomaly,
    ManualBlock,
}

impl std::fmt::Display for BlockReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockReason::HighRps(r) => write!(f, "high_rps:{}", r),
            BlockReason::SubnetBatch => write!(f, "subnet_batch"),
            BlockReason::ForecastAnomaly => write!(f, "forecast_anomaly"),
            BlockReason::EntropyAnomaly => write!(f, "entropy_anomaly"),
            BlockReason::ManualBlock => write!(f, "manual"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubnetRecord {
    pub prefix: [u8; 3],
    pub active_ips: u32,
    pub blocked_ips: u32,
    pub total_rps: u64,
    pub threat_score: f32,
    pub last_updated_ns: u64,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub value: Value,
    pub expires_at: Option<Instant>,
    pub version: u64,
}

impl Entry {
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|e| Instant::now() > e)
    }
}

pub struct Store {
    inner: Arc<DashMap<String, Entry>>,
    subnet_table: Arc<DashMap<u32, SubnetRecord>>,
    /// Reverse index: subnet key -> list of IP strings for efficient subnet-based lookups.
    /// Maintained during batch flush to avoid O(store_size) scans.
    subnet_index: Arc<Mutex<AHashMap<u32, AHashMap<String, ()>>>>,
    ram_bytes: Arc<AtomicUsize>,
    pub traffic: Arc<TrafficCounters>,
    pub total_inserts: Arc<AtomicU64>,
    pub total_evictions: Arc<AtomicU64>,
}

impl Store {
    pub fn new(shard_count: usize) -> Self {
        let shards = shard_count.next_power_of_two();
        tracing::debug!("Store::new - Initializing store with {} shards", shards);
        Self {
            inner: Arc::new(DashMap::with_shard_amount(shards)),
            subnet_table: Arc::new(DashMap::with_shard_amount(32)),
            subnet_index: Arc::new(Mutex::new(AHashMap::with_capacity(256))),
            ram_bytes: Arc::new(AtomicUsize::new(0)),
            traffic: Arc::new(TrafficCounters::new()),
            total_inserts: Arc::new(AtomicU64::new(0)),
            total_evictions: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Merge subnet-scale counters from a batch flush (O(subnets in batch)).
    pub fn merge_subnet_window(&self, key: u32, prefix: [u8; 3], events: u32, now_ns: u64) {
        let mut rec = self
            .subnet_table
            .get(&key)
            .map(|e| e.value().clone())
            .unwrap_or(SubnetRecord {
                prefix,
                active_ips: 0,
                blocked_ips: 0,
                total_rps: 0,
                threat_score: 0.0,
                last_updated_ns: now_ns,
            });
        rec.total_rps = rec.total_rps.saturating_add(events as u64);
        rec.last_updated_ns = now_ns;
        self.subnet_table.insert(key, rec);
    }

    pub fn reset_subnet_window(&self, key: u32) {
        if let Some(mut e) = self.subnet_table.get_mut(&key) {
            e.total_rps = 0;
        }
    }

    /// Insert with RAM limit enforcement. Only enforces limit on net-new growth,
    /// allowing replacement of existing entries without triggering capacity errors.
    pub fn insert(
        &self,
        key: String,
        value: Value,
        ttl_secs: Option<u64>,
        ram_limit_bytes: usize,
    ) -> Result<()> {
        tracing::debug!(
            "Store::insert - key: {}, ram_limit_bytes: {}",
            key,
            ram_limit_bytes
        );
        let expires_at = ttl_secs.map(|s| Instant::now() + Duration::from_secs(s));
        let new_entry = Entry {
            value,
            expires_at,
            version: 0,
        };
        let entry_size = key.len() + std::mem::size_of::<Entry>() + new_entry.value.heap_bytes();

        // Insert first, then check adjusted budget (replacement is free)
        let old_size = self.inner.insert(key.clone(), new_entry).map_or(0, |old| {
            std::mem::size_of::<Entry>() + old.value.heap_bytes() + key.len()
        });

        let net_growth = entry_size.saturating_sub(old_size);
        let current = self.ram_bytes.load(Ordering::Relaxed);
        tracing::debug!(
            "Store::insert - current ram_bytes: {}, net_growth: {}",
            current,
            net_growth
        );

        // Only enforce limit on actual growth, not replacement
        if old_size == 0 && current + net_growth > ram_limit_bytes {
            // Rollback
            self.inner.remove(&key);
            tracing::warn!("Store::insert - CapacityExceeded for key: {}", key);
            return Err(RsError::CapacityExceeded {
                limit_mb: ram_limit_bytes / (1024 * 1024),
            });
        }

        self.ram_bytes.fetch_add(net_growth, Ordering::Relaxed);
        self.total_inserts.fetch_add(1, Ordering::Relaxed);
        tracing::debug!("Store::insert - Successfully inserted key: {}", key);
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        let entry = self.inner.get(key)?;
        if entry.is_expired() {
            return None;
        }
        Some(entry.value.clone())
    }

    pub fn increment(&self, key: &str, delta: u64) -> u64 {
        let mut e = self.inner.entry(key.to_string()).or_insert_with(|| Entry {
            value: Value::Counter(0),
            expires_at: None,
            version: 0,
        });
        if let Value::Counter(ref mut c) = e.value {
            *c += delta;
            return *c;
        }
        0
    }

    pub fn evict_batch(&self, keys: &[String]) {
        for key in keys {
            let expired = self.inner.get(key).is_some_and(|e| e.is_expired());
            if expired {
                if let Some((k, e)) = self.inner.remove(key) {
                    let freed = k.len() + std::mem::size_of::<Entry>() + e.value.heap_bytes();
                    self.ram_bytes.fetch_sub(freed, Ordering::Relaxed);
                    self.total_evictions.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    pub fn remove(&self, key: &str) -> Option<Value> {
        self.inner.remove(key).map(|(k, e)| {
            let freed = k.len() + std::mem::size_of::<Entry>() + e.value.heap_bytes();
            self.ram_bytes.fetch_sub(freed, Ordering::Relaxed);
            self.total_evictions.fetch_add(1, Ordering::Relaxed);
            e.value
        })
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    pub fn ram_bytes(&self) -> usize {
        self.ram_bytes.load(Ordering::Relaxed)
    }
    pub fn inner(&self) -> &DashMap<String, Entry> {
        &self.inner
    }
    pub fn subnet_table(&self) -> &DashMap<u32, SubnetRecord> {
        &self.subnet_table
    }

    /// Update the reverse index for subnet lookups. Call after inserting/updating an IP record.
    pub fn update_subnet_index(&self, ip_key: &str, subnet_key: Option<u32>, is_removal: bool) {
        let Some(sk) = subnet_key else { return };

        if let Ok(mut index) = self.subnet_index.lock() {
            if is_removal {
                if let Some(subnet_ips) = index.get_mut(&sk) {
                    subnet_ips.remove(ip_key);
                    if subnet_ips.is_empty() {
                        index.remove(&sk);
                    }
                }
            } else {
                index
                    .entry(sk)
                    .or_insert_with(|| AHashMap::with_capacity(64))
                    .insert(ip_key.to_string(), ());
            }
        }
    }

    /// Get all IP keys in a given subnet using the reverse index (O(1) lookup).
    pub fn get_ips_in_subnet(&self, subnet_key: u32) -> Vec<String> {
        self.subnet_index
            .lock()
            .map(|index| {
                index
                    .get(&subnet_key)
                    .map(|ips| ips.keys().cloned().collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_for_small() {
        let v = Value::from_bytes(&[1u8; 10]);
        assert!(matches!(v, Value::Inline(_)));
    }

    #[test]
    fn blob_for_large() {
        let v = Value::from_bytes(&[1u8; 100]);
        assert!(matches!(v, Value::Blob(_)));
    }

    #[test]
    fn insert_get_remove() {
        let store = Store::new(16);
        store
            .insert("k".into(), Value::Counter(1), None, 64 * 1024 * 1024)
            .unwrap();
        assert!(store.get("k").is_some());
        store.remove("k");
        assert!(store.get("k").is_none());
    }

    #[test]
    fn increment_creates_and_adds() {
        let store = Store::new(16);
        assert_eq!(store.increment("x", 5), 5);
        assert_eq!(store.increment("x", 3), 8);
    }

    #[test]
    fn ttl_lazy_expiry() {
        let store = Store::new(16);
        store
            .insert("x".into(), Value::Counter(1), Some(0), 64 * 1024 * 1024)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(store.get("x").is_none());
    }
}
