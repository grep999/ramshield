//! Per-key nonce store with TTL eviction and LRU bounding.
//!
//! Used by `auth::verify` (and the IPC server) to reject replays of a
//! previously-seen frame within the clock-skew window. A "nonce" here is the
//! `HMAC-SHA256(key, ts_ms || payload)` digest the verifier just computed —
//! this is checked separately from the signature scheme, so we don't have to
//! rewire the existing on-the-wire format and old clients keep working.
//! `check_and_record` is the only mutating call: it records on first sight
//! and returns `Err` on collision. Eviction is lazy (on access) by TTL and
//! hard by LRU capacity.

use std::collections::VecDeque;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ahash::AHashMap;

/// Composite key: `(key_id_hash, nonce_bytes)`. Length-prefixed so
/// `"a\0bc"` and `"ab\0c"` don't collide even if `key_id` is empty.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NonceKey {
    pub key_id: String,
    pub nonce: Vec<u8>,
}

/// Per-key nonce store. Bounded by LRU capacity; entries expire after `ttl`.
pub struct ReplayStore {
    cap: usize,
    per_key_cap: usize,
    ttl: Duration,

    // Map key -> insert instant. Insertion order = LRU order (front = oldest).
    inner: Mutex<StoreInner>,
}

struct StoreInner {
    order: VecDeque<NonceKey>,
    map: AHashMap<NonceKey, Instant>,
}

impl ReplayStore {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self::with_per_key_cap(capacity, capacity, ttl)
    }

    /// Construct with per-key capacity limit. `per_key_cap` bounds how many
    /// entries one key_id can occupy before its own oldest are evicted.
    /// The global LRU `capacity` still applies as an outer bound.
    pub fn with_per_key_cap(capacity: usize, per_key_cap: usize, ttl: Duration) -> Self {
        Self {
            cap: capacity.max(1),
            per_key_cap: per_key_cap.max(1),
            ttl,
            inner: Mutex::new(StoreInner {
                order: VecDeque::with_capacity(capacity),
                map: AHashMap::with_capacity(capacity),
            }),
        }
    }

    /// Current entry count (test/diagnostic only).
    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.order.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Record a freshly-observed nonce under `key_id`. Returns
    /// `Err("replay")` if the same `(key_id, nonce)` was seen within `ttl`,
    /// `Ok(())` otherwise. Evicts expired entries and overflows the LRU.
    /// Per-key capacity rejection without evicting unexpired entries (B06).
    pub fn check_and_record(&self, key_id: &str, nonce: &[u8]) -> Result<(), &'static str> {
        // B06: Ensure full identity and atomic check-and-record, not lossy hash.
        // Use the actual key_id string to preserve identity.
        let key = NonceKey {
            key_id: key_id.to_string(),
            nonce: nonce.to_vec(),
        };
        let now = Instant::now();
        let mut g = self.inner.lock().map_err(|_| "replay store poisoned")?;
        // Lazy TTL sweep on the front (oldest). A full sweep is O(n) and
        // not needed — old entries fall out of the LRU window anyway.
        while g
            .order
            .front()
            .and_then(|k| g.map.get(k))
            .is_some_and(|ts| now.duration_since(*ts) >= self.ttl)
        {
            if let Some(expired) = g.order.pop_front() {
                g.map.remove(&expired);
            }
        }
        // Reject only if unexpired duplicate already exists.
        if g.map
            .get(&key)
            .is_some_and(|prev| now.duration_since(*prev) < self.ttl)
        {
            return Err("replay");
        }
        // B06: Check per-key capacity BEFORE mutating. The store must never
        // change state when it returns an error — a rejected request leaves
        // the store exactly as it was.
        if self.per_key_cap < self.cap {
            let own_count = g
                .order
                .iter()
                .filter(|k| k.key_id == key.key_id)
                .filter(|k| {
                    g.map
                        .get(*k)
                        .is_some_and(|ts| now.duration_since(*ts) < self.ttl)
                })
                .count();
            if own_count >= self.per_key_cap {
                return Err("capacity");
            }
        }
        // Insert / refresh.
        g.order.retain(|k| k != &key);
        g.map.insert(key.clone(), now);
        g.order.push_back(key.clone());
        // LRU bound: drop the oldest ONLY if exceeds global capacity.
        while g.order.len() > self.cap {
            if let Some(old) = g.order.pop_front() {
                g.map.remove(&old);
            }
        }
        Ok(())
    }
}

impl Default for ReplayStore {
    /// Small store (cap 10, ttl 1s) for tests. Real callers construct via
    /// `new`/`with_per_key_cap` with configured values.
    fn default() -> Self {
        Self::new(10, Duration::from_secs(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn per_key_limit_prevents_flood_across_keys() {
        let s = ReplayStore::with_per_key_cap(64, 2, Duration::from_millis(100000));
        // Fill one key with 2 entries — should be ok
        assert!(s.check_and_record("kA", &[1; 32]).is_ok());
        assert!(s.check_and_record("kA", &[2; 32]).is_ok());
        // Third entry for same key should fail due to per-key capacity (no eviction)
        assert!(s.check_and_record("kA", &[3; 32]).is_err());
        // kB entries should still be accepted (global cap not hit)
        assert!(s.check_and_record("kB", &[4; 32]).is_ok());
    }

    #[test]
    fn first_seen_ok_then_replay_rejected() {
        let s = ReplayStore::new(16, Duration::from_millis(100));
        let sig: &[u8] = &[1u8; 32];
        assert!(s.check_and_record("k1", sig).is_ok());
        assert!(s.check_and_record("k1", sig).is_err());
    }

    #[test]
    fn ttl_lets_same_sig_through_again() {
        let s = ReplayStore::new(16, Duration::from_millis(30));
        let sig: &[u8] = &[2u8; 32];
        assert!(s.check_and_record("k1", sig).is_ok());
        assert!(s.check_and_record("k1", sig).is_err());
        thread::sleep(Duration::from_millis(50));
        assert!(s.check_and_record("k1", sig).is_ok());
    }

    #[test]
    fn lru_bound_holds() {
        let s = ReplayStore::new(4, Duration::from_secs(60));
        for i in 0u8..16 {
            let n = format!("n{}", i);
            s.check_and_record("k1", n.as_bytes()).unwrap();
        }
        assert!(s.len() <= 4);
    }
}
