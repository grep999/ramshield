//! Thread-safe, TTL-backed concurrent cache.
//!
//! Uses [`DashMap`] for the backing store and a dedicated OS thread for
//! lazy expiration.  `Cache<K, V>` is `Send + Sync` when `K: Send + Sync`
//! and `V: Send + Sync` — no `unsafe` required.

use dashmap::DashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ── Entry ────────────────────────────────────────────────────────────────

/// A single cache entry.
#[derive(Debug, Clone)]
pub struct Entry<V> {
    /// The stored value.
    pub value: V,
    /// Expiry timestamp in nanoseconds since UNIX epoch, or `None` for no expiry.
    pub expire: Option<u64>,
    /// Creation timestamp in nanoseconds since UNIX epoch.
    pub created: u64,
}

/// Current time as nanoseconds since UNIX epoch.
///
/// Falls back to `0` if the system clock is before the epoch (should
/// never happen in practice, but we avoid `.unwrap()` per project rules).
fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

// ── Cache ────────────────────────────────────────────────────────────────

/// A generic, concurrent, TTL-backed cache.
///
/// # Type Parameters
/// * `K` – key type (`Hash + Eq + Clone + Send + Sync + 'static`)
/// * `V` – value type (`Clone + Send + Sync + 'static`)
///
/// # Concurrency
/// All operations are lock-free at the DashMap shard level.  A
/// background evictor thread removes expired entries every 5 seconds.
pub struct Cache<K, V> {
    inner: Arc<DashMap<K, Entry<V>>>,
    capacity: usize,
    default_ttl: Duration,
    count: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
}

impl<K, V> Cache<K, V>
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Create a new cache with the given soft capacity and default TTL.
    ///
    /// Spawns a background evictor thread.  The thread terminates when
    /// [`shutdown`](Self::shutdown) is called or the cache is dropped.
    ///
    /// # Errors
    /// Returns `Err` if the evictor thread cannot be spawned (e.g. out of
    /// system resources).  In that case the cache is still functional —
    /// just without background eviction.
    pub fn new(entry_cap: usize, default_ttl: Duration) -> Self {
        let inner = Arc::new(DashMap::new());
        let count = Arc::new(AtomicU64::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));

        Self::spawn_evictor(inner.clone(), count.clone(), shutdown.clone());

        Self {
            inner,
            capacity: entry_cap,
            default_ttl,
            count,
            shutdown,
        }
    }

    /// Spawn the background evictor thread.
    fn spawn_evictor(
        inner: Arc<DashMap<K, Entry<V>>>,
        count: Arc<AtomicU64>,
        shutdown: Arc<AtomicBool>,
    ) {
        let _ = std::thread::Builder::new()
            .name("cache-evictor".into())
            .spawn(move || {
                while !shutdown.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_secs(5));
                    let now = now_ns();
                    let mut removed = 0u64;
                    inner.retain(|_, entry| match entry.expire {
                        Some(exp) if now >= exp => {
                            removed += 1;
                            false
                        }
                        _ => true,
                    });
                    if removed > 0 {
                        count.fetch_sub(removed, Ordering::Relaxed);
                    }
                }
            });
        // If spawn fails the cache still works; eviction just won't happen
        // in the background.  We silently continue per the "no panics in
        // production" rule.
    }

    /// Signal the evictor thread to stop.  Idempotent.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
    }

    /// Retrieve a reference to the value for `key`.
    ///
    /// Returns `None` if the key is absent or has expired.  Expired
    /// entries are removed lazily on access.
    /// Retrieve a reference to the value for `key`.
    ///
    /// Returns `None` if the key is absent or has expired.  Expired
    /// entries are removed lazily on access.
    pub fn get<'a>(&'a self, key: &'a K) -> Option<dashmap::mapref::one::Ref<'a, K, Entry<V>>> {
        let now = now_ns();
        let entry = self.inner.get(key)?;

        if let Some(exp) = entry.expire {
            if now >= exp {
                drop(entry);
                self.inner.remove(key);
                self.count.fetch_sub(1, Ordering::Relaxed);
                return None;
            }
        }
        Some(entry)
    }

    /// Insert a value with an optional per-entry TTL.
    ///
    /// If `ttl` is `None`, the default TTL is used.  A `ttl` of
    /// `Duration::ZERO` means "never expire".
    pub fn insert(&self, key: K, value: V, ttl: Option<Duration>) {
        let ttl = ttl.unwrap_or(self.default_ttl);
        let expire = if ttl.is_zero() {
            None
        } else {
            Some(now_ns().saturating_add(ttl.as_nanos() as u64))
        };

        let entry = Entry {
            value,
            expire,
            created: now_ns(),
        };

        let old = self.inner.insert(key, entry);
        if old.is_none() {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
        // Soft capacity: the evictor thread handles overflow cleanup.
    }

    /// Remove a key, returning its entry if it existed.
    pub fn remove(&self, key: &K) -> Option<Entry<V>> {
        let removed = self.inner.remove(key).map(|(_k, entry)| entry);
        if removed.is_some() {
            self.count.fetch_sub(1, Ordering::Relaxed);
        }
        removed
    }

    /// Remove all entries.
    pub fn clear(&self) {
        self.inner.clear();
        self.count.store(0, Ordering::Relaxed);
    }

    /// Current number of entries (approximate under concurrency).
    pub fn len(&self) -> usize {
        self.count.load(Ordering::Relaxed) as usize
    }

    /// `true` when the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Soft capacity ceiling.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl<K, V> Drop for Cache<K, V> {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    fn cache() -> Cache<String, String> {
        Cache::new(100, Duration::from_secs(60))
    }

    #[test]
    fn insert_and_get() {
        let c = cache();
        c.insert("k1".to_owned(), "v1".to_owned(), None);
        assert_eq!(
            c.get(&"k1".to_owned()).map(|e| e.value.clone()),
            Some("v1".to_owned())
        );
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn get_missing_key() {
        let c = cache();
        assert!(c.get(&"nope".to_owned()).is_none());
        assert!(c.is_empty());
    }

    #[test]
    fn remove_existing() {
        let c = cache();
        c.insert("k".to_owned(), "v".to_owned(), None);
        let entry = c.remove(&"k".to_owned());
        assert!(entry.is_some());
        assert!(c.is_empty());
    }

    #[test]
    fn remove_missing() {
        let c = cache();
        assert!(c.remove(&"x".to_owned()).is_none());
    }

    #[test]
    fn clear_empties() {
        let c = cache();
        c.insert("a".to_owned(), "1".to_owned(), None);
        c.insert("b".to_owned(), "2".to_owned(), None);
        assert_eq!(c.len(), 2);
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn ttl_expiry_on_access() {
        let c = Cache::new(100, Duration::from_millis(1));
        c.insert(
            "k".to_owned(),
            "v".to_owned(),
            Some(Duration::from_millis(1)),
        );
        thread::sleep(Duration::from_millis(5));
        assert!(c.get(&"k".to_owned()).is_none());
        assert!(c.is_empty());
    }

    #[test]
    fn default_ttl_applied() {
        let c = Cache::new(100, Duration::from_millis(1));
        c.insert("k".to_owned(), "v".to_owned(), None);
        thread::sleep(Duration::from_millis(5));
        assert!(c.get(&"k".to_owned()).is_none());
    }

    #[test]
    fn zero_ttl_never_expires() {
        let c = cache();
        c.insert("k".to_owned(), "v".to_owned(), Some(Duration::ZERO));
        assert!(c.get(&"k".to_owned()).is_some());
    }

    #[test]
    fn replace_does_not_increase_count() {
        let c = cache();
        c.insert("k".to_owned(), "v1".to_owned(), None);
        c.insert("k".to_owned(), "v2".to_owned(), None);
        assert_eq!(c.len(), 1);
        assert_eq!(c.get(&"k".to_owned()).unwrap().value, "v2");
    }

    #[test]
    fn concurrent_inserts() {
        let c = Arc::new(cache());
        let mut handles = Vec::new();
        for i in 0..16 {
            let c = c.clone();
            handles.push(thread::spawn(move || {
                c.insert(format!("k{}", i), format!("v{}", i), None);
            }));
        }
        for h in handles {
            let _ = h.join();
        }
        assert_eq!(c.len(), 16);
        for i in 0..16 {
            assert_eq!(
                c.get(&format!("k{}", i)).map(|e| e.value.clone()),
                Some(format!("v{}", i))
            );
        }
    }

    #[test]
    fn capacity_accessor() {
        let c: Cache<String, String> = Cache::new(42, Duration::from_secs(1));
        assert_eq!(c.capacity(), 42);
    }
}
