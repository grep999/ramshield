//! Unified RamShield storage: sharded in-memory Store (from src, perf-tuned)
//! + WAL / BlobStore durability modules (from crate).
//!
//! Types: `Value::IpRecord` is the canonical per-IP entry; subnet keys are
//! `u128` (IPv4 packed low-32, IPv6 full address) with `IpNetwork` metadata.

pub mod atomic_ops;
pub mod blob_store;
pub mod subnet;
pub mod wal;

pub use subnet::{subnet_key_u128, subnet_key_v4, subnet_key_v6};

use crossbeam_queue::SegQueue;
use dashmap::DashMap;
use ramshield_types::{BlockReason, IpNetwork, Result, RsError};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
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
    /// Lock-free atomic array for concurrent reads/writes from detection and forecasting.
    pub subnet_window: [AtomicU64; 256],
    /// High-threat IPs from latest flush (bounded sample for preemptive block).
    /// Lock-free unbounded MPMC queue.
    pub threat_sample: SegQueue<(IpAddr, f32)>,
    /// RAM limit in MB from config.
    pub ram_limit_mb: AtomicUsize,
    /// Byte-precise usage tracking (crate port — complements ram_bytes estimate).
    pub used_bytes: AtomicU64,
    /// Process uptime in seconds.
    pub uptime_secs: AtomicU64,
}

impl TrafficCounters {
    pub fn new() -> Self {
        Self {
            events_last_second: AtomicU64::new(0),
            unique_ips_window: AtomicU64::new(0),
            promoted_ips: AtomicU64::new(0),
            subnet_window: std::array::from_fn(|_| AtomicU64::new(0)),
            threat_sample: SegQueue::new(),
            ram_limit_mb: AtomicUsize::new(0),
            used_bytes: AtomicU64::new(0),
            uptime_secs: AtomicU64::new(0),
        }
    }

    pub fn record_flush(&self, total_events: u64, unique_ips: u64, subnet_counts: &[u64]) {
        self.events_last_second
            .store(total_events, Ordering::Relaxed);
        self.unique_ips_window.store(unique_ips, Ordering::Relaxed);
        // Snapshot semantics: this flush's counts fully replace the previous
        // window. Only zero the slots BEYOND the incoming range — slots
        // inside the range are about to be overwritten with the new value
        // anyway. Saves ~256 atomic stores on every flush when the
        // subnet count is well under 256.
        let n = subnet_counts.len().min(256);
        for slot in &self.subnet_window[n..] {
            slot.store(0, Ordering::Relaxed);
        }
        for (i, count) in subnet_counts.iter().take(256).enumerate() {
            self.subnet_window[i].store(*count, Ordering::Relaxed);
        }
    }

    /// Push multiple threat samples into the queue.
    pub fn push_threat_samples(&self, samples: Vec<(IpAddr, f32)>) {
        for item in samples {
            self.threat_sample.push(item);
        }
    }

    /// Atomically drain the threat sample queue.
    pub fn drain_threat_sample(&self) -> Vec<(IpAddr, f32)> {
        let mut sample = Vec::with_capacity(self.threat_sample.len());
        while let Some(item) = self.threat_sample.pop() {
            sample.push(item);
        }
        sample
    }
}

impl Default for TrafficCounters {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    Counter(u64),
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

    pub fn is_blocked(&self) -> bool {
        matches!(self, Value::IpRecord(rec) if rec.block_state != BlockState::Clean)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpRecord {
    pub ip: IpAddr,
    pub request_count: u64,
    pub ewma_rps: f64,
    /// P1 CUSUM accumulator (Page 1954) — sustained sub-threshold drift.
    #[serde(default)]
    pub cusum_s: f64,
    /// Slow-EWMA baseline the CUSUM measures deviation from.
    #[serde(default)]
    pub baseline_rps: f64,
    /// Debounce latch: previous sample was over threshold.
    #[serde(default)]
    pub prev_sample_hot: bool,
    /// Flush samples observed (saturating) — gates CUSUM warm-up. u8 is plenty:
    /// 6 samples to arm, saturates long before overflow matters.
    #[serde(default)]
    pub sample_count: u8,
    pub first_seen_ns: u64,
    pub last_seen_ns: u64,
    pub bytes_in: u64,
    pub status_dist: [u32; 5],
    pub proto_fingerprint: u32,
    pub threat_score: f32,
    pub block_state: BlockState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BlockState {
    Clean,
    Suspicious,
    Blocked { reason: BlockReason, since_ns: u64 },
}

impl BlockState {
    /// Returns the since_ns timestamp if blocked, 0 otherwise.
    pub fn since_ns(&self) -> u64 {
        match self {
            BlockState::Blocked { since_ns, .. } => *since_ns,
            _ => 0,
        }
    }
}

/// Subnet aggregate. `prefix` metadata carried by `IpNetwork` (v4 /24, v6 /64);
/// `prefix_octets` kept for dashboard display of v4 subnets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubnetRecord {
    pub prefix: [u8; 3],
    pub total_rps: u64,
    /// Distinct-source signal for the current window (v4 only): 256-bit map of
    /// seen host octets, 32 B flat. The real swarm signal — one abuser at 500
    /// events is a single offender; 40 distinct IPs × 12 events is an attack.
    /// v6 /64s are too large to bitmap; they report `unique_ips == 0` and rely
    /// on per-IP EWMA + volume gates.
    /// ponytail: 32B/24-bit-host ceiling; swap for HLL when v6 swarm detection
    /// is actually needed.
    pub host_bitmap: [u64; 4],
    pub last_updated_ns: u64,
}

impl SubnetRecord {
    #[inline]
    pub fn unique_ips(&self) -> u64 {
        self.host_bitmap.iter().map(|w| w.count_ones() as u64).sum()
    }

    #[inline]
    fn mark_host_v4(&mut self, ip: std::net::IpAddr) {
        if let std::net::IpAddr::V4(v4) = ip {
            let o = v4.octets()[3] as usize;
            self.host_bitmap[o / 64] |= 1 << (o % 64);
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub value: Value,
    pub expires_at: Option<Instant>,
}

impl Entry {
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|e| Instant::now() > e)
    }
}

/// Subnet key: IPv4 packed into low 32 bits, IPv6 full 128 bits.
pub type SubnetKey = u128;
type SubnetTable = DashMap<SubnetKey, SubnetRecord>;

pub struct Store {
    inner: Arc<DashMap<IpAddr, Entry>>,
    subnet_table: Arc<SubnetTable>,
    /// Reverse index: subnet key -> list of IPs for efficient subnet-based lookups.
    /// Maintained during batch flush to avoid O(store_size) scans.
    subnet_index: Arc<DashMap<SubnetKey, DashSet<IpAddr>>>,
    ram_bytes: Arc<AtomicUsize>,
    /// O(1) blocked count — updated on BlockState transitions in insert().
    /// ponytail: does not track pre-existing blocked IPs from WAL replay unless
    /// replay calls insert() with a Blocked state (it does). add scan at boot
    /// if needed.
    blocked_count: Arc<AtomicU64>,
    pub traffic: Arc<TrafficCounters>,
    pub total_inserts: Arc<AtomicU64>,
    pub total_evictions: Arc<AtomicU64>,
}

/// Minimal single-value set over DashMap (avoids pulling dashmap-set feature).
type DashSet<T> = DashMap<T, ()>;

impl Store {
    pub fn new(shard_count: usize) -> Self {
        let shards = shard_count.next_power_of_two();
        tracing::debug!("Store::new - Initializing store with {} shards", shards);
        Self {
            inner: Arc::new(DashMap::with_shard_amount(shards)),
            subnet_table: Arc::new(DashMap::with_shard_amount(32)),
            subnet_index: Arc::new(DashMap::with_shard_amount(32)),
            ram_bytes: Arc::new(AtomicUsize::new(0)),
            blocked_count: Arc::new(AtomicU64::new(0)),
            traffic: Arc::new(TrafficCounters::new()),
            total_inserts: Arc::new(AtomicU64::new(0)),
            total_evictions: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Merge subnet-scale counters from a batch flush (O(subnets in batch)).
    /// Windowed: entries older than `window_ns` reset before adding, so a /24
    /// can't accumulate across windows and false-positive the batch blocker.
    pub fn merge_subnet_window(
        &self,
        key: SubnetKey,
        net: IpNetwork,
        events: u32,
        members: Option<&[std::net::IpAddr]>,
        now_ns: u64,
    ) {
        const WINDOW_NS: u64 = 2 * 1_000_000_000; // ponytail: fixed 2s window vs config plumbing — matches pre_aggs flush cadence; add per-subnet window cfg when justified.
        let prefix = net.prefix_octets();
        let mut rec = self
            .subnet_table
            .get(&key)
            .map(|e| e.value().clone())
            .unwrap_or(SubnetRecord {
                prefix,
                total_rps: 0,
                host_bitmap: [0; 4],
                last_updated_ns: now_ns,
            });
        if now_ns.saturating_sub(rec.last_updated_ns) > WINDOW_NS {
            // Window rollover: reset counters + bitmap so a quiet subnet
            // de-arms naturally instead of carrying stale swarm signals.
            rec.total_rps = 0;
            rec.host_bitmap = [0; 4];
        }
        rec.total_rps = rec.total_rps.saturating_add(events as u64);
        if let Some(members) = members {
            for ip in members {
                rec.mark_host_v4(*ip);
            }
        }
        rec.last_updated_ns = now_ns;
        self.subnet_table.insert(key, rec);
    }

    pub fn reset_subnet_window(&self, key: SubnetKey) {
        if let Some(mut e) = self.subnet_table.get_mut(&key) {
            e.total_rps = 0;
            e.host_bitmap = [0; 4];
        }
    }

    /// Insert with RAM limit enforcement. Only enforces limit on net-new growth,
    /// allowing replacement of existing entries without triggering capacity errors.
    /// Capacity semantics ported from crate atomic_insert: byte-accounted via
    /// heap_bytes delta, errors as Result (never panic/log-and-continue).
    pub fn insert(
        &self,
        key: IpAddr,
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
        // Snapshot old blocked state before insert for O(1) blocked_count tracking.
        let was_blocked = self
            .inner
            .get(&key)
            .is_some_and(|e| e.value().value.is_blocked());
        let new_entry = Entry { value, expires_at };
        let new_blocked = new_entry.value.is_blocked();
        let entry_size = std::mem::size_of::<IpAddr>()
            + std::mem::size_of::<Entry>()
            + new_entry.value.heap_bytes();

        // Insert first, then check adjusted budget (replacement is free)
        let old_size = self.inner.insert(key, new_entry).map_or(0, |old| {
            std::mem::size_of::<Entry>() + old.value.heap_bytes() + std::mem::size_of::<IpAddr>()
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
        self.traffic
            .used_bytes
            .fetch_add(net_growth as u64, Ordering::Relaxed);
        self.total_inserts.fetch_add(1, Ordering::Relaxed);
        // O(1) blocked_count tracking — only mutate on transition.
        if !was_blocked && new_blocked {
            self.blocked_count.fetch_add(1, Ordering::Relaxed);
        } else if was_blocked && !new_blocked {
            self.blocked_count.fetch_sub(1, Ordering::Relaxed);
        }
        tracing::debug!("Store::insert - Successfully inserted key: {}", key);
        Ok(())
    }

    pub fn get(&self, key: &IpAddr) -> Option<Value> {
        let entry = self.inner.get(key)?;
        if entry.is_expired() {
            return None;
        }
        Some(entry.value.clone())
    }

    pub fn evict_batch(&self, keys: &[IpAddr]) {
        // ponytail: `entry()` gives exclusive shard lock once per key
        // (no separate get + remove = 1 lock acquisition instead of 2).
        for key in keys {
            if let dashmap::Entry::Occupied(e) = self.inner.entry(*key)
                && e.get().is_expired()
            {
                let was_blocked = e.get().value.is_blocked();
                let (_, removed) = e.remove_entry();
                let freed = std::mem::size_of::<IpAddr>()
                    + std::mem::size_of::<Entry>()
                    + removed.value.heap_bytes();
                self.ram_bytes.fetch_sub(freed, Ordering::Relaxed);
                self.traffic
                    .used_bytes
                    .fetch_sub(freed as u64, Ordering::Relaxed);
                self.total_evictions.fetch_add(1, Ordering::Relaxed);
                if was_blocked {
                    self.blocked_count.fetch_sub(1, Ordering::Relaxed);
                }
            }
        }
    }

    pub fn remove(&self, key: &IpAddr) -> Option<Value> {
        self.inner.remove(key).map(|(_k, e)| {
            let freed =
                std::mem::size_of::<IpAddr>() + std::mem::size_of::<Entry>() + e.value.heap_bytes();
            self.ram_bytes.fetch_sub(freed, Ordering::Relaxed);
            self.traffic
                .used_bytes
                .fetch_sub(freed as u64, Ordering::Relaxed);
            self.total_evictions.fetch_add(1, Ordering::Relaxed);
            if e.value.is_blocked() {
                self.blocked_count.fetch_sub(1, Ordering::Relaxed);
            }
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

    #[doc(hidden)]
    pub fn set_ram_limit_mb_for_testing(&self, mb: usize) {
        self.traffic.ram_limit_mb.store(mb, Ordering::Relaxed);
    }

    #[doc(hidden)]
    pub fn set_ram_bytes_for_testing(&self, bytes: usize) {
        self.ram_bytes.store(bytes, Ordering::Relaxed);
    }
    pub fn inner(&self) -> &DashMap<IpAddr, Entry> {
        &self.inner
    }
    pub fn subnet_table(&self) -> &SubnetTable {
        &self.subnet_table
    }
    /// Returns aggregate store statistics for dashboard / CLI.
    pub fn get_stats(&self) -> StoreStats {
        let ips_tracked = self.inner.len();
        // ponytail: O(1) via atomic counter — no O(store) scan.
        let blocked = self.blocked_count.load(Ordering::Relaxed);
        let ram_bytes = self.ram_bytes.load(Ordering::Relaxed);
        let ram_limit_mb = self.traffic.ram_limit_mb.load(Ordering::Relaxed);
        let uptime_secs = self.traffic.uptime_secs.load(Ordering::Relaxed);
        let evictions = self.total_evictions.load(Ordering::Relaxed);
        StoreStats {
            ips_tracked,
            blocked,
            ram_bytes,
            ram_limit_mb,
            uptime_secs,
            evictions,
        }
    }

    /// Update the reverse index for subnet lookups. Call after inserting/updating an IP record.
    pub fn update_subnet_index(
        &self,
        ip_key: IpAddr,
        subnet_key: Option<SubnetKey>,
        is_removal: bool,
    ) {
        let Some(sk) = subnet_key else { return };

        if is_removal {
            if let Some(subnet_ips) = self.subnet_index.get(&sk) {
                subnet_ips.remove(&ip_key);
                if subnet_ips.is_empty() {
                    self.subnet_index.remove(&sk);
                }
            }
        } else {
            self.subnet_index
                .entry(sk)
                .or_insert_with(|| DashMap::with_capacity(64))
                .insert(ip_key, ());
        }
    }

    /// Get all IP keys in a given subnet using the reverse index (O(1) lookup).
    pub fn get_ips_in_subnet(&self, subnet_key: SubnetKey) -> Vec<IpAddr> {
        self.subnet_index
            .get(&subnet_key)
            .map(|ips| ips.iter().map(|e| *e.key()).collect())
            .unwrap_or_default()
    }

    /// Get all currently blocked IPs for XDP reconciliation.
    pub fn get_all_blocked_ips(&self) -> Vec<IpAddr> {
        self.inner
            .iter()
            .filter(|e| e.value().value.is_blocked())
            .map(|e| *e.key())
            .collect()
    }
}

#[derive(Debug)]
pub struct StoreStats {
    pub ips_tracked: usize,
    pub blocked: u64,
    pub ram_bytes: usize,
    pub ram_limit_mb: usize,
    pub uptime_secs: u64,
    pub evictions: u64,
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
            .insert(
                "127.0.0.1".parse().unwrap(),
                Value::Counter(1),
                None,
                64 * 1024 * 1024,
            )
            .unwrap();
        assert!(store.get(&"127.0.0.1".parse().unwrap()).is_some());
        store.remove(&"127.0.0.1".parse().unwrap());
        assert!(store.get(&"127.0.0.1".parse().unwrap()).is_none());
    }

    #[test]
    fn ttl_lazy_expiry() {
        let store = Store::new(16);
        store
            .insert(
                "127.0.0.3".parse().unwrap(),
                Value::Counter(1),
                Some(0),
                64 * 1024 * 1024,
            )
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(store.get(&"127.0.0.3".parse().unwrap()).is_none());
    }

    #[test]
    fn capacity_enforced_on_growth_only() {
        let store = Store::new(16);
        let limit = 1024; // tiny
        store
            .insert("10.0.0.1".parse().unwrap(), Value::Counter(1), None, limit)
            .unwrap();
        // Replacement never trips capacity
        store
            .insert("10.0.0.1".parse().unwrap(), Value::Counter(2), None, limit)
            .unwrap();
        // Net-new beyond limit fails
        let err = store
            .insert(
                "10.0.0.2".parse().unwrap(),
                Value::Blob(vec![0u8; 4096]),
                None,
                limit,
            )
            .unwrap_err();
        assert!(matches!(err, RsError::CapacityExceeded { .. }));
        assert!(store.get(&"10.0.0.2".parse().unwrap()).is_none());
    }

    #[test]
    fn subnet_window_v6_key() {
        let store = Store::new(16);
        let v6: IpAddr = "2001:db8::1".parse().unwrap();
        let key = subnet::subnet_key_u128(v6).unwrap();
        let net = IpNetwork::of_ip(v6);
        store.merge_subnet_window(key, net, 5, Some(&[v6]), 1_000_000_000);
        assert_eq!(store.subnet_table().get(&key).unwrap().total_rps, 5);
        store.reset_subnet_window(key);
        assert_eq!(store.subnet_table().get(&key).unwrap().total_rps, 0);
    }
}
