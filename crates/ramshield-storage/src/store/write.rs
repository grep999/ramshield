use crate::*;

impl Store {
    /// Insert with RAM limit enforcement. Only enforces limit on net-new growth,
    /// allowing replacement of existing entries without triggering capacity errors.
    /// Capacity semantics: byte-accounted via
    /// heap_bytes delta, errors as Result (never panic/log-and-continue).
    pub fn insert(
        &self,
        key: IpAddr,
        value: Value,
        ttl_secs: Option<u64>,
        ram_limit_bytes: usize,
    ) -> Result<()> {
        let expires_at = ttl_secs.map(|s| Instant::now() + Duration::from_secs(s));
        let new_has_ttl = expires_at.is_some();
        let new_entry = Entry { value, expires_at };
        let new_blocked = new_entry.value.is_blocked();
        let entry_size = std::mem::size_of::<IpAddr>()
            + std::mem::size_of::<Entry>()
            + new_entry.value.heap_bytes();

        // Shard-locked commit (entry API): the shard write lock is held
        // across the capacity check AND the publication, so
        //  - a rejected fresh insert is NEVER observable — the old flow
        //    published via inner.insert() and rolled back with remove(),
        //    leaving a window where a concurrent get() saw a phantom entry;
        //  - RAM accounting changes in the same critical section as the
        //    value it accounts for;
        //  - a replacement swaps the LIVE value under the lock (there is no
        //    stale get()-snapshot to race), so a late commit cannot roll
        //    back a newer value: block state transitions can never be
        //    clobbered by an older copy.
        match self.inner.entry(key) {
            dashmap::Entry::Occupied(mut o) => {
                // Replacement: swap in place. net_growth may be negative
                // (smaller value) — the limit only gates net-new growth, so
                // adjust the counters directly.
                let old = std::mem::replace(
                    o.get_mut(),
                    Entry {
                        value: new_entry.value,
                        expires_at,
                    },
                );
                let old_size = std::mem::size_of::<Entry>()
                    + old.value.heap_bytes()
                    + std::mem::size_of::<IpAddr>();
                let was_blocked = old.value.is_blocked();
                let old_had_ttl = old.expires_at.is_some();
                let net_growth = entry_size.saturating_sub(old_size);
                if entry_size >= old_size {
                    self.ram_bytes
                        .fetch_add(entry_size - old_size, Ordering::Relaxed);
                } else {
                    self.ram_bytes
                        .fetch_sub(old_size - entry_size, Ordering::Relaxed);
                }
                // Blocked index updates while still holding the shard lock.
                // Must be before `drop(o)` to prevent races with concurrent
                // unblock/evict that would observe inconsistent state.
                if !was_blocked && new_blocked {
                    self.blocked_count.fetch_add(1, Ordering::Relaxed);
                    self.blocked_set.insert(key, ());
                } else if was_blocked && !new_blocked {
                    self.blocked_count.fetch_sub(1, Ordering::Relaxed);
                    self.blocked_set.remove(&key);
                }
                match (old_had_ttl, new_has_ttl) {
                    (false, true) => {
                        self.ttl_entries.fetch_add(1, Ordering::Relaxed);
                    }
                    (true, false) => {
                        self.ttl_entries.fetch_sub(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
                if tracing::enabled!(tracing::Level::TRACE) {
                    let current = self.ram_bytes.load(Ordering::Relaxed);
                    tracing::trace!(
                        ram_bytes = current,
                        net_growth,
                        key = %key,
                        "store insert accounted"
                    );
                }
                if entry_size >= old_size {
                    self.traffic
                        .used_bytes
                        .fetch_add((entry_size - old_size) as u64, Ordering::Relaxed);
                } else {
                    self.traffic
                        .used_bytes
                        .fetch_sub((old_size - entry_size) as u64, Ordering::Relaxed);
                }
                self.total_inserts.fetch_add(1, Ordering::Relaxed);
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(key = %key, total_inserts = self.total_inserts.load(Ordering::Relaxed), "store insert committed");
                }
                drop(o); // lock released after all bookkeeping
                Ok(())
            }
            dashmap::Entry::Vacant(v) => {
                // Fresh insert: reserve capacity atomically BEFORE the entry
                // exists. Capacity check must be atomic: two threads both
                // reading the pre-insert `ram_bytes` and both deciding "fits"
                // will both exceed the budget — the CAS serializes them.
                let mut current = self.ram_bytes.load(Ordering::Relaxed);
                loop {
                    if current + entry_size > ram_limit_bytes {
                        tracing::warn!(
                            key = %key,
                            limit_mb = ram_limit_bytes / (1024 * 1024),
                            "store insert rejected: capacity exceeded"
                        );
                        return Err(RsError::CapacityExceeded {
                            limit_mb: ram_limit_bytes / (1024 * 1024),
                        });
                    }
                    match self.ram_bytes.compare_exchange_weak(
                        current,
                        current + entry_size,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(observed) => current = observed,
                    }
                }
                v.insert(new_entry);
                if new_blocked {
                    self.blocked_count.fetch_add(1, Ordering::Relaxed);
                    self.blocked_set.insert(key, ());
                }
                if new_has_ttl {
                    self.ttl_entries.fetch_add(1, Ordering::Relaxed);
                }
                self.traffic
                    .used_bytes
                    .fetch_add(entry_size as u64, Ordering::Relaxed);
                self.total_inserts.fetch_add(1, Ordering::Relaxed);
                if tracing::enabled!(tracing::Level::TRACE) {
                    let current = self.ram_bytes.load(Ordering::Relaxed);
                    tracing::trace!(
                        ram_bytes = current,
                        net_growth = entry_size,
                        key = %key,
                        "store insert accounted"
                    );
                }
                Ok(())
            }
        }
    }

    /// Atomic block of an IP under ONE shard lock. F-9.
    ///
    /// The block path used to be `get()` -> mutate -> `insert()`: two lock
    /// acquisitions. A detection flush that ran in between advanced
    /// `cusum_s`, `sample_count` and the pulse counters on the live
    /// record, and the block write then published its stale snapshot
    /// back - rewinding the attacker's accumulated evidence, and with it
    /// the warm-up count that `hot || cusum_fired || pulse_fired` needs
    /// to reach.
    ///
    /// This touches only `block_state` on a live record and leaves every
    /// stat field alone, so neither side can clobber the other. The
    /// record is published directly in the blocked state: the enforcement
    /// actor is the authority on `block_state`, and a clean state is
    /// never published for an IP it holds blocked.
    ///
    /// `fallback` is the record to publish when there is no live record
    /// (missing or expired). Its stat fields are zeroed by the caller;
    /// its `block_state` is set here.
    ///
    /// A genuinely new key that the RAM budget refuses is an error, the
    /// same refusal `insert` gives.
    pub fn block_ip(&self, key: IpAddr, fallback: IpRecord, ram_limit_bytes: usize) -> Result<()> {
        let state = fallback.block_state.clone();
        debug_assert!(
            matches!(state, BlockState::Blocked { .. }),
            "block_ip publishes a blocked record"
        );
        match self.inner.entry(key) {
            dashmap::Entry::Occupied(mut o) => {
                let e = o.get_mut();
                if !e.is_expired()
                    && let Value::IpRecord(rec) = &mut e.value
                {
                    // Live record: change the block state and nothing else.
                    // The stat fields belong to detection and stay put.
                    let was_blocked = rec.block_state != BlockState::Clean;
                    rec.block_state = state;
                    // A blocked record must not expire under the store's
                    // passive expiry - the enforcement actor is the
                    // authority on the block, not the TTL. This is what
                    // the old insert(ttl = None) did.
                    if e.expires_at.take().is_some() {
                        self.ttl_entries.fetch_sub(1, Ordering::Relaxed);
                    }
                    self.sync_blocked_index(key, was_blocked, true);
                    return Ok(());
                }
                // Occupied but expired / wrong variant: replace in place
                // with the same bookkeeping insert() does for a
                // replacement. The expired record's stats are gone with
                // the expiry; the block state is not.
                let (was_blocked, old_size) = {
                    let old_value = std::mem::replace(&mut e.value, Value::Counter(0));
                    let wb = old_value.is_blocked();
                    let os = std::mem::size_of::<Entry>()
                        + old_value.heap_bytes()
                        + std::mem::size_of::<IpAddr>();
                    (wb, os)
                };
                let old_had_ttl = e.expires_at.is_some();
                e.value = Value::IpRecord(fallback);
                e.expires_at = None;
                if old_had_ttl {
                    self.ttl_entries.fetch_sub(1, Ordering::Relaxed);
                }
                self.sync_blocked_index(key, was_blocked, true);
                let new_size = std::mem::size_of::<Entry>() + std::mem::size_of::<IpAddr>();
                drop(o); // release the shard lock after all bookkeeping
                self.apply_growth_size_only(old_size, new_size);
                Ok(())
            }
            dashmap::Entry::Vacant(v) => {
                // Net-new: the same capacity gate insert() applies.
                let entry_size = std::mem::size_of::<IpAddr>() + std::mem::size_of::<Entry>();
                let mut current = self.ram_bytes.load(Ordering::Relaxed);
                loop {
                    if current + entry_size > ram_limit_bytes {
                        tracing::warn!(
                            key = %key,
                            limit_mb = ram_limit_bytes / (1024 * 1024),
                            "store block rejected: capacity exceeded"
                        );
                        return Err(RsError::CapacityExceeded {
                            limit_mb: ram_limit_bytes / (1024 * 1024),
                        });
                    }
                    match self.ram_bytes.compare_exchange_weak(
                        current,
                        current + entry_size,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(observed) => current = observed,
                    }
                }
                v.insert(Entry {
                    value: Value::IpRecord(fallback),
                    expires_at: None,
                });
                self.blocked_count.fetch_add(1, Ordering::Relaxed);
                self.blocked_set.insert(key, ());
                self.traffic
                    .used_bytes
                    .fetch_add(entry_size as u64, Ordering::Relaxed);
                self.total_inserts.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
        }
    }

    /// Atomic unblock of an IP under ONE shard lock. F-9.
    ///
    /// The mirror of `block_ip`, and the same defect: the unblock path
    /// was `get()` -> mutate -> `insert()`, so a detection flush in
    /// between had its counters rolled back by the unblock's stale
    /// snapshot. Here the live record keeps its stat fields and only
    /// the block state is cleared.
    ///
    /// A missing or expired record is left alone: an unblock must not
    /// create a record for an IP there is no state for.
    pub fn unblock_ip(&self, key: &IpAddr) -> bool {
        let Some(mut o) = self.inner.get_mut(key) else {
            return false;
        };
        let e = &mut *o;
        if e.is_expired() {
            return false;
        }
        let Value::IpRecord(rec) = &mut e.value else {
            return false;
        };
        let was_blocked = rec.block_state != BlockState::Clean;
        rec.block_state = BlockState::Clean;
        // Same TTL semantics as the block path and as the old
        // insert(ttl = None): the record becomes permanent.
        if e.expires_at.take().is_some() {
            self.ttl_entries.fetch_sub(1, Ordering::Relaxed);
        }
        self.sync_blocked_index(*key, was_blocked, false);
        true
    }

    /// Keep the store's blocked index in step with a block-state change.
    ///
    /// Called while the shard lock is held, so a concurrent reader never
    /// sees the index disagree with the record.
    fn sync_blocked_index(&self, key: IpAddr, was_blocked: bool, now_blocked: bool) {
        if !was_blocked && now_blocked {
            self.blocked_count.fetch_add(1, Ordering::Relaxed);
            self.blocked_set.insert(key, ());
        } else if was_blocked && !now_blocked {
            self.blocked_count.fetch_sub(1, Ordering::Relaxed);
            self.blocked_set.remove(&key);
        }
    }

    /// Atomic read-modify-write of an `IpRecord` under ONE shard lock.
    ///
    /// P0 fix (round-4 Q3): `merge_record` used `get()` -> clone -> mutate ->
    /// `insert()` — two lock acquisitions. If the enforcement actor blocked an
    /// IP inside that window, the flusher's stale snapshot (captured while
    /// still `Clean`) re-inserted over the fresh `Blocked` state: active
    /// attacker resurrected, exactly under flood conditions. The reverse
    /// order also lost stat updates to last-writer-wins.
    ///
    /// Contract: `f` mutates only stat fields — it MUST NOT touch
    /// `block_state` (that authority lives with the enforcement actor, which
    /// keeps using `insert`). Because the record is mutated in place, other
    /// writers' block transitions can never be clobbered by a stale copy.
    ///
    /// Returns `(f's result, stored)` — `stored=false` when a genuinely new
    /// key was refused by the RAM budget (the mutated record is dropped;
    /// matches `insert`'s CapacityExceeded behavior, which also only ever
    /// rejects net-new keys).
    pub fn update_ip<R>(
        &self,
        key: IpAddr,
        default: IpRecord,
        ram_limit_bytes: usize,
        f: impl FnOnce(&mut IpRecord) -> R,
    ) -> (R, bool) {
        match self.inner.entry(key) {
            dashmap::Entry::Occupied(mut o) => {
                let e = o.get_mut();
                // Live IpRecord: mutate in place, zero accounting changes
                // (IpRecord is fixed-size, heap_bytes() == 0 by design).
                if !e.is_expired()
                    && let Value::IpRecord(rec) = &mut e.value
                {
                    return (f(rec), true);
                }
                // Occupied but expired / wrong variant: replace in place with
                // the same bookkeeping insert() does for a replacement.
                let (was_blocked, old_size) = {
                    let old_value = std::mem::replace(&mut e.value, Value::Counter(0));
                    let wb = old_value.is_blocked();
                    let os = std::mem::size_of::<Entry>()
                        + old_value.heap_bytes()
                        + std::mem::size_of::<IpAddr>();
                    (wb, os)
                };
                let mut rec = default;
                let out = f(&mut rec);
                let old_had_ttl = e.expires_at.is_some();
                e.value = Value::IpRecord(rec);
                e.expires_at = None;
                if old_had_ttl {
                    // Replaced a ttl-bearing entry with a permanent one.
                    self.ttl_entries.fetch_sub(1, Ordering::Relaxed);
                }
                // Blocked index update while still holding shard lock.
                if was_blocked {
                    self.blocked_count.fetch_sub(1, Ordering::Relaxed);
                    self.blocked_set.remove(&key);
                }
                let new_size = std::mem::size_of::<Entry>() + std::mem::size_of::<IpAddr>();
                drop(o); // release shard lock after all bookkeeping
                self.apply_growth_size_only(old_size, new_size);
                (out, true)
            }
            dashmap::Entry::Vacant(v) => {
                let mut rec = default;
                let out = f(&mut rec);
                let entry_size = std::mem::size_of::<IpAddr>() + std::mem::size_of::<Entry>(); // heap 0, Clean
                // Net-new capacity gate: same CAS loop as insert()'s fresh path.
                let mut current = self.ram_bytes.load(Ordering::Relaxed);
                loop {
                    if current + entry_size > ram_limit_bytes {
                        tracing::warn!("Store::update_ip - CapacityExceeded for key: {}", key);
                        return (out, false);
                    }
                    match self.ram_bytes.compare_exchange_weak(
                        current,
                        current + entry_size,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(observed) => current = observed,
                    }
                }
                v.insert(Entry {
                    value: Value::IpRecord(rec),
                    expires_at: None,
                });
                self.traffic
                    .used_bytes
                    .fetch_add(entry_size as u64, Ordering::Relaxed);
                self.total_inserts.fetch_add(1, Ordering::Relaxed);
                (out, true)
            }
        }
    }
}
