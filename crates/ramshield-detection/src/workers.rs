use super::*;

impl DetectionEngine {
    /// Spawns `n` batch-processor threads (default: CPU cores) consuming from the
    /// shared event channel, plus one subnet-analysis thread.  Each batch thread
    /// writes to the same `pre_aggs` DashMap — sharded internally so concurrent
    /// writers on different IPs don't block each other.
    ///
    /// ponytail: if `n == 0`, fall back to available_parallelism. Add a config knob
    /// when worker_threads tuning becomes a real SLO target.
    pub fn spawn_workers(self: Arc<Self>, n: usize) -> bool {
        let det = self.config.load().detection.clone();
        let requested_workers = if n == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        } else {
            n
        };
        // Hard bound the runtime fan-out as well as configuration validation.
        // This protects callers that construct DetectionEngine directly and
        // prevents a pathological worker count from multiplying local maps.
        let n_workers = requested_workers.min(256);
        info!(
            "Detection: spawning {} batch processors (max {} events / {} ms window), 1 subnet loop",
            n_workers, det.batch_max_events, det.batch_window_ms
        );

        // crossbeam Receiver inside Arc — clone Arc for each worker (cheap refcount bump).
        // Each worker drains aggressively with try_recv() inside a recv_timeout window.
        // ponytail: a bare lock unwrap here is poison-panic risk — one panicked
        // worker poisons the shared mutex and every later spawn/join panics too.
        // unwrap_or_else(PoisonError::into_inner) recovers the guard instead.
        let mut started_workers = 0usize;
        let mut subnet_started = false;
        let mut handles = self
            .worker_handles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for i in 0..n_workers {
            let eng = self.clone();
            let rx = self.event_rx.clone();
            // A failed spawn must not panic the daemon; the shared queue keeps
            // whatever this worker would have drained, so log loudly and run
            // with the workers that did start.
            let spawned = std::thread::Builder::new()
                .name(format!("rs-batch-{i}"))
                .spawn(move || eng.batch_processor_loop_from(rx));
            match spawned {
                Ok(h) => {
                    handles.push(h);
                    started_workers += 1;
                }
                Err(e) => error!(
                    "Detection: batch worker rs-batch-{i} did not spawn: {e} — \
                     running with fewer workers (ingest capacity reduced)"
                ),
            }
        }

        let eng = self.clone();
        let spawned = std::thread::Builder::new()
            .name("rs-subnet".into())
            .spawn(move || eng.subnet_batch_loop());
        match spawned {
            Ok(h) => {
                handles.push(h);
                subnet_started = true;
            }
            Err(e) => error!(
                "Detection: subnet batch loop did not spawn: {e} — \
                 CIDR/subnet batch-block is DISABLED until restart"
            ),
        }
        drop(handles);
        started_workers > 0 && subnet_started
    }

    /// F9: block until batch/subnet threads exit (each final-flushes on the
    /// way out). Returns after `grace` elapses at worst.
    pub fn join_workers(&self, grace: std::time::Duration) {
        // ponytail: poison-recover on the shared worker_handles mutex — a
        // panicked worker must not poison every later join.
        let handles: Vec<_> = self
            .worker_handles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect();
        // Workers exit within recv_timeout (<= batch_window_ms) of the flag
        // + one final flush; poll-until-finished gives the grace cap without
        // inventing a join_timeout (std has none). Last-resort join() is safe
        // because every worker path ends in break on the shutdown flag.
        let deadline = std::time::Instant::now() + grace;
        loop {
            if handles.iter().all(|h| h.is_finished()) {
                break;
            }
            if std::time::Instant::now() >= deadline {
                warn!("join_workers: grace expired with workers still running");
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        for h in handles {
            if h.is_finished() {
                let _ = h.join();
            }
        }
    }

    /// Core batch loop — takes an explicit Receiver so N workers can share the
    /// same crossbeam channel (Receiver is Clone).
    ///
    /// RAM-for-CPU item 3: events are absorbed into a worker-LOCAL
    /// open-addressed map — zero shared locks on the per-event path (the old
    /// shared DashMap took a shard write-lock per event, ping-ponging cache
    /// lines between workers). The buffer merges into `pre_aggs` only at the
    /// flush boundary (interval grain), and the flush itself is the F1
    /// single-flusher CAS path, so cross-worker counts keep the old
    /// semantics: one entry per IP per flush window.
    /// ponytail: memory guard stays on shared pre_aggs.len() only; local
    /// buffers add ≤ one flush interval of uniques per worker (~100ms of
    /// fanout). Revisit if pre_aggs_max_size tuning becomes an SLO.
    pub(crate) fn batch_processor_loop_from(&self, rx: Arc<Receiver<ConnectionEvent>>) {
        let mut local: HashMap<IpAddr, IpAgg> = HashMap::new();
        loop {
            if self.shutdown.load(Ordering::Acquire) {
                // P2 fix (F9): events sitting in pre_aggs at exit (up to one
                // flush interval — 100ms in prod) were never promoted to the
                // store; WAL persists blocks, not counts. Final flush on the
                // way out; the single-flusher CAS gate makes N concurrent
                // exit-flushes safe (one drains, others no-op).
                self.merge_local(&mut local);
                self.flush_pre_aggs_to_store();
                info!("Batch processor shutting down");
                break;
            }

            // Load config once per iteration (interval is fixed for process lifetime).
            let cfg = self.config.load();
            let window = Duration::from_millis(cfg.detection.batch_window_ms);
            let max = cfg.detection.batch_max_events;
            let emergency_threshold = cfg.detection.emergency_burst_threshold;

            // Drain events into the worker-local buffer — no shared lock.
            match rx.recv_timeout(window) {
                Ok(ev) => self.absorb_or_emergency(&mut local, &ev, emergency_threshold),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    // Senders all gone: deliver what we hold, then exit.
                    self.merge_local(&mut local);
                    self.flush_pre_aggs_to_store();
                    break;
                }
            }

            // Drain remaining events up to batch_max_events. Check the
            // distinct-IP cap BEFORE consuming another event: checking only
            // after this loop allowed one large configured batch to grow the
            // worker-local map far beyond LOCAL_MERGE_SOFT_CAP before flushing.
            for _ in 0..max.saturating_sub(1) {
                if local.len() >= LOCAL_MERGE_SOFT_CAP {
                    break;
                }
                match rx.try_recv() {
                    Ok(ev) => self.absorb_or_emergency(&mut local, &ev, emergency_threshold),
                    Err(_) => break,
                }
            }

            // Flush pre_aggs to main store when size or timeout threshold hit
            if self.pre_aggs.len() >= cfg.detection.pre_aggs_max_size
                || local.len() >= LOCAL_MERGE_SOFT_CAP
                || self
                    .pre_aggs_needs_flush_due_to_timeout(cfg.detection.pre_aggs_flush_interval_ms)
            {
                self.merge_local(&mut local);
                self.flush_pre_aggs_to_store();
            }
        }
    }
}
