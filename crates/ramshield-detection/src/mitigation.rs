use super::*;

impl DetectionEngine {
    /// Emit a bounded application-layer mitigation through the same enforcement
    /// queue as every other detector. L7 never bypasses WAL/XDP authority.
    pub(crate) fn emit_l7_block(&self, ip: IpAddr, reason: BlockReason, ttl_secs: u64) {
        let key = (ip, reason);
        if !self.admit_mitigation(key, ttl_secs, now_ns()) {
            return;
        }
        let cmd = EnforceCommand {
            decision_id: Uuid::new_v4(),
            policy_version: 1,
            source: "detection-l7".into(),
            actor: "system".into(),
            timestamp_utc: (now_ns() / 1_000_000_000) as i64,
            ttl_seconds: ttl_secs,
            reason: reason.as_str().into(),
            ip,
            cidr: None,
            action: EnforceAction::Block,
            evidence_source: ramshield_types::EvidenceSource::LocalSignals,
        };
        match self.enforcement_tx.try_send(cmd) {
            Ok(()) => {}
            Err(_) => self.retreat_mitigation(key),
        }
    }

    /// Step 3 fast path: emit a per-IP block the instant its unflushed-window
    /// event count crosses `emergency_threshold`, instead of waiting for the
    /// periodic flush (~50-1000ms of uninhibited traffic). `local` drains at
    /// each flush, so its count IS the current window's count; the threshold
    /// is crossed exactly once (count only rises) — no per-IP flag needed.
    /// Reuses the flush path's emission verbatim (same command, same
    /// try_send, same dropped-block metric); the enforcement layer dedupes
    /// by (ip, reason), so a later flush that re-blocks just refreshes TTL.
    #[inline]
    pub(crate) fn absorb_or_emergency(
        &self,
        local: &mut HashMap<IpAddr, IpAgg>,
        ev: &ConnectionEvent,
        emergency_threshold: u32,
    ) {
        let a = local.entry(ev.ip).or_default();
        a.absorb(ev);
        // `count` increments by exactly 1 per event, so it crosses the threshold
        // at exactly `count == threshold` — one lookup, and it can only be true
        // once per window (one-shot, no per-IP flag needed).
        if emergency_threshold > 0 && a.count == emergency_threshold {
            self.emit_emergency_block(ev.ip, a.count);
        }
    }

    /// Admission gate: may we emit a mitigation for `(ip, reason)` now?
    ///
    /// First admission of a key passes and stamps the wall clock; a later
    /// admission inside the cooldown (ttl/2, or 30s for permanent blocks)
    /// is suppressed. The cooldown re-admission IS the block TTL refresh —
    /// it stops the per-flush-window re-emission that previously re-did the
    /// whole enforcement command path (WAL, store, ring, XDP) for every
    /// sustained attacker.
    ///
    /// On `try_send` failure the caller MUST `retreat_mitigation` so the
    /// next window retries: a queue-full rejection is a delivery failure,
    /// not a suppression signal.
    pub(crate) fn admit_mitigation(
        &self,
        key: (IpAddr, BlockReason),
        ttl_secs: u64,
        now: u64,
    ) -> bool {
        let cooldown_ns = if ttl_secs == 0 {
            30_000_000_000
        } else {
            (ttl_secs.saturating_mul(1_000_000_000) / 2).max(1_000_000_000)
        };
        // The check/evict/insert must be one critical section: separate
        // DashMap operations allow concurrent emitters to all observe spare
        // capacity and then exceed the hard limit.
        let _guard = self
            .pending_mitigation_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if self
            .pending_mitigations
            .get(&key)
            .is_some_and(|g| now.saturating_sub(*g) < cooldown_ns)
        {
            return false;
        }

        // Strictly bound memory without a full-map sweep on every new key.
        // At saturation, evict one existing admission before inserting. This
        // uses a shard iterator and avoids deliberate full-map scans,
        // unlike retain()+min_by_key(), which makes each flood key trigger one
        // or two O(capacity) scans. Eviction can allow an earlier
        // duplicate mitigation, but enforcement remains authoritative and
        // queue admission still bounds delivery.
        if !self.pending_mitigations.contains_key(&key) {
            while self.pending_mitigations.len() >= PENDING_MITIGATION_CAP {
                let victim = self
                    .pending_mitigations
                    .iter()
                    .next()
                    .map(|entry| *entry.key());
                match victim {
                    Some(victim) => {
                        self.pending_mitigations.remove(&victim);
                    }
                    None => break,
                }
            }
        }

        self.pending_mitigations.insert(key, now);
        true
    }

    /// Undo an admission whose command never reached the enforcement queue.
    pub(crate) fn retreat_mitigation(&self, key: (IpAddr, BlockReason)) {
        let _guard = self
            .pending_mitigation_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.pending_mitigations.remove(&key);
    }

    /// The emergency emit — byte-identical to flush_batch's HighRps block.
    pub(crate) fn emit_emergency_block(&self, ip: IpAddr, events: u32) {
        let ttl = self.config.load().detection.block_ttl_secs;
        let key = (ip, BlockReason::HighRps);
        if !self.admit_mitigation(key, ttl, now_ns()) {
            return;
        }
        let cmd = EnforceCommand {
            decision_id: Uuid::new_v4(),
            policy_version: 1,
            source: "detection".into(),
            actor: "system".into(),
            timestamp_utc: (now_ns() / 1_000_000_000) as i64,
            ttl_seconds: ttl,
            reason: BlockReason::HighRps.as_str().into(),
            ip,
            cidr: None,
            action: EnforceAction::Block,
            evidence_source: ramshield_types::EvidenceSource::LocalSignals,
        };
        match self.enforcement_tx.try_send(cmd) {
            Ok(()) => {
                self.metrics
                    .record_block_ip(&ip, BlockReason::HighRps.as_str(), "detection");
                // Fast path bypasses flush_batch, so record_batch never sees
                // this decision — count it here or the applied-block total
                // (dashboard blocks_total, enforcement stage) misses it.
                self.metrics
                    .blocks_detection
                    .fetch_add(1, Ordering::Relaxed);
                trace!(ip = %ip, events, "emergency fast-path block emitted pre-flush");
            }
            Err(_) => {
                // Queue rejected it: undo the admission so the next window
                // retries instead of suppressing this key.
                self.retreat_mitigation(key);
                self.metrics.inc_enforcement_dropped();
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn pending_mitigations_len(&self) -> usize {
        // Read under the same admission lock as insert/evict so concurrent
        // tests observe a stable count rather than DashMap's mid-mutation view.
        let _guard = self
            .pending_mitigation_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.pending_mitigations.len()
    }
}
