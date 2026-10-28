use super::*;

impl DetectionEngine {
    /// Move a worker's local buffer into shared `pre_aggs` (RAM-for-CPU
    /// item 3). Runs once per flush boundary, not per event. Same-IP
    /// consolidation across workers uses IpAgg::merge_with, so the batch
    /// that reaches flush_batch keeps the exact old cross-worker semantics
    /// (one entry per IP per flush window, summed counts).
    pub(crate) fn merge_local(&self, local: &mut HashMap<IpAddr, IpAgg>) {
        if local.is_empty() {
            return;
        }
        let max_entries = self.config.load().detection.pre_aggs_max_size;

        // ONE lock acquisition for the whole drain.
        //
        // The old code took the merge lock once PER IP. At high IP
        // cardinality that turned a parallel aggregation into a serialised
        // one: every worker contended for the same mutex once per distinct
        // IP it had seen, and the mutex was held across the whole map walk
        // whenever it flushed — so a flush by one worker stalled every other
        // worker's merge for the duration of a million-entry walk.
        //
        // Holding the lock for the whole drain also makes the drain atomic
        // with respect to other workers' merges, which is what the bound
        // needs: no peer can refill the map between two of our own keys.
        let _merge_guard = self
            .pre_aggs_merge_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        for (ip, agg) in local.drain() {
            // Open the rate observation window at the FIRST event that lands
            // in an empty shared map: the batch's window is measured from its
            // first event, never from flush scheduling.
            if self.pre_aggs.is_empty() {
                self.pre_aggs_batch_start_ns
                    .compare_exchange(0, now_ns(), Ordering::Relaxed, Ordering::Relaxed)
                    .ok();
            }
            // The shared map is full and this is a new key: flush to make
            // room. The current aggregate remains owned locally until there
            // is room, so pressure never drops counts.
            //
            // Because the merge lock is held for the whole drain, only OUR
            // OWN inserts can have filled the map since the last flush, so
            // this branch is reached at most ceil(local.len() / max_entries)
            // times per drain. The old code released the lock before
            // flushing, so a peer could refill the map between the flush and
            // the next key — and the flush was then repeated once per IP,
            // making a single drain O(keys x map size).
            loop {
                if self.pre_aggs.contains_key(&ip) || self.pre_aggs.len() < max_entries {
                    self.pre_aggs
                        .entry(ip)
                        .and_modify(|cur| cur.merge_with(&agg))
                        .or_insert(agg);
                    break;
                }
                self.flush_pre_aggs_locked();
            }
        }
    }

    /// Test/diagnostic entry: absorb straight into the shared map (what the
    /// pre-item-3 stream path did). Production workers absorb into a local
    /// buffer and merge via `merge_local`.
    #[cfg(test)]
    pub(crate) fn absorb_shared(&self, ev: ConnectionEvent) {
        if self.pre_aggs.is_empty() {
            self.pre_aggs_batch_start_ns
                .compare_exchange(0, now_ns(), Ordering::Relaxed, Ordering::Relaxed)
                .ok();
        }
        self.pre_aggs
            .entry(ev.ip)
            .and_modify(|a| a.absorb(&ev))
            .or_insert_with(|| {
                let mut a = IpAgg::default();
                a.absorb(&ev);
                a
            });
    }

    pub(crate) fn pre_aggs_needs_flush_due_to_timeout(&self, interval_ms: u64) -> bool {
        let last_flush = self.last_pre_aggs_flush_ns.load(Ordering::Relaxed);
        now_ns().saturating_sub(last_flush) >= interval_ms * 1_000_000
    }

    pub(crate) fn flush_pre_aggs_to_store(&self) {
        // P1 fix (F1 race + F7 rate): N workers can hit the flush trigger in
        // the same tick. Old path: iter_mut + mem::take + clear() — a worker
        // inserting during another's walk had its fresh event erased by the
        // clear(), silently dropping up to walk_duration x rate events
        // (~2K/flush at 1M eps) and pushing zero-ghost uniques. Now:
        // (a) the merge lock gives one flusher at a time, others wait
        // (they'll retry next loop iteration); (b) pop() drain — each entry
        // removed under its own shard lock, racy inserts survive to the next
        // flush instead of being cleared; (c) the real elapsed window is
        // passed to flush_batch so events_last_second is a true rate
        // even when prod flushes every 100ms (old code stored raw
        // per-flush counts, lieing 10x to the forecaster).
        // Pair with merge_local's admission lock: while a flush snapshots and
        // removes entries, no production worker can refill the map past its
        // configured bound.
        let _merge_guard = self
            .pre_aggs_merge_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.flush_pre_aggs_locked();
    }

    /// The body of the pre-aggregation flush.
    ///
    /// The caller MUST hold `pre_aggs_merge_lock`. It is held for the whole
    /// walk, so the map cannot be refilled while it is drained and the walk
    /// is a consistent cut of the shared aggregate map.
    fn flush_pre_aggs_locked(&self) {
        let now = now_ns();
        // Every attempt advances the TIMEOUT SCHEDULER so idle workers don't
        // spin the trigger; the rate window is measured independently below.
        let prev = self.last_pre_aggs_flush_ns.swap(now, Ordering::Relaxed);

        if self.pre_aggs.is_empty() {
            return;
        }

        // Rate observation window = since the batch's FIRST event. Empty
        // flushes and worker scheduling cannot distort it: idle time before
        // the batch started is excluded (batch start is stamped on the first
        // insert into an empty map), and idle flushes don't move it. When no
        // batch start exists (pre-aggs seeded by a legacy/test path), fall
        // back to the previous flush's timestamp.
        let start = self.pre_aggs_batch_start_ns.swap(0, Ordering::Relaxed);
        let window_ns = if start != 0 {
            now.saturating_sub(start)
        } else {
            now.saturating_sub(prev)
        }
        .max(1);

        // (DashMap 6 has no pop() — reviewer snippet was aspirational.
        // Collect keys, then per-key remove(): each remove is atomic under the
        // shard lock and returns the CURRENT value, so events racing in between
        // are either included here or survive as a fresh entry for next flush.)
        let keys: Vec<IpAddr> = self.pre_aggs.iter().map(|e| *e.key()).collect();
        let mut aggs: Vec<(IpAddr, IpAgg)> = Vec::with_capacity(keys.len());
        for k in keys {
            if let Some((ip, agg)) = self.pre_aggs.remove(&k) {
                aggs.push((ip, agg));
            }
        }

        self.metrics.inc_pre_aggs_flush();
        let total_events: u64 = aggs.iter().map(|a| a.1.count as u64).sum();
        self.metrics.inc_ingested(total_events);
        // ponytail: the batch path is the owner of analytics ingestion. Keep
        // counters adjacent to the aggregate so dashboard values cannot drift
        // from the events actually entering detection.
        self.metrics.inc_hll_inserts(aggs.len() as u64);
        self.metrics.inc_cms_increments(total_events);

        let subnet_counts = subnet_counts_of(&aggs);
        self.flush_batch(
            &aggs,
            &subnet_counts,
            &HashMap::new(),
            total_events,
            window_ns,
        );
    }
}
