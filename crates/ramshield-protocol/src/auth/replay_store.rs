//! Per-key nonce store with TTL eviction and bounded capacity.
//!
//! Used by `auth::verify` to reject replays of a previously-seen frame within
//! the clock-skew window. A nonce is the HMAC-SHA256 digest computed by the
//! verifier. Expired entries are removed lazily; live entries are never
//! evicted to make room for a new nonce. Saturation rejects new entries so
//! capacity pressure cannot reopen a replay window.

use std::collections::VecDeque;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ahash::AHashMap;

/// Composite key: (key_id_hash, nonce_bytes).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct NonceKey {
    pub key_id_hash: u64,
    pub nonce: Vec<u8>,
}

/// Replay store bounded by a global cap, a per-key cap, and an entry TTL.
pub struct ReplayStore {
    cap: usize,
    per_key_cap: usize,
    ttl: Duration,
    /// Process-random hash builder for key_id -> u64.
    key_hasher: ahash::RandomState,
    // Insertion order (front = oldest). All timestamps are sampled under lock.
    inner: Mutex<StoreInner>,
}

struct StoreInner {
    order: VecDeque<NonceKey>,
    map: AHashMap<NonceKey, Instant>,
}

impl ReplayStore {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        let capacity = capacity.max(1);
        Self {
            cap: capacity,
            per_key_cap: capacity,
            ttl,
            key_hasher: ahash::RandomState::new(),
            inner: Mutex::new(StoreInner {
                order: VecDeque::with_capacity(capacity),
                map: AHashMap::with_capacity(capacity),
            }),
        }
    }

    /// Construct with per-key capacity limit. The global capacity remains an
    /// outer bound. Saturation rejects new nonces rather than evicting live
    /// replay markers.
    pub fn with_per_key_cap(capacity: usize, per_key_cap: usize, ttl: Duration) -> Self {
        let capacity = capacity.max(1);
        Self {
            cap: capacity,
            per_key_cap: per_key_cap.max(1).min(capacity),
            ttl,
            key_hasher: ahash::RandomState::new(),
            inner: Mutex::new(StoreInner {
                order: VecDeque::with_capacity(capacity),
                map: AHashMap::with_capacity(capacity),
            }),
        }
    }

    /// Record a fresh nonce. Returns `Err("replay")` for a live duplicate,
    /// `Err("capacity")` when a global or per-key limit is reached, and
    /// `Ok(())` otherwise. Expired entries are removed lazily.
    pub fn check_and_record(&self, key_id: &str, nonce: &[u8]) -> Result<(), &'static str> {
        let key = NonceKey {
            key_id_hash: self.key_hasher.hash_one(key_id),
            nonce: nonce.to_vec(),
        };
        let mut g = self.inner.lock().map_err(|_| "replay store poisoned")?;
        let now = Instant::now();

        // Since timestamps are sampled under this mutex and insertion order is
        // chronological, expired entries can be swept from the front.
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

        if g.map
            .get(&key)
            .is_some_and(|prev| now.duration_since(*prev) < self.ttl)
        {
            return Err("replay");
        }

        let own_key_count = g
            .order
            .iter()
            .filter(|existing| existing.key_id_hash == key.key_id_hash)
            .count();
        if g.map.len() >= self.cap || own_key_count >= self.per_key_cap {
            return Err("capacity");
        }

        g.map.insert(key.clone(), now);
        g.order.push_back(key);
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

impl Default for ReplayStore {
    fn default() -> Self {
        Self::new(10, Duration::from_secs(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn per_key_limit_rejects_new_nonce_without_evicting_live_markers() {
        let s = ReplayStore::with_per_key_cap(64, 2, Duration::from_secs(60));
        assert!(s.check_and_record("kA", &[1; 32]).is_ok());
        assert!(s.check_and_record("kA", &[2; 32]).is_ok());
        assert_eq!(s.check_and_record("kA", &[3; 32]), Err("capacity"));
        assert_eq!(s.check_and_record("kA", &[1; 32]), Err("replay"));
        assert_eq!(s.check_and_record("kB", &[4; 32]), Ok(()));
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn first_seen_ok_then_replay_rejected() {
        let s = ReplayStore::new(16, Duration::from_millis(100));
        let sig: &[u8] = &[1u8; 32];
        assert!(s.check_and_record("k1", sig).is_ok());
        assert_eq!(s.check_and_record("k1", sig), Err("replay"));
    }

    #[test]
    fn ttl_lets_same_sig_through_again() {
        let s = ReplayStore::new(16, Duration::from_millis(30));
        let sig: &[u8] = &[2u8; 32];
        assert!(s.check_and_record("k1", sig).is_ok());
        assert_eq!(s.check_and_record("k1", sig), Err("replay"));
        thread::sleep(Duration::from_millis(50));
        assert!(s.check_and_record("k1", sig).is_ok());
    }

    #[test]
    fn global_limit_rejects_new_nonce_and_preserves_old_markers() {
        let s = ReplayStore::new(2, Duration::from_secs(60));
        assert!(s.check_and_record("k1", b"one").is_ok());
        assert!(s.check_and_record("k2", b"two").is_ok());
        assert_eq!(s.check_and_record("k3", b"three"), Err("capacity"));
        assert_eq!(s.check_and_record("k1", b"one"), Err("replay"));
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn poisoned_store_fails_closed() {
        let s = std::sync::Arc::new(ReplayStore::new(4, Duration::from_secs(60)));
        let worker = s.clone();
        let _ = thread::spawn(move || {
            let _guard = worker.inner.lock().unwrap();
            panic!("poison store for test");
        })
        .join();
        assert_eq!(
            s.check_and_record("k1", b"nonce"),
            Err("replay store poisoned")
        );
    }
}
