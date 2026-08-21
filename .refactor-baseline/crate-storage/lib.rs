use anyhow::Result;
use crossbeam_queue::SegQueue;
use dashmap::DashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};

pub use ramshield_types::error::BlockReason;

pub mod atomic_ops;
pub mod blob_store;
pub mod ttl_wheel;
pub mod wal;

#[derive(Debug, Clone, Default)]
pub struct StoreStats {
    pub uptime_secs: u64,
    pub ips_tracked: usize,
    pub blocked: u64,
    pub ram_bytes: usize,
    pub ram_limit_mb: u64,
}

pub type SubnetTable = DashMap<u32, SubnetEntry>;

#[derive(Debug, Clone)]
pub struct SubnetEntry {
    pub prefix: [u8; 3],
    pub total_rps: u64,
}



#[derive(Debug, Clone)]
pub struct Entry {
    pub value: String,
    pub is_blocked: bool,
    pub reason: Option<BlockReason>,
}

pub struct TrafficCounters {
    pub ram_limit_mb: AtomicU64,
    pub used_bytes: AtomicU64,
    pub uptime_secs: AtomicU64,
    pub events_last_second: AtomicU64,
    pub unique_ips_window: AtomicU64,
    /// Lock-free atomic array for concurrent reads/writes from detection and forecasting.
    pub subnet_window: [AtomicU64; 256],
    /// Lock-free unbounded MPMC queue.
    pub threat_sample: SegQueue<(IpAddr, f32)>,
    pub blocked_count: AtomicU64,
}

impl TrafficCounters {
    pub fn new() -> Self {
        const Z: AtomicU64 = AtomicU64::new(0);
        Self {
            ram_limit_mb: AtomicU64::new(0),
            used_bytes: AtomicU64::new(0),
            uptime_secs: AtomicU64::new(0),
            events_last_second: AtomicU64::new(0),
            unique_ips_window: AtomicU64::new(0),
            subnet_window: [Z; 256],
            threat_sample: SegQueue::new(),
            blocked_count: AtomicU64::new(0),
        }
    }

    pub fn push_threat_samples(&self, samples: Vec<(IpAddr, f32)>) {
        for item in samples {
            self.threat_sample.push(item);
        }
    }

    pub fn drain_threat_sample(&self) -> Vec<(IpAddr, f32)> {
        let mut sample = Vec::with_capacity(self.threat_sample.len());
        while let Some(item) = self.threat_sample.pop() {
            sample.push(item);
        }
        sample
    }
}

pub struct Store {
    pub inner: DashMap<String, Entry>,
    pub traffic: TrafficCounters,
    pub subnet_table: SubnetTable,
}

impl Store {
    pub fn new(ram_limit: u64) -> Self {
        Self {
            inner: DashMap::new(),
            traffic: TrafficCounters {
                ram_limit_mb: AtomicU64::new(ram_limit),
                ..TrafficCounters::new()
            },
            subnet_table: SubnetTable::new(),
        }
    }

    pub fn atomic_insert(&self, key: String, value: Entry) -> Result<()> {
        atomic_ops::atomic_insert(self, key, value).map_err(|e| anyhow::anyhow!(e))
    }

    pub fn insert(&self, key: String, value: Entry) -> Result<()> {
        let entry_size = std::mem::size_of::<Entry>() as u64;
        let ram_limit = self.ram_bytes();
        let current_bytes = self.traffic.used_bytes.load(Ordering::Relaxed);
        if self.inner.get(&key).is_none() && current_bytes + entry_size > ram_limit {
            return Err(anyhow::anyhow!("CapacityExceeded"));
        }
        let was_occupied = self.inner.insert(key, value).is_some();
        if !was_occupied {
            self.traffic.used_bytes.fetch_add(entry_size, Ordering::Relaxed);
            self.traffic.blocked_count.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    pub fn remove(&self, key: &str) -> Option<(String, Entry)> {
        let removed = self.inner.remove(key);
        if let Some((_, _)) = &removed {
            let entry_size = std::mem::size_of::<Entry>() as u64;
            self.traffic.used_bytes.fetch_sub(entry_size, Ordering::Relaxed);
            self.traffic.blocked_count.fetch_sub(1, Ordering::Relaxed);
        }
        removed
    }

    pub fn get(&self, key: &str) -> Option<dashmap::mapref::one::Ref<'_, String, Entry>> {
        self.inner.get(key)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn iter(&self) -> dashmap::iter::Iter<'_, String, Entry> {
        self.inner.iter()
    }

    /// ponytail: no callers yet; implement when detection uses incrementing
    pub fn atomic_increment(&self, key: &str) -> Result<()> {
        let entry_size = std::mem::size_of::<Entry>() as u64;
        let ram_limit = self.ram_bytes();
        let mut map_entry = self.inner.entry(key.to_string());

        match map_entry {
            dashmap::mapref::entry::Entry::Occupied(ref mut o) => {
                let e = o.get_mut();
                // ponytail: Entry has no counter field yet; increment value length as proxy
                e.value.push(' ');
            }
            dashmap::mapref::entry::Entry::Vacant(v) => {
                let current_bytes = self.traffic.used_bytes.load(Ordering::Relaxed);
                if current_bytes + entry_size > ram_limit {
                    return Err(anyhow::anyhow!("CapacityExceeded"));
                }
                v.insert(Entry {
                    value: key.to_string(),
                    is_blocked: false,
                    reason: None,
                });
                self.traffic.used_bytes.fetch_add(entry_size, Ordering::Relaxed);
                self.traffic.blocked_count.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    pub fn evict_batch(&self, keys: &[String]) {
        let entry_size = std::mem::size_of::<Entry>() as u64;
        for key in keys {
            if self.inner.remove(key).is_some() {
                self.traffic.used_bytes.fetch_sub(entry_size, Ordering::Relaxed);
                self.traffic.blocked_count.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    pub fn ram_bytes(&self) -> u64 {
        self.traffic.ram_limit_mb.load(Ordering::Relaxed) * 1024 * 1024
    }

    pub fn get_stats(&self) -> StoreStats {
        StoreStats {
            uptime_secs: self.traffic.uptime_secs.load(Ordering::Relaxed),
            ips_tracked: self.inner.len(),
            blocked: self.traffic.blocked_count.load(Ordering::Relaxed),
            ram_bytes: self.traffic.used_bytes.load(Ordering::Relaxed) as usize,
            ram_limit_mb: self.traffic.ram_limit_mb.load(Ordering::Relaxed),
        }
    }

    /// Get all currently blocked IPs for XDP reconciliation.
    pub fn get_all_blocked_ips(&self) -> Vec<std::net::IpAddr> {
        self.inner
            .iter()
            .filter(|e| e.value().is_blocked)
            .filter_map(|e| e.key().parse().ok())
            .collect()
    }

    pub fn subnet_table(&self) -> &SubnetTable {
        &self.subnet_table
    }

    pub fn record_flush(&self, total_events: u64, unique_ips: u64, subnet_counts: &[u64]) {
        self.traffic.events_last_second
            .store(total_events, Ordering::Relaxed);
        self.traffic.unique_ips_window.store(unique_ips, Ordering::Relaxed);
        for (i, count) in subnet_counts.iter().enumerate() {
            if i < 256 {
                self.traffic.subnet_window[i].store(*count, Ordering::Relaxed);
            }
        }
    }

    pub fn set_threat_sample(&self, sample: Vec<(std::net::IpAddr, f32)>) {
        while self.traffic.threat_sample.pop().is_some() {}
        self.traffic.push_threat_samples(sample);
    }
}


