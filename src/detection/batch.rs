use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone)]
pub struct ConnectionEvent {
    pub ip: String,
    pub timestamp_ns: u64,
    pub bytes: u64,
    pub status_code: u16,
    pub proto_fingerprint: u32,
}

pub struct Batch {
    pub events: Vec<ConnectionEvent>,
    pub event_count: std::sync::atomic::AtomicU64,
}

impl Batch {
    pub fn new() -> Self {
        Self {
            events: Vec::with_capacity(4096),
            event_count: AtomicU64::new(0),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn add_event(&mut self, event: ConnectionEvent) {
        self.events.push(event);
        self.event_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn is_full(&self, max_events: usize) -> bool {
        self.events.len() >= max_events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn clear(&mut self) {
        self.events.clear();
        self.event_count.store(0, Ordering::Relaxed);
    }
}

impl Default for Batch {
    fn default() -> Self {
        Self::new()
    }
}

pub struct BloomFilter {
    bits: Vec<u64>,
    size: usize,
}

impl BloomFilter {
    pub fn new(bit_count: usize) -> Self {
        let words = bit_count.div_ceil(64);
        Self { bits: vec![0u64; words], size: words * 64 }
    }

    pub fn add(&mut self, item: &str) {
        let hash = self.hash(item);
        let idx1 = (hash as usize) % self.size;
        let idx2 = ((hash >> 32) as usize) % self.size;

        self.bits[idx1 / 64] |= 1 << (idx1 % 64);
        self.bits[idx2 / 64] |= 1 << (idx2 % 64);
    }

    pub fn contains(&self, item: &str) -> bool {
        let hash = self.hash(item);
        let idx1 = (hash as usize) % self.size;
        let idx2 = ((hash >> 32) as usize) % self.size;

        let bit1 = (self.bits[idx1 / 64] >> (idx1 % 64)) & 1;
        let bit2 = (self.bits[idx2 / 64] >> (idx2 % 64)) & 1;

        bit1 == 1 && bit2 == 1
    }

    fn hash(&self, item: &str) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        item.hash(&mut hasher);
        hasher.finish()
    }
}
