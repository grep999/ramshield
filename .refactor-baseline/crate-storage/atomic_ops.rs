use crate::{Entry, Store};
use anyhow::{anyhow, Result};
use std::sync::atomic::Ordering;

pub fn atomic_insert(store: &Store, key: String, entry: Entry) -> Result<()> {
    let entry_size = std::mem::size_of::<Entry>() as u64;
    let ram_limit = store.ram_bytes();
    
    // DashMap Entry API (shard-aware locking)
    let mut map_entry = store.inner.entry(key);
    
    match map_entry {
        dashmap::mapref::entry::Entry::Occupied(ref mut o) => {
            // Replacement: size delta is zero for fixed-size Entry
            o.insert(entry);
        }
        dashmap::mapref::entry::Entry::Vacant(v) => {
            // New entry: check RAM using actual tracked bytes
            let current_bytes = store.traffic.used_bytes.load(Ordering::Relaxed);
            if current_bytes + entry_size > ram_limit {
                return Err(anyhow!("CapacityExceeded"));
            }
            v.insert(entry);
            store.traffic.used_bytes.fetch_add(entry_size, Ordering::Relaxed);
            store.traffic.blocked_count.fetch_add(1, Ordering::Relaxed);
        }
    }
    
    Ok(())
}
