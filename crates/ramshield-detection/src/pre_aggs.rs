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
            if self.pre_aggs.contains_key(&ip) || self.pre_aggs.len() < max_entries {
                self.pre_aggs
                    .entry(ip)
                    .and_modify(|existing| existing.merge_with(&agg))
                    .or_insert(agg);
            } else {
                // Shared map full and new key: flush to make room, then insert.
                self.flush_pre_aggs_locked();
                if self.pre_aggs.is_empty() {
                    self.pre_aggs_batch_start_ns
                        .compare_exchange(0, now_ns(), Ordering::Relaxed, Ordering::Relaxed)
                        .ok();
                }
                self.pre_aggs
                    .entry(ip)
                    .and_modify(|existing| existing.merge_with(&agg))
                    .or_insert(agg);
            }
        }
    }

    pub(crate) fn absorb_shared(&self, ev: ConnectionEvent) {
        if self.pre_aggs.is_empty() {
            self.pre_aggs_batch_start_ns
                .compare_exchange(0, now_ns(), Ordering::Relaxed, Ordering::Relaxed)
                .ok();
        }
        self.pre_aggs
            .entry(ev.ip)
            .and_modify(|agg| agg.absorb(&ev))
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
        let _merge_guard = self
            .pre_aggs_merge_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.flush_pre_aggs_locked();
    }

    /// The body of the pre-aggregation flush.
    ///
    /// The caller MUST hold `pre_aggs_merge_lock`.
    fn flush_pre_aggs_locked(&self) {
        let now = now_ns();
        let prev = self.last_pre_aggs_flush_ns.swap(now, Ordering::Relaxed);

        if self.pre_aggs.is_empty() {
            return;
        }

        // Rate observation window = since the batch's FIRST event.
        let start = self.pre_aggs_batch_start_ns.swap(0, Ordering::Relaxed);
        let window_ns = if start != 0 {
            now.saturating_sub(start)
        } else {
            now.saturating_sub(prev)
        }
        .max(1);

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

#[cfg(test)]
mod rate_window_tests {
    use super::super::*;
    use ramshield_config::Config;
    use std::net::Ipv4Addr;
    use std::sync::atomic::Ordering;
    use tokio::sync::mpsc;

    fn engine() -> Arc<DetectionEngine> {
        let cfg = Config::default().into_handle();
        let store = Arc::new(Store::new(16));
        let metrics = Arc::new(Metrics::new());
        let (etx, _erx) = mpsc::channel(64);
        let shutdown = Arc::new(AtomicBool::new(false));
        Arc::new(
            DetectionEngine::try_new(store, cfg, etx, metrics, shutdown).expect("detection try_new"),
        )
    }

    /// Batch rate window opens on the first insert into an empty shared map
    /// (`pre_aggs_batch_start_ns`), not on flush scheduling. Empty flushes must
    /// not stamp a start; a real flush clears the stamp so the next batch
    /// measures from its own first event.
    #[test]
    fn batch_start_stamps_on_first_shared_insert_and_clears_on_flush() {
        let eng = engine();
        assert_eq!(
            eng.pre_aggs_batch_start_ns.load(Ordering::Relaxed),
            0,
            "idle engine has no open batch window"
        );

        eng.flush_pre_aggs_to_store();
        assert_eq!(
            eng.pre_aggs_batch_start_ns.load(Ordering::Relaxed),
            0,
            "empty flush must not open a rate-observation window"
        );

        let ip: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9));
        let mut local: HashMap<IpAddr, IpAgg> = HashMap::new();
        let mut agg = IpAgg::default();
        agg.absorb(&ConnectionEvent {
            ip,
            timestamp_ns: 1,
            bytes: 64,
            status_code: 200,
            proto_fingerprint: 0,
            l7: None,
        });
        local.insert(ip, agg);
        eng.merge_local(&mut local);

        let start = eng.pre_aggs_batch_start_ns.load(Ordering::Relaxed);
        assert_ne!(start, 0, "first shared insert must open the batch window");

        eng.flush_pre_aggs_to_store();
        assert_eq!(
            eng.pre_aggs_batch_start_ns.load(Ordering::Relaxed),
            0,
            "flush must clear the batch start so the next batch is independent"
        );
        assert!(
            eng.pre_aggs.is_empty(),
            "flush drains the shared pre_aggs map"
        );
    }
}
