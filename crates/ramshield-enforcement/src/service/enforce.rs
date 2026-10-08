use crate::*;

impl EnforcementService {
    pub(crate) fn remember_result(&mut self, result: &EnforceResult) {
        if self
            .processed_results
            .insert(result.decision_id, result.clone())
            .is_none()
        {
            self.processed_order.push_back(result.decision_id);
            while self.processed_order.len() > 65_536 {
                if let Some(old) = self.processed_order.pop_front() {
                    self.processed_results.remove(&old);
                }
            }
        }
    }

    /// Execute one command. Order: WAL append → storage mutation → local/XDP
    /// indexes. A failed storage mutation must not leave a phantom block; a
    /// failed WAL append aborts before any state change (durable-first).
    pub async fn enforce(
        &mut self,
        cmd: EnforceCommand,
    ) -> Result<EnforceResult, EnforcementError> {
        // Idempotent replay: return what actually happened the first time —
        // a fabricated `xdp_applied: true` would hide a dataplane failure
        // from every retry consumer.
        if let Some(cached) = self.processed_results.get(&cmd.decision_id) {
            trace!(decision_id = %cmd.decision_id, "duplicate decision — returning cached original result");
            return Ok(cached.clone());
        }
        if cmd.ip.is_unspecified() {
            trace!(
                ip = %cmd.ip,
                reason = %cmd.reason,
                "enforce rejected: unspecified IP"
            );
            return Err(EnforcementError::InvalidCommand(
                "unspecified IP is not blockable".into(),
            ));
        }

        // Serialize the synchronous WAL + Store commit against checkpoint capture.
        // Keep the guard scoped to the non-awaiting section so the enforcement
        // future remains Send when the actor is spawned on Tokio.

        // Hold the checkpoint barrier through synchronous WAL/store mutation, then release it before async mesh I/O.
        let checkpoint_shared = self.checkpoint_shared.clone();
        let _ckpt_guard = checkpoint_shared.as_ref().map(|shared| {
            shared.barrier.lock().unwrap_or_else(|e| e.into_inner())
        });

        // Step 1: commit intent to WAL (durable) — before any state change.
        let wal_lsn = if let Some(ref wal) = self.wal {
            let now_ns = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            let entry = match cmd.action {
                EnforceAction::Block => match cmd.cidr {
                    Some(cidr) => WalEntry::BlockCidr {
                        cidr,
                        reason: cmd.reason.clone(),
                        ttl_secs: (cmd.ttl_seconds > 0).then_some(cmd.ttl_seconds),
                        ts_ns: now_ns,
                    },
                    None => WalEntry::BlockIp {
                        ip: cmd.ip.to_string(),
                        reason: cmd.reason.clone(),
                        ttl_secs: (cmd.ttl_seconds > 0).then_some(cmd.ttl_seconds),
                        ts_ns: now_ns,
                    },
                },
                EnforceAction::Unblock => match cmd.cidr {
                    Some(cidr) => WalEntry::UnblockCidr {
                        cidr,
                        ts_ns: now_ns,
                    },
                    None => WalEntry::UnblockIp {
                        ip: cmd.ip.to_string(),
                        ts_ns: now_ns,
                    },
                },
            };
            let lsn = wal
                .append(&entry)
                .map_err(|e| EnforcementError::Wal(e.to_string()))?;
            self.last_wal_lsn = Some(lsn);
            Some(lsn)
        } else {
            None
        };

        // Step 2: storage mutation.
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        match cmd.action {
            EnforceAction::Block => {
                let reason = reason_to_block_reason(&cmd.reason);

                // Domain split: a CIDR command is a prefix detention and MUST
                // NOT create/clear an IpRecord, IP TTL, or mesh/mirror state
                // for the representative `cmd.ip`. IP and CIDR are independent
                // detention domains released independently.
                if let Some(network) = cmd.cidr {
                    // CIDR detention is its own state domain.
                    self.store.active_cidrs.insert(network, ());

                    if cmd.ttl_seconds > 0 {
                        // TTL is clamped at every entry point (IPC boundary,
                        // config validate) — checked_add is the belt-and-suspenders
                        // guard so a future path can never overflow into a panic
                        // and kill the enforcement task (blocks silently die).
                        let at = Instant::now()
                            .checked_add(Duration::from_secs(cmd.ttl_seconds))
                            .unwrap_or_else(|| {
                                Instant::now() + Duration::from_secs(MAX_EXPIRY_FALLBACK_SECS)
                            });
                        let now_unix_ns = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_nanos() as u64)
                            .unwrap_or(0);
                        self.cidr_expirations.insert(network, at);
                        if let Some(shared) = &self.checkpoint_shared {
                            shared.set_cidr_expiration(
                                network,
                                unix_ns_from_instant(at, now_unix_ns),
                            );
                        }
                    } else {
                        self.cidr_expirations.remove(&network);
                        if let Some(shared) = &self.checkpoint_shared {
                            shared.remove_cidr_expiration(&network);
                        }
                    }
                } else {
                    // Explicit IP detention.
                    let rec = self
                        .store
                        .get(&cmd.ip)
                        .and_then(|v| match v {
                            Value::IpRecord(r) => Some(r),
                            _ => None,
                        })
                        .unwrap_or(IpRecord {
                            ip: cmd.ip,
                            request_count: 0,
                            ewma_rps: 0.0,
                            cusum_s: 0.0,
                            baseline_rps: 0.0,
                            prev_sample_hot: false,
                            sample_count: 0,
                            relative_breach_streak: 0,
                            pulse_samples_in_window: 0,
                            pulse_window_start_ns: 0,
                            first_seen_ns: now_ns,
                            last_seen_ns: now_ns,
                            bytes_in: 0,
                            status_dist: [0; 5],
                            proto_fingerprint: 0,
                            threat_score: 0.0,
                            block_state: BlockState::Clean,
                        });
                    let mut updated = rec;
                    updated.block_state = BlockState::Blocked {
                        reason,
                        since_ns: now_ns,
                    };

                    // Do not let Store's passive expiry hide a still-blocked record.
                    self.store
                        .insert(
                            cmd.ip,
                            Value::IpRecord(updated),
                            None,
                            self.store.traffic.ram_limit_mb.load(Ordering::Relaxed) * 1024 * 1024,
                        )
                        .map_err(|e| EnforcementError::Storage(e.to_string()))?;

                    self.blocked_ips.insert(cmd.ip);
                    // Re-block resets attribution — a new block is a new epoch.
                    self.drops_by_blocked.remove(&cmd.ip);
                    // Invariant: at most one expiration per IP. A re-block must not
                    // inherit a stale TTL from a previous block/unblock cycle.
                    // Ring schedule/detach are both O(1) — TTL refresh moves the
                    // card between buckets instead of appending a duplicate.
                    if cmd.ttl_seconds > 0 {
                        // TTL is clamped at every entry point (IPC boundary,
                        // config validate) — checked_add is the belt-and-suspenders
                        // guard so a future path can never overflow into a panic
                        // and kill the enforcement task (blocks silently die).
                        let at = Instant::now()
                            .checked_add(Duration::from_secs(cmd.ttl_seconds))
                            .unwrap_or_else(|| {
                                Instant::now() + Duration::from_secs(MAX_EXPIRY_FALLBACK_SECS)
                            });
                        let now_unix_ns = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_nanos() as u64)
                            .unwrap_or(0);
                        self.schedule_expiration(cmd.ip, at);
                        if let Some(shared) = &self.checkpoint_shared {
                            shared.set_ip_expiration(cmd.ip, unix_ns_from_instant(at, now_unix_ns));
                        }
                    } else {
                        self.detach_expiration(cmd.ip);
                        if let Some(shared) = &self.checkpoint_shared {
                            shared.remove_ip_expiration(&cmd.ip);
                        }
                    }
                }

                // Step 3: dataplane (barrier released — XDP stays outside).
                let is_cidr = cmd.cidr.is_some();
                // WAL + Store are complete; release checkpoint coordination before
                // any dataplane or mesh await.
                drop(_ckpt_guard);

                let xdp_applied = match cmd.cidr {
                    Some(network) => {
                        self.xdp
                            .apply_cidr_block(network, cmd.decision_id, cmd.ttl_seconds)
                    }
                    None => self
                        .xdp
                        .apply_block(cmd.ip, cmd.decision_id, cmd.ttl_seconds),
                }
                .map(|()| true)
                .unwrap_or_else(|e| {
                    // Kernel/userspace divergence is real NOW: surface it
                    // immediately, not at the next reconcile tick.
                    self.metrics.mark_xdp_projection_stale();
                    // Userspace + WAL hold this block; the kernel does not, so
                    // the wire keeps passing the target. Counter is the only
                    // scrapeable signal. CIDR LPM tries have a hard cap and no
                    // LRU support, so a full trie means the subnet-swarm leg
                    // has silently stopped — that case is loud, not a warn.
                    self.metrics.inc_xdp_apply_failures();
                    if is_cidr {
                        error!(
                            ip=%cmd.ip, cidr=?cmd.cidr,
                            "XDP subnet block did NOT reach the kernel (CIDR LPM trie full?): {} \
                             — wire mitigation is OFF for this prefix while the engine reports it blocked",
                            e
                        );
                    } else {
                        warn!(ip=%cmd.ip, "XDP block failed: {}", e);
                    }
                    false
                });
                self.metrics.inc_blocks();
                // Prepare the mesh mutation while still on the synchronous
                // enforcement path; perform network I/O only after the checkpoint
                // guard has been released so no std::sync::MutexGuard crosses await.
                let pending_mesh_block = if cmd.cidr.is_none() && cmd.source != "mesh" {
                    self.mesh_operator_suppressions.remove(&cmd.ip);
                    self.mesh_blocklist.as_ref().map(|mesh| {
                        let ttl_ms = cmd.ttl_seconds.saturating_mul(1000);
                        let delta = mesh.record_ban(cmd.ip, ttl_ms, 2);
                        self.metrics.inc_mesh_record_ban();
                        delta
                    })
                } else {
                    None
                };
                drop(_ckpt_guard);

                // Release checkpoint coordination before network I/O.
                drop(_ckpt_guard);

                if let (Some(handle), Some(delta)) = (&self.mesh_handle, pending_mesh_block) {
                    handle
                        .broadcast(ramshield_mesh::MeshMessage::Block(delta))
                        .await;
                }
                let result = EnforceResult {
                    decision_id: cmd.decision_id,
                    committed: true,
                    applied: true,
                    wal_lsn,
                    xdp_applied,
                    error: None,
                };
                self.remember_result(&result);
                trace!(
                    ip = %cmd.ip,
                    action = "block",
                    reason = %cmd.reason,
                    ttl_seconds = cmd.ttl_seconds,
                    wal_lsn = ?wal_lsn,
                    xdp_applied,
                    decision_id = %cmd.decision_id,
                    "enforce applied: block committed"
                );
                Ok(result)
            }
            EnforceAction::Unblock => {
                // Domain split: a CIDR unblock is a prefix release and MUST NOT
                // clear IpRecord/blocked_ips/drops/IP-TTL/mesh state for
                // cmd.ip. Only the explicit-IP path touches IP state.
                if let Some(network) = cmd.cidr {
                    self.cidr_expirations.remove(&network);
                    if let Some(shared) = &self.checkpoint_shared {
                        shared.remove_cidr_expiration(&network);
                    }
                    self.store.active_cidrs.remove(&network);
                } else {
                    if let Some(Value::IpRecord(mut rec)) = self.store.get(&cmd.ip) {
                        rec.block_state = BlockState::Clean;
                        self.store
                            .insert(
                                cmd.ip,
                                Value::IpRecord(rec),
                                None,
                                self.store.traffic.ram_limit_mb.load(Ordering::Relaxed)
                                    * 1024
                                    * 1024,
                            )
                            .map_err(|e| EnforcementError::Storage(e.to_string()))?;
                    }
                    self.blocked_ips.remove(&cmd.ip);
                    self.drops_by_blocked.remove(&cmd.ip);
                    // Prepare mesh unban deltas while the synchronous state transition is serialized.
                    let pending_mesh_unblocks = if let Some(mesh) = &self.mesh_blocklist {
                        let deltas = mesh.record_unban(cmd.ip);
                        self.metrics.inc_mesh_record_unban();
                        (cmd.source != "mesh").then_some(deltas)
                    } else {
                        None
                    };
                    if cmd.source != "mesh" {
                        self.mesh_operator_suppressions.insert(cmd.ip);
                    }
                    self.mesh_applied_ips.remove(&cmd.ip);
                    // Purge any pending TTL so a later re-block starts clean.
                    self.detach_expiration(cmd.ip);
                    if let Some(shared) = &self.checkpoint_shared {
                        shared.remove_ip_expiration(&cmd.ip);
                    }

                    // Release checkpoint coordination before network I/O.
                    drop(_ckpt_guard);
                    if let (Some(handle), Some(deltas)) =
                        (&self.mesh_handle, pending_mesh_unblocks)
                    {
                        for delta in deltas {
                            handle
                                .broadcast(ramshield_mesh::MeshMessage::Unblock(delta))
                                .await;
                        }
                    }
                }
                let xdp_applied = match cmd.cidr {
                    Some(network) => self.xdp.apply_cidr_unblock(network, cmd.decision_id),
                    None => self.xdp.apply_unblock(cmd.ip, cmd.decision_id),
                }
                .map(|()| true)
                .unwrap_or_else(|e| {
                    self.metrics.mark_xdp_projection_stale();
                    warn!(ip=%cmd.ip, cidr=?cmd.cidr, "XDP unblock failed: {}", e);
                    false
                });
                let result = EnforceResult {
                    decision_id: cmd.decision_id,
                    committed: true,
                    applied: true,
                    wal_lsn,
                    xdp_applied,
                    error: None,
                };
                self.remember_result(&result);
                trace!(
                    ip = %cmd.ip,
                    action = "unblock",
                    reason = %cmd.reason,
                    wal_lsn = ?wal_lsn,
                    xdp_applied,
                    decision_id = %cmd.decision_id,
                    "enforce applied: unblock committed"
                );
                Ok(result)
            }
        }
    }
}
