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
                // `insert_entry` keeps the shard lock until the guard is
                // dropped, so the publication, the blocked index and the
                // size accounting land in one critical section. `insert`
                // releases the lock here, and a concurrent unblock or
                // remove landing in that window would decrement a counter
                // that is still zero - the same defect F-9 removed from
                // `block_ip`, one function over.
                let entry = v.insert_entry(new_entry);
                self.sync_blocked_index(key, false, new_blocked);
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
                drop(entry); // release the shard lock after all bookkeeping
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
        // The caller owns the reason, so the fallback must carry a
        // blocked state. Publishing it as-is would put the key in the
        // blocked index while the record says it is clean - the
        // `debug_assert!` that used to guard this was compiled out in
        // release builds.
        let state = match &fallback.block_state {
            BlockState::Blocked { reason, since_ns } => BlockState::Blocked {
                reason: *reason,
                since_ns: *since_ns,
            },
            other => {
                return Err(RsError::InvalidState(format!(
                    "block_ip requires a blocked fallback, got {other:?}"
                )));
            }
        };
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
                // The size accounting belongs inside the lock, like
                // insert() and update_ip() do it: outside the lock the
                // capacity gate can be momentarily exceeded by a
                // concurrent replacement.
                self.apply_growth_size_only(old_size, new_size);
                drop(o); // release the shard lock after all bookkeeping
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
                // The index update has to happen while the shard lock is
                // still held. `insert` would release it, and a concurrent
                // unblock landing in that window would decrement a counter
                // that is still zero - a u64 underflow - and leave the
                // index holding a key whose record is clean.
                // `insert_entry` keeps the lock until the guard is dropped.
                let entry = v.insert_entry(Entry {
                    value: Value::IpRecord(fallback),
                    expires_at: None,
                });
                self.sync_blocked_index(key, false, true);
                self.traffic
                    .used_bytes
                    .fetch_add(entry_size as u64, Ordering::Relaxed);
                self.total_inserts.fetch_add(1, Ordering::Relaxed);
                drop(entry);
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
    /// Every call site holds the shard lock, so a concurrent reader never
    /// sees the index disagree with the record: a reader that can see the
    /// record can also see the index entry. The vacant branch of
    /// `block_ip` used to break this - it published the record with
    /// `insert`, which releases the lock, and updated the index
    /// afterwards.
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
    /// uses `block_ip` and `unblock_ip`). Because the record is mutated in
    /// place, other writers' block transitions can never be clobbered by a
    /// stale copy.
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
                let new_blocked = rec.block_state != BlockState::Clean;
                let old_had_ttl = e.expires_at.is_some();
                e.value = Value::IpRecord(rec);
                e.expires_at = None;
                if old_had_ttl {
                    // Replaced a ttl-bearing entry with a permanent one.
                    self.ttl_entries.fetch_sub(1, Ordering::Relaxed);
                }
                // Blocked index update while still holding shard lock.
                self.sync_blocked_index(key, was_blocked, new_blocked);
                let new_size = std::mem::size_of::<Entry>() + std::mem::size_of::<IpAddr>();
                // The size accounting belongs inside the lock, like
                // `insert` and `block_ip` do it: outside the lock the
                // capacity gate can be momentarily exceeded by a
                // concurrent insert.
                self.apply_growth_size_only(old_size, new_size);
                drop(o); // release shard lock after all bookkeeping
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
                // The publication, the blocked index and the size
                // accounting share one critical section. `insert` would
                // release the shard lock before the index was updated -
                // and the published record's block state would never
                // reach the index at all, so a blocked record the caller
                // published would be invisible to every reader that asks
                // the index.
                let new_blocked = rec.block_state != BlockState::Clean;
                let entry = v.insert_entry(Entry {
                    value: Value::IpRecord(rec),
                    expires_at: None,
                });
                self.sync_blocked_index(key, false, new_blocked);
                self.traffic
                    .used_bytes
                    .fetch_add(entry_size as u64, Ordering::Relaxed);
                self.total_inserts.fetch_add(1, Ordering::Relaxed);
                drop(entry); // release the shard lock after all bookkeeping
                (out, true)
            }
        }
    }

    /// Single-shard-lock mutate of only `block_state` — mirrors `update_ip`'s
    /// contract for the enforcement block path.
    ///
    /// The enforcement block path previously did `store.get()` -> mutate
    /// `block_state` -> `store.insert()`, two lock acquisitions. Between them a
    /// concurrent detection flush could advance `cusum_s`, `sample_count` and
    /// pulse counters via `update_ip`, only for enforcement's stale `insert()`
    /// to roll those counters back. `update_block_state` holds one shard lock
    /// for the whole read-modify-write and touches only `block_state`, leaving
    /// stat fields untouched.
    ///
    /// Returns `true` if the record was created or mutated; `false` when a
    /// net-new key was refused by the RAM budget.
    pub fn update_block_state(
        &self,
        key: IpAddr,
        new_state: BlockState,
        ram_limit_bytes: usize,
    ) -> bool {
        match self.inner.entry(key) {
            dashmap::Entry::Occupied(mut o) => {
                let e = o.get_mut();
                // Live IpRecord: mutate only block_state in place.
                // `if let` can't be hoisted into the condition (no let chains in Rust 2015),
                // so the nesting is intentional.
                #[allow(clippy::collapsible_if)]
                if !e.is_expired() {
                    if let Value::IpRecord(rec) = &mut e.value {
                        rec.block_state = new_state;
                        return true;
                    }
                }
                // Occupied but expired / wrong variant: replace in place,
                // preserving bookkeeping (TTL counter, blocked index, size).
                let (was_blocked, old_size) = {
                    let old_value = std::mem::replace(&mut e.value, Value::Counter(0));
                    let wb = old_value.is_blocked();
                    let os = std::mem::size_of::<Entry>()
                        + old_value.heap_bytes()
                        + std::mem::size_of::<IpAddr>();
                    (wb, os)
                };
                let rec = IpRecord {
                    ip: key,
                    request_count: 0,
                    ewma_rps: 0.0,
                    cusum_s: 0.0,
                    baseline_rps: 0.0,
                    prev_sample_hot: false,
                    sample_count: 0,
                    relative_breach_streak: 0,
                    pulse_samples_in_window: 0,
                    pulse_window_start_ns: 0,
                    adaptive_baseline_rps: 0.0,
                    adaptive_cusum_s: 0.0,
                    adaptive_sample_count: 0,
                    adaptive_bucket_id: 0,
                    adaptive_bucket_packets: 0,
                    adaptive_bucket_initialized: false,
                    variance: 0.1,
                    last_bucket_ns: 0,
                    recovery_count: 0,
                    bucket_accum_pps: 0.0,
                    detector_mode: 0,
                    adaptive_model_sample_count: 0,
                    first_seen_ns: 0,
                    last_seen_ns: 0,
                    bytes_in: 0,
                    status_dist: [0; 5],
                    proto_fingerprint: 0,
                    threat_score: 0.0,
                    block_state: new_state,
                    adaptive_sample_count: 0,
                };
                let old_had_ttl = e.expires_at.is_some();
                e.value = Value::IpRecord(rec.clone());
                e.expires_at = None;
                if old_had_ttl {
                    self.ttl_entries.fetch_sub(1, Ordering::Relaxed);
                }
                if was_blocked {
                    self.blocked_count.fetch_sub(1, Ordering::Relaxed);
                    self.blocked_set.remove(&key);
                }
                let new_size = std::mem::size_of::<Entry>() + std::mem::size_of::<IpAddr>();
                drop(o);
                self.apply_growth_size_only(old_size, new_size);
                true
            }
            dashmap::Entry::Vacant(v) => {
                let rec = IpRecord {
                    ip: key,
                    request_count: 0,
                    ewma_rps: 0.0,
                    cusum_s: 0.0,
                    baseline_rps: 0.0,
                    prev_sample_hot: false,
                    sample_count: 0,
                    relative_breach_streak: 0,
                    pulse_samples_in_window: 0,
                    pulse_window_start_ns: 0,
                    adaptive_baseline_rps: 0.0,
                    adaptive_cusum_s: 0.0,
                    adaptive_sample_count: 0,
                    adaptive_bucket_id: 0,
                    adaptive_bucket_packets: 0,
                    adaptive_bucket_initialized: false,
                    variance: 0.1,
                    last_bucket_ns: 0,
                    recovery_count: 0,
                    bucket_accum_pps: 0.0,
                    detector_mode: 0,
                    adaptive_model_sample_count: 0,
                    first_seen_ns: 0,
                    last_seen_ns: 0,
                    bytes_in: 0,
                    status_dist: [0; 5],
                    proto_fingerprint: 0,
                    threat_score: 0.0,
                    block_state: new_state,
                    adaptive_sample_count: 0,
                };
                let entry_size = std::mem::size_of::<IpAddr>() + std::mem::size_of::<Entry>();
                let mut current = self.ram_bytes.load(Ordering::Relaxed);
                loop {
                    if current + entry_size > ram_limit_bytes {
                        tracing::warn!(
                            "Store::update_block_state - CapacityExceeded for key: {}",
                            key
                        );
                        return false;
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
                true
            }
        }
    }
}
