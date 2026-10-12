use crate::*;

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) enum PendingMeshMutation {
    Block {
        ip: IpAddr,
        node_id: u32,
        incarnation: Uuid,
        counter: u64,
        created_at_ms: u64,
        expires_at_ms: Option<u64>,
        tier: u8,
    },
    Unblock {
        ip: IpAddr,
        node_id: u32,
        incarnation: Uuid,
        counter: u64,
    },
}

#[allow(dead_code)]
impl PendingMeshMutation {
    pub(crate) fn to_wal_entry(&self, ts_ns: u64) -> WalEntry {
        match self {
            Self::Block {
                ip,
                node_id,
                incarnation,
                counter,
                created_at_ms,
                expires_at_ms,
                tier,
            } => WalEntry::MeshMutation {
                schema_version: 1,
                origin_node_id: *node_id,
                origin_incarnation: incarnation.to_string(),
                origin_counter: *counter,
                ip: ip.to_string(),
                action: ramshield_storage::wal::MeshMutationAction::Block,
                created_at_ms: *created_at_ms,
                expires_at_ms: *expires_at_ms,
                tier: *tier,
                signature: None,
                ts_ns,
            },
            Self::Unblock {
                ip,
                node_id,
                incarnation,
                counter,
            } => WalEntry::MeshMutation {
                schema_version: 1,
                origin_node_id: *node_id,
                origin_incarnation: incarnation.to_string(),
                origin_counter: *counter,
                ip: ip.to_string(),
                action: ramshield_storage::wal::MeshMutationAction::Unblock,
                created_at_ms: ts_ns / 1_000_000,
                expires_at_ms: None,
                tier: 0,
                signature: None,
                ts_ns,
            },
        }
    }
}

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
        self.enforce_inner(cmd, None).await
    }

    #[cfg(feature = "mesh")]
    pub(crate) async fn enforce_mesh_mutation(
        &mut self,
        cmd: EnforceCommand,
        mutation: PendingMeshMutation,
    ) -> Result<EnforceResult, EnforcementError> {
        self.enforce_inner(cmd, Some(mutation)).await
    }

    async fn enforce_inner(
        &mut self,
        cmd: EnforceCommand,
        incoming_mesh_mutation: Option<PendingMeshMutation>,
    ) -> Result<EnforceResult, EnforcementError> {
        #[cfg(not(feature = "mesh"))]
        let _ = &incoming_mesh_mutation;
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

        // Checkpoint barrier: serialize [WAL append + store mutation] against
        // the checkpoint loop's [begin_checkpoint + store capture]. Without it
        // an entry can be durable BELOW the checkpoint boundary while its store
        // mutation is still in flight — the snapshot misses it AND tail replay
        // starts after it: the block silently vanishes on recovery. Steps 1-2
        // contain no await, so a std guard is safe here; XDP (step 3) stays
        // outside the barrier.
        let _ckpt_arc = self.checkpoint_shared.clone();
        let _ckpt_guard = _ckpt_arc
            .as_ref()
            .map(|s| s.barrier.lock().unwrap_or_else(|e| e.into_inner()));

        // Reserve the exact CRDT mutation before writing the WAL, but do not
        // publish it to in-memory mesh state until the event is persisted.
        #[cfg(feature = "mesh")]
        let prepared_mesh_block = if matches!(&cmd.action, EnforceAction::Block)
            && cmd.cidr.is_none()
            && cmd.source != "mesh"
        {
            self.mesh_blocklist
                .as_ref()
                .map(|mesh| mesh.prepare_ban(cmd.ip, cmd.ttl_seconds.saturating_mul(1000), 2))
        } else {
            None
        };
        #[cfg(feature = "mesh")]
        let prepared_mesh_unblocks = if matches!(&cmd.action, EnforceAction::Unblock)
            && cmd.cidr.is_none()
            && cmd.source != "mesh"
        {
            self.mesh_blocklist
                .as_ref()
                .map(|mesh| mesh.prepare_unban(cmd.ip))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        #[cfg(feature = "mesh")]
        let mesh_wal_entries: Vec<WalEntry> = if let Some(mutation) =
            incoming_mesh_mutation.as_ref()
        {
            vec![mutation.to_wal_entry(now_ns)]
        } else if cmd.source != "mesh" && cmd.cidr.is_none() {
            if let Some(delta) = prepared_mesh_block.as_ref() {
                vec![WalEntry::MeshMutation {
                    schema_version: 1,
                    origin_node_id: delta.dot.node_id,
                    origin_incarnation: delta.dot.incarnation.to_string(),
                    origin_counter: delta.dot.counter,
                    ip: delta.ip.to_string(),
                    action: ramshield_storage::wal::MeshMutationAction::Block,
                    created_at_ms: delta.created_at_ms,
                    expires_at_ms: (delta.expires_at_ms != u64::MAX).then_some(delta.expires_at_ms),
                    tier: delta.tier,
                    signature: None,
                    ts_ns: now_ns,
                }]
            } else if !prepared_mesh_unblocks.is_empty() {
                prepared_mesh_unblocks
                    .iter()
                    .map(|delta| WalEntry::MeshMutation {
                        schema_version: 1,
                        origin_node_id: delta.dot.node_id,
                        origin_incarnation: delta.dot.incarnation.to_string(),
                        origin_counter: delta.dot.counter,
                        ip: delta.ip.to_string(),
                        action: ramshield_storage::wal::MeshMutationAction::Unblock,
                        created_at_ms: now_ns / 1_000_000,
                        expires_at_ms: None,
                        tier: 0,
                        signature: None,
                        ts_ns: now_ns,
                    })
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        #[cfg(not(feature = "mesh"))]
        let mesh_wal_entries: Vec<WalEntry> = Vec::new();

        // Step 1: commit intent to WAL (durable) — before any state change.
        // Mesh mutations carry their original CRDT dot. Persist all tombstones
        // for an unblock as one ordered batch before applying any of them.
        let wal_lsn = if let Some(ref wal) = self.wal {
            if !mesh_wal_entries.is_empty() {
                let mut last_lsn = None;
                for entry in &mesh_wal_entries {
                    let lsn = wal
                        .append(entry)
                        .map_err(|e| EnforcementError::Wal(e.to_string()))?;
                    last_lsn = Some(lsn);
                }
                wal.sync()
                    .map_err(|e| EnforcementError::Wal(e.to_string()))?;
                let lsn = last_lsn.expect("non-empty mesh WAL batch has a last LSN");
                self.last_wal_lsn = Some(lsn);
                Some(lsn)
            } else if cmd.source == "mesh" {
                // Incoming mesh events are durably appended by apply_mesh_* before
                // their CRDT merge. Do not write a second, provenance-free record.
                None
            } else {
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
            }
        } else {
            None
        };

        // Once the event is durable, publish the exact same operation into
        // the CRDT before touching projections. If a later store/XDP operation
        // fails, WAL and mesh still agree and reconciliation can repair the projection.
        #[cfg(feature = "mesh")]
        if let Some(mesh) = &self.mesh_blocklist {
            if let Some(mutation) = incoming_mesh_mutation.as_ref() {
                match mutation {
                    PendingMeshMutation::Block {
                        ip,
                        node_id,
                        incarnation,
                        counter,
                        created_at_ms,
                        expires_at_ms,
                        tier,
                    } => {
                        mesh.merge_delta(&ramshield_mesh::aworset::ClusterBlockDelta {
                            ip: *ip,
                            dot: ramshield_mesh::aworset::ClusterDot {
                                node_id: *node_id,
                                incarnation: *incarnation,
                                counter: *counter,
                            },
                            created_at_ms: *created_at_ms,
                            expires_at_ms: expires_at_ms.unwrap_or(u64::MAX),
                            tier: *tier,
                        });
                    }
                    PendingMeshMutation::Unblock {
                        ip,
                        node_id,
                        incarnation,
                        counter,
                    } => {
                        mesh.merge_unblock_delta(&ramshield_mesh::aworset::ClusterUnblockDelta {
                            ip: *ip,
                            dot: ramshield_mesh::aworset::ClusterDot {
                                node_id: *node_id,
                                incarnation: *incarnation,
                                counter: *counter,
                            },
                        });
                    }
                }
            }
            if let Some(delta) = prepared_mesh_block.as_ref() {
                mesh.merge_delta(delta);
            }
            for delta in &prepared_mesh_unblocks {
                mesh.merge_unblock_delta(delta);
            }
        }

        #[cfg(feature = "mesh")]
        if !mesh_wal_entries.is_empty() {
            self.mesh_projection_pending.insert(cmd.ip);
        }

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

                #[cfg(feature = "mesh")]
                if prepared_mesh_block.is_some() {
                    // The userspace Store projection succeeded; a later XDP
                    // failure must not make a peer tombstone look unapplied.
                    self.mesh_applied_ips.insert(cmd.ip);
                }
                #[cfg(feature = "mesh")]
                if !mesh_wal_entries.is_empty() {
                    self.mesh_projection_pending.remove(&cmd.ip);
                }
                drop(_ckpt_guard);
                // Step 3: dataplane (barrier released — XDP stays outside).
                let is_cidr = cmd.cidr.is_some();
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
                // First-party local/operator blocks clear a prior mesh suppression
                // and may publish the decision to peers. Mesh-originated blocks do
                // not rebroadcast, preventing gossip amplification.
                #[cfg(feature = "mesh")]
                if cmd.cidr.is_none() && cmd.source != "mesh" {
                    self.mesh_operator_suppressions.remove(&cmd.ip);
                    if let Some(mesh) = &self.mesh_blocklist {
                        let delta = prepared_mesh_block.clone().unwrap_or_else(|| {
                            mesh.record_ban(cmd.ip, cmd.ttl_seconds.saturating_mul(1000), 2)
                        });
                        self.metrics.inc_mesh_record_ban();
                        if let Some(handle) = &self.mesh_handle {
                            let report = handle
                                .broadcast(ramshield_mesh::MeshMessage::Block(delta))
                                .await;
                            for peer in report.peers.iter().filter(|peer| {
                                peer.outcome != ramshield_mesh::transport::PeerSendOutcome::Written
                            }) {
                                warn!(
                                    peer = %peer.peer,
                                    outcome = ?peer.outcome,
                                    "mesh block was committed locally but broadcast was not delivered"
                                );
                            }
                            if report.task_failures > 0 {
                                warn!(
                                    task_failures = report.task_failures,
                                    "mesh block broadcast tasks failed before reporting outcomes"
                                );
                            }
                        }
                    }
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
                // Mesh deltas are collected under the barrier (CRDT mutation is
                // store-side); gossip IO happens after drop so the std mutex
                // never lives across .await (P0-A: run() future must be Send).
                #[cfg(feature = "mesh")]
                let mut pending_unban_deltas = Vec::new();
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
                    // Ponytail: mesh CRDT unbans — publish so dashboard reflects
                    // live unblock activity. A CIDR unblock is NOT an IP unban.
                    #[cfg(feature = "mesh")]
                    {
                        if self.mesh_blocklist.is_some() {
                            if cmd.source != "mesh" {
                                // These exact tombstones were included in the WAL batch
                                // above and already committed to the CRDT after durability.
                                pending_unban_deltas = prepared_mesh_unblocks.clone();
                            }
                            // Incoming mesh tombstones were already merged and must
                            // not be re-originated or rebroadcast here.
                            self.metrics.inc_mesh_record_unban();
                        }
                        if cmd.source != "mesh" {
                            self.mesh_operator_suppressions.insert(cmd.ip);
                        }
                        self.mesh_applied_ips.remove(&cmd.ip);
                    }
                    // Purge any pending TTL so a later re-block starts clean.
                    self.detach_expiration(cmd.ip);
                    if let Some(shared) = &self.checkpoint_shared {
                        shared.remove_ip_expiration(&cmd.ip);
                    }
                }
                // Barrier protects WAL+store only; mesh IO and XDP stay outside.
                #[cfg(feature = "mesh")]
                if !mesh_wal_entries.is_empty() {
                    self.mesh_projection_pending.remove(&cmd.ip);
                }
                drop(_ckpt_guard);
                #[cfg(feature = "mesh")]
                if !pending_unban_deltas.is_empty() {
                    if let Some(handle) = &self.mesh_handle {
                        for delta in pending_unban_deltas {
                            let report = handle
                                .broadcast(ramshield_mesh::MeshMessage::Unblock(delta))
                                .await;
                            for peer in report.peers.iter().filter(|peer| {
                                peer.outcome != ramshield_mesh::transport::PeerSendOutcome::Written
                            }) {
                                warn!(
                                    peer = %peer.peer,
                                    outcome = ?peer.outcome,
                                    "mesh unblock was committed locally but broadcast was not delivered"
                                );
                            }
                            if report.task_failures > 0 {
                                warn!(
                                    task_failures = report.task_failures,
                                    "mesh unblock broadcast tasks failed before reporting outcomes"
                                );
                            }
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
