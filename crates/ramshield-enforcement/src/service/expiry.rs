use crate::*;

impl EnforcementService {
    pub(crate) async fn expire_due(&mut self) {
        // O(due): only buckets whose (whole-second) deadline has passed are
        // touched — previously every 250ms tick re-examined ALL expirations.
        let now_ts = self.epoch.elapsed().as_secs();
        let mut due = Vec::new();
        while let Some((&b, _)) = self.buckets.first_key_value() {
            if b > now_ts {
                break;
            }
            if let Some((_, vec)) = self.buckets.pop_first() {
                for ip in vec {
                    self.expirations.remove(&ip);
                    due.push(ip);
                }
            }
        }
        for (idx, ip) in due.into_iter().enumerate() {
            if idx > 0 && idx % 128 == 0 {
                tokio::task::yield_now().await;
            }
            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 0,
                source: "ttl".into(),
                actor: "system".into(),
                timestamp_utc: epoch_seconds(),
                ttl_seconds: 0,
                reason: "ttl_expired".into(),
                ip,
                cidr: None,
                action: EnforceAction::Unblock,
                evidence_source: ramshield_types::EvidenceSource::LocalSignals,
            };
            match self.enforce(cmd).await {
                Ok(_) => {
                    self.metrics.inc_blocks_expired();
                }
                Err(EnforcementError::InvalidCommand(_)) => {
                    warn!(%ip, "TTL unblock rejected as invalid; dropping lease");
                }
                Err(e) => {
                    // A transient WAL/storage failure must never convert a
                    // temporary block into a permanent one: re-arm the lease
                    // one second out and let the next tick retry the same
                    // Unblock transition.
                    warn!(%ip, "TTL unblock failed: {e} — re-arming lease");
                    self.schedule_expiration(ip, Instant::now() + Duration::from_secs(1));
                }
            }
        }
        let now = Instant::now();
        let cidrs: Vec<IpNetwork> = self
            .cidr_expirations
            .iter()
            .filter_map(|(network, &deadline)| (deadline <= now).then_some(*network))
            .collect();
        for network in cidrs {
            self.cidr_expirations.remove(&network);
            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 0,
                source: "ttl".into(),
                actor: "system".into(),
                timestamp_utc: epoch_seconds(),
                ttl_seconds: 0,
                reason: "ttl_expired".into(),
                ip: network.addr,
                cidr: Some(network),
                action: EnforceAction::Unblock,
                evidence_source: ramshield_types::EvidenceSource::LocalSignals,
            };
            match self.enforce(cmd).await {
                Ok(_) => {}
                Err(EnforcementError::InvalidCommand(_)) => {
                    warn!(cidr=?network, "CIDR TTL unblock rejected as invalid; dropping lease");
                }
                Err(e) => {
                    warn!(cidr=?network, "CIDR TTL unblock failed: {e} — re-arming lease");
                    self.cidr_expirations
                        .insert(network, Instant::now() + Duration::from_secs(1));
                }
            }
        }
    }

    /// Bucket for a deadline: ceil to whole seconds from epoch, so a bucket
    /// only drains when the exact deadline has passed (never early).
    pub(crate) fn bucket_of(&self, at: Instant) -> u64 {
        let d = at.saturating_duration_since(self.epoch);
        d.as_secs() + u64::from(d.subsec_nanos() > 0)
    }

    /// Remove an IP's pending expiration (O(1)); swap-remove keeps bucket
    /// vectors dense — the moved neighbour's position index is fixed up.
    pub(crate) fn detach_expiration(&mut self, ip: IpAddr) {
        if let Some((b, pos)) = self.expirations.remove(&ip)
            && let Some(vec) = self.buckets.get_mut(&b)
        {
            if pos < vec.len() {
                // Self-heal guard: a drifting index (bug elsewhere) would
                // silently remove the WRONG card and orphan this IP forever.
                // Assert the card identity; on mismatch, scan the bucket.
                let detach_pos = if vec[pos] == ip {
                    Some(pos)
                } else {
                    vec.iter().position(|&candidate| candidate == ip)
                };
                if let Some(p) = detach_pos {
                    vec.swap_remove(p);
                    if let Some(&moved) = vec.get(p)
                        && let Some(slot) = self.expirations.get_mut(&moved)
                    {
                        slot.1 = p;
                    }
                }
            }
            if vec.is_empty() {
                self.buckets.remove(&b);
            }
        }
    }

    pub(crate) fn schedule_expiration(&mut self, ip: IpAddr, at: Instant) {
        self.detach_expiration(ip);
        let b = self.bucket_of(at);
        let idx = {
            let vec = self.buckets.entry(b).or_default();
            vec.push(ip);
            vec.len() - 1
        };
        self.expirations.insert(ip, (b, idx));
    }

    /// P1-4: re-arm the TTL ring with blocks restored from WAL replay.
    /// `replay_wal_into_store` returns remaining-TTL pairs; call before
    /// `run()` so restored blocks expire on schedule instead of forever.
    pub fn restore_expirations(&mut self, pairs: impl IntoIterator<Item = (IpAddr, u64)>) {
        for (ip, remaining_secs) in pairs {
            if remaining_secs == 0 {
                continue;
            }
            let at = Instant::now() + Duration::from_secs(remaining_secs);
            self.schedule_expiration(ip, at);
            if let Some(shared) = &self.checkpoint_shared {
                let now_unix_ns = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                shared.set_ip_expiration(ip, unix_ns_from_instant(at, now_unix_ns));
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn check_ring_invariant(&self) {
        for (&ip, &(b, pos)) in &self.expirations {
            let vec = self
                .buckets
                .get(&b)
                .unwrap_or_else(|| panic!("{ip} bucket {b} gone"));
            assert_eq!(vec.get(pos), Some(&ip), "index drift for {ip}");
        }
        assert_eq!(
            self.expirations.len(),
            self.buckets.values().map(Vec::len).sum::<usize>(),
            "ring/map size diverged"
        );
    }
}
