//! Capacity-checked insert semantics over the unified Store.
//! Logic ported from the old crate Entry-based atomic_insert; operates on
//! `Value`-shaped entries. Primary insert path is `Store::insert` (which
//! already enforces capacity); this module exists for callers holding a
//! dashmap entry guard mid-update.

use crate::{Entry, Store};
use ramshield_types::Result;
use std::net::IpAddr;

/// Insert-or-replace with RAM accounting. Replacement is free (fixed-size
/// Value ⇒ zero delta); net-new growth checks budget and rolls back on breach.
pub fn atomic_insert(
    store: &Store,
    key: IpAddr,
    entry: Entry,
    ram_limit_bytes: usize,
) -> Result<()> {
    let entry_size =
        std::mem::size_of::<IpAddr>() + std::mem::size_of::<Entry>() + entry.value.heap_bytes();

    // DashMap Entry API (shard-aware locking) — single lock round-trip.
    let mut map_entry = store.inner().entry(key);

    match map_entry {
        dashmap::mapref::entry::Entry::Occupied(ref mut o) => {
            let old_heap = o.get().value.heap_bytes();
            let new_heap = entry.value.heap_bytes();
            o.insert(entry);
            let delta = new_heap as i64 - old_heap as i64;
            if delta > 0 {
                store
                    .traffic
                    .used_bytes
                    .fetch_add(delta as u64, std::sync::atomic::Ordering::Relaxed);
            }
        }
        dashmap::mapref::entry::Entry::Vacant(v) => {
            let current_bytes = store
                .traffic
                .used_bytes
                .load(std::sync::atomic::Ordering::Relaxed);
            if current_bytes as usize + entry_size > ram_limit_bytes {
                return Err(ramshield_types::RsError::CapacityExceeded {
                    limit_mb: ram_limit_bytes / (1024 * 1024),
                });
            }
            v.insert(entry);
            store
                .traffic
                .used_bytes
                .fetch_add(entry_size as u64, std::sync::atomic::Ordering::Relaxed);
        }
    }

    Ok(())
}
