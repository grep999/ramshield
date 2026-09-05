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
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct NonceKey {
    pub key_id_hash: u64,
    pub nonce: Vec<u8>,
}

/// Per-key nonce store. Bounded by LRU capacity; entries expire after `ttl`.
pub struct ReplayStore {
    cap: usize,
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
        Self {
            cap: capacity.max(1),
            ttl,
            inner: Mutex::new(StoreInner {
                order: VecDeque::with_capacity(capacity),
                map: AHashMap::with_capacity(capacity),
            }),
        }
    }

    /// Record a freshly-observed nonce under `key_id`. Returns
    /// `Err("replay")` if the same `(key_id, nonce)` was seen within `ttl`,
    /// `Ok(())` otherwise. Evicts expired entries and overflows the LRU.
    pub fn check_and_record(&self, key_id: &str, nonce: &[u8]) -> Result<(), &'static str> {
        let key = NonceKey {
            key_id_hash: ahash::RandomState::with_seeds(0, 0, 0, 0).hash_one(key_id),
            nonce: nonce.to_vec(),
        };
        let now = Instant::now();
        let mut g = self.inner.lock().map_err(|_| "replay store poisoned")?;
        // Lazy TTL sweep on the front (oldest). A full sweep is O(n) and
        // not needed — old entries fall out of the LRU window anyway.
        while g.order
            .front()
            .and_then(|k| g.map.get(k))
            .map_or(false, |ts| now.duration_since(*ts) >= self.ttl)
        {
            if let Some(expired) = g.order.pop_front() {
                g.map.remove(&expired);
            }
        }
        if let Some(prev) = g.map.get(&key) {
            if now.duration_since(*prev) < self.ttl {
                return Err("replay");
            }
        }
        // Insert / refresh.
        g.order.retain(|k| k != &key);
        g.map.insert(key.clone(), now);
        g.order.push_back(key);
        // LRU bound: drop the oldest.
        while g.order.len() > self.cap {
            if let Some(old) = g.order.pop_front() {
                g.map.remove(&old);
            }
        }
        Ok(())
    }

    /// Current entry count (test/diagnostic only).
    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.order.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

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
