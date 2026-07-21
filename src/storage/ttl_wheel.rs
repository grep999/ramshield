use std::sync::{Arc, Mutex};
use tokio::time::{interval, Duration, MissedTickBehavior};
use tracing::debug;

use crate::storage::Store;

pub struct TtlWheel {
    slots: Vec<Arc<Mutex<Vec<String>>>>,
    size: usize,
    resolution_ms: u64,
    overflow: Arc<Mutex<Vec<Overflow>>>,
}

struct Overflow {
    key: String,
    expire_ms: u64,
}

impl TtlWheel {
    pub fn new(size: usize, resolution_ms: u64) -> Self {
        let slots = (0..size)
            .map(|_| Arc::new(Mutex::new(Vec::<String>::new())))
            .collect();
        Self {
            slots,
            size,
            resolution_ms,
            overflow: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn schedule(&self, key: String, ttl_ms: u64) {
        let now_ms = epoch_ms();
        let expire_ms = now_ms + ttl_ms;
        let max_range = self.size as u64 * self.resolution_ms;
        if ttl_ms >= max_range {
            self.overflow
                .lock()
                .unwrap()
                .push(Overflow { key, expire_ms });
            return;
        }
        let ticks = (ttl_ms / self.resolution_ms).max(1) as usize;
        let cursor = (now_ms / self.resolution_ms) as usize;
        let slot = (cursor + ticks) % self.size;
        self.slots[slot].lock().unwrap().push(key);
    }

    pub fn tick(&self) -> Vec<String> {
        let now_ms = epoch_ms();
        let cursor = (now_ms / self.resolution_ms) as usize;
        let slot = cursor % self.size;

        let expired = {
            let mut s = self.slots[slot].lock().unwrap();
            std::mem::take(&mut *s)
        };

        let window_end = now_ms + self.size as u64 * self.resolution_ms;
        let mut overflow = self.overflow.lock().unwrap();
        let mut keep = Vec::with_capacity(overflow.len());
        for entry in overflow.drain(..) {
            if entry.expire_ms <= window_end {
                let rem = entry.expire_ms.saturating_sub(now_ms);
                let ticks = (rem / self.resolution_ms).max(1) as usize;
                let target = (cursor + ticks) % self.size;
                self.slots[target].lock().unwrap().push(entry.key);
            } else {
                keep.push(entry);
            }
        }
        *overflow = keep;
        debug!("TTL tick slot {}: {} keys", slot, expired.len());
        expired
    }
}

fn epoch_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub async fn run_ttl_wheel(
    wheel: Arc<TtlWheel>,
    store: Arc<Store>,
    resolution_ms: u64,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) {
    let mut ticker = interval(Duration::from_millis(resolution_ms));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let keys = wheel.tick();
                if !keys.is_empty() {
                    let s = store.clone();
                    tokio::task::spawn_blocking(move || s.evict_batch(&keys));
                }
            }
            _ = shutdown.recv() => { debug!("TTL wheel stopped"); break; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_when_exceeds_window() {
        let wheel = TtlWheel::new(10, 100);
        wheel.schedule("key".into(), 5_000);
        assert_eq!(wheel.overflow.lock().unwrap().len(), 1);
    }
}
