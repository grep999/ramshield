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
        for (ip, agg) in local.drain() {
            // Serialize only the batch-boundary merge path, not per-event
            // ingestion. If the shared map is full and this is a new key,
            // flush before admitting it; the current aggregate remains owned
            // locally until there is room, so pressure never drops counts.
            loop {
                let guard = self
                    .pre_aggs_merge_lock
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if self.pre_aggs.contains_key(&ip) || self.pre_aggs.len() < max_entries {
                    self.pre_aggs
                        .entry(ip)
                        .and_modify(|cur| cur.merge_with(&agg))
                        .or_insert(agg);
                    break;
                }
                drop(guard);
                self.flush_pre_aggs_to_store();
            }
        }
    }

    /// Test/diagnostic entry: absorb straight into the shared map (what the
    /// pre-item-3 stream path did). Production workers absorb into a local
    /// buffer and merge via `merge_local`.
    #[cfg(test)]
    pub(crate) fn absorb_shared(&self, ev: ConnectionEvent) {
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
        // (a) CAS gate — one flusher at a time, others skip (they'll retry
        //     next loop iteration); (b) pop() drain — each entry removed
        //     under its own shard lock, racy inserts survive to the next
        //     flush instead of being cleared; (c) the real elapsed window is
        //     passed to flush_batch so events_last_second is a true rate
        //     even when prod flushes every 100ms (old code stored raw
        //     per-flush counts, lieing 10x to the forecaster).
        // Pair with merge_local's admission lock: while a flush snapshots and
        // removes entries, no production worker can refill the map past its
        // configured bound.
        let _merge_guard = self
            .pre_aggs_merge_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self
            .flushing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        let _flush_guard = FlushGuard(&self.flushing);

        let now = now_ns();
        let prev = self.last_pre_aggs_flush_ns.swap(now, Ordering::Relaxed);
        let window_ns = now.saturating_sub(prev).max(1);

        if self.pre_aggs.is_empty() {
            return;
        }

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
