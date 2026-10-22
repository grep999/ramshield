use super::*;

impl Metrics {
    pub fn set_xdp_counters(&self, v4_drop: u64, v6_drop: u64, pass: u64, parse_fail: u64) {
        self.xdp_v4_drops.store(v4_drop, Ordering::Relaxed);
        self.xdp_v6_drops.store(v6_drop, Ordering::Relaxed);
        self.xdp_wire_pass.store(pass, Ordering::Relaxed);
        self.xdp_parse_fails.store(parse_fail, Ordering::Relaxed);
    }

    /// Record a successful XDP reconciliation. Age resets to 0 and the
    /// success timestamp moves to now.
    pub fn record_reconcile_success(&self, now_unix: u64) {
        self.reconcile_successes_total
            .fetch_add(1, Ordering::Relaxed);
        self.reconcile_last_success_unix
            .store(now_unix, Ordering::Relaxed);
        self.reconcile_age_seconds.store(0, Ordering::Relaxed);
        if self.xdp_projection_active.load(Ordering::Acquire) {
            self.xdp_projection_stale.store(0, Ordering::Release);
        }
    }

    /// Mark the XDP projection stale immediately (called on mutation failure
    /// so kernel/userspace divergence is visible NOW, not at next reconcile).
    pub fn mark_xdp_projection_stale(&self) {
        if self.xdp_projection_active.load(Ordering::Acquire) {
            self.xdp_projection_stale.store(1, Ordering::Release);
        }
    }

    /// Record a failed XDP reconciliation attempt. Age keeps growing until
    /// the next success (the gauge drifts upward — that drift is the alert).
    pub fn record_reconcile_failure(&self, now_unix: u64, last_success_unix: u64) {
        self.reconcile_failures_total
            .fetch_add(1, Ordering::Relaxed);
        if last_success_unix > 0 {
            let age = now_unix.saturating_sub(last_success_unix);
            self.reconcile_age_seconds.store(age, Ordering::Relaxed);
            if self.xdp_projection_active.load(Ordering::Acquire) {
                self.xdp_projection_stale
                    .store((age > XDP_PROJECTION_STALE_SECS) as u64, Ordering::Release);
            }
        } else if self.xdp_projection_active.load(Ordering::Acquire) {
            self.xdp_projection_stale.store(1, Ordering::Release);
        }
    }

    /// Publish whether the current XDP projection is outside the engine's
    /// freshness contract. The engine is the authority for the threshold.
    pub fn set_xdp_projection_active(&self, active: bool) {
        self.xdp_projection_active.store(active, Ordering::Release);
        if !active {
            self.xdp_projection_stale.store(0, Ordering::Release);
            return;
        }
        let last = self.reconcile_last_success_unix.load(Ordering::Acquire);
        let age = if last == 0 {
            u64::MAX
        } else {
            self.reconcile_age_seconds.load(Ordering::Acquire)
        };
        self.xdp_projection_stale.store(
            (last == 0 || age > XDP_PROJECTION_STALE_SECS) as u64,
            Ordering::Release,
        );
    }

    pub fn tick_reconcile_age(&self, now_unix: u64) {
        let last = self.reconcile_last_success_unix.load(Ordering::Acquire);
        let age = if last > 0 {
            now_unix.saturating_sub(last)
        } else {
            0
        };
        self.reconcile_age_seconds.store(age, Ordering::Relaxed);
        // The age can only make the projection MORE stale, never less. A
        // mutation failure raises the flag on its own, and only a
        // successful reconcile may clear it - recomputing it here erased
        // the failure within one 250 ms tick.
        if self.xdp_projection_active.load(Ordering::Acquire)
            && (last == 0 || age > XDP_PROJECTION_STALE_SECS)
        {
            self.xdp_projection_stale.store(1, Ordering::Release);
        }
    }
}
