use crate::*;

/// Whether the durable WAL record for a decision already exists.
///
/// A mesh delta is journalled with its origin dot when it is first applied,
/// so a later projection repair must not append a second, provenance-free
/// record for the same dot. That contract is stated by the caller instead of
/// being inferred from the free-form `source` label: a decision that merely
/// claims the `mesh` source would otherwise silently never reach the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(feature = "mesh"), allow(dead_code))]
pub(crate) enum WalDurability {
    /// Append a WAL record for this decision.
    Append,
    /// The decision is already durable; only repair the projection.
    AlreadyDurable,
}

/// A mesh mutation received from a peer.
///
/// The mutation carries the remote origin dot so the event is persisted to the
/// WAL and merged into the CRDT exactly once, with provenance intact.
#[derive(Clone, Debug)]
#[cfg_attr(not(feature = "mesh"), allow(dead_code))]
pub(crate) enum PendingMeshMutation {
    Block {
        ip: std::net::IpAddr,
        node_id: u32,
        incarnation: uuid::Uuid,
        counter: u64,
        created_at_ms: u64,
        expires_at_ms: Option<u64>,
        tier: u8,
    },
    Unblock {
        ip: std::net::IpAddr,
        node_id: u32,
        incarnation: uuid::Uuid,
        counter: u64,
    },
}

impl PendingMeshMutation {
    /// Encode the mutation as a WAL entry that preserves the origin dot.
    ///
    /// Only the mesh path encodes mutations, so the helper is dead code in
    /// a build without the `mesh` feature.
    #[cfg_attr(not(feature = "mesh"), allow(dead_code))]
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
    fn append_wal(&mut self, cmd: &EnforceCommand) -> Result<Option<u64>, EnforcementError> {
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
            // Make the record durable before anything else observes it.
            //
            // The caller merges this decision into the CRDT and then into the
            // store and the XDP map. If the record is not durable yet, a crash
            // can leave the CRDT and the store AHEAD of the log — and
            // recovery, which rebuilds both from the log, would then silently
            // drop the block.
            //
            // The mesh path already does this (`append_mesh_wal` syncs before
            // its merge). This is the local path's equivalent, and it was
            // missing: the local block was merged into the CRDT while its WAL
            // record was still only in the writer's buffer.
            //
            // `append_is_durable()` keeps the cost honest: under `SyncEach`
            // the append already fsynced, and a second fsync would be pure
            // waste on the hot path.
            if !wal.append_is_durable() {
                wal.sync()
                    .map_err(|e| EnforcementError::Wal(e.to_string()))?;
            }
            self.last_wal_lsn = Some(lsn);
            Some(lsn)
        } else {
            None
        };
        Ok(wal_lsn)
    }

    /// Append a mesh mutation to the WAL with its original CRDT dot.
    ///
    /// The record is synced before the CRDT merge so a crash can never leave
    /// the CRDT ahead of the durable log.
    #[cfg(feature = "mesh")]
    fn append_mesh_wal(
        &mut self,
        mutation: &PendingMeshMutation,
    ) -> Result<Option<u64>, EnforcementError> {
        let Some(ref wal) = self.wal else {
            return Ok(None);
        };
        let ts_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let entry = mutation.to_wal_entry(ts_ns);
        let lsn = wal
            .append(&entry)
            .map_err(|e| EnforcementError::Wal(e.to_string()))?;
        wal.sync()
            .map_err(|e| EnforcementError::Wal(e.to_string()))?;
        self.last_wal_lsn = Some(lsn);
        Ok(Some(lsn))
    }

    fn commit_block_state(
        &mut self,
        cmd: &EnforceCommand,
        now_ns: u64,
    ) -> Result<(), EnforcementError> {
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
                    shared.set_cidr_expiration(network, unix_ns_from_instant(at, now_unix_ns));
                }
            } else {
                self.cidr_expirations.remove(&network);
                if let Some(shared) = &self.checkpoint_shared {
                    shared.remove_cidr_expiration(&network);
                }
            }
        } else {
            // Explicit IP detention.
            //
            // F-9: this used to be `get()` -> mutate -> `insert()`, two
            // lock acquisitions. A detection flush that ran in between
            // advanced the record's counters and the block then published
            // its stale snapshot back, rewinding the attacker's evidence.
            // `block_ip` changes only the block state, under one lock.
            //
            // This record is published only when there is no live one.
            // Its stat fields are zero: an IP with no history has none.
            let rec = IpRecord {
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
                block_state: BlockState::Blocked {
                    reason,
                    since_ns: now_ns,
                },
            };

            // Do not let Store's passive expiry hide a still-blocked record.
            self.store
                .block_ip(
                    cmd.ip,
                    rec,
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
        Ok(())
    }

    async fn apply_block_projection(&mut self, cmd: &EnforceCommand) -> bool {
        let is_cidr = cmd.cidr.is_some();
        let xdp_applied = match cmd.cidr {
            Some(network) => self
                .xdp
                .apply_cidr_block(network, cmd.decision_id, cmd.ttl_seconds),
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
            if let (Some(mesh), Some(handle)) = (&self.mesh_blocklist, &self.mesh_handle) {
                let ttl_ms = cmd.ttl_seconds.saturating_mul(1000);
                let delta = mesh.record_ban(cmd.ip, ttl_ms, 2);
                self.metrics.inc_mesh_record_ban();
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
        xdp_applied
    }

    pub async fn enforce(
        &mut self,
        cmd: EnforceCommand,
    ) -> Result<EnforceResult, EnforcementError> {
        self.enforce_inner(cmd, None, WalDurability::Append).await
    }

    /// Apply a mesh mutation received from a peer.
    ///
    /// The remote origin dot is persisted to the WAL and merged into the CRDT
    /// inside the checkpoint barrier, before any projection is touched, so a
    /// crash can never leave the WAL and the CRDT disagreeing.
    #[cfg(feature = "mesh")]
    pub(crate) async fn enforce_mesh_mutation(
        &mut self,
        cmd: EnforceCommand,
        mutation: PendingMeshMutation,
    ) -> Result<EnforceResult, EnforcementError> {
        self.enforce_inner(cmd, Some(mutation), WalDurability::Append)
            .await
    }

    /// Re-project a mesh decision whose CRDT dot is already durable.
    ///
    /// The delta was journalled with its origin dot when it was first
    /// applied, so this call must not append a second, provenance-free record
    /// for the same dot. The durability contract is stated here instead of
    /// being inferred from the `source` label, so a decision that only claims
    /// the `mesh` source can never silently skip the log.
    #[cfg(feature = "mesh")]
    pub(crate) async fn enforce_durable_mesh_projection(
        &mut self,
        cmd: EnforceCommand,
    ) -> Result<EnforceResult, EnforcementError> {
        self.enforce_inner(cmd, None, WalDurability::AlreadyDurable)
            .await
    }

    #[cfg_attr(not(feature = "mesh"), allow(unused_variables))]
    async fn enforce_inner(
        &mut self,
        cmd: EnforceCommand,
        incoming_mesh_mutation: Option<PendingMeshMutation>,
        wal_durability: WalDurability,
    ) -> Result<EnforceResult, EnforcementError> {
        // ┌──────────────────────────────────────────────────────────────────────┐
        // │ Section 0: Idempotent replay guard — return cached result if        │
        // │ already processed. A fabricated `xdp_applied: true` would hide a    │
        // │ dataplane failure from every retry consumer.                        │
        // └──────────────────────────────────────────────────────────────────────┘
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

        // ┌──────────────────────────────────────────────────────────────────────┐
        // │ Section 1: Checkpoint barrier — serialize WAL append + store        │
        // │ mutation against checkpoint loop. Prevents silent block loss on     │
        // │ crash recovery. Steps 1-2 hold no await; XDP (step 3) stays outside │
        // │ barrier.                                                            │
        // └──────────────────────────────────────────────────────────────────────┘
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

        // Step 1: commit intent to WAL (durable) — before any state change.
        // A mesh mutation is persisted with its original CRDT dot so recovery
        // can rebuild the CRDT from the log; a second, provenance-free record
        // is never written for it.
        #[cfg(feature = "mesh")]
        let wal_lsn = match incoming_mesh_mutation.as_ref() {
            Some(mutation) => self.append_mesh_wal(mutation)?,
            // The caller states that this decision is already durable (a mesh
            // delta that was journalled with its origin dot when it was first
            // applied), so only the projection is repaired here.
            None if wal_durability == WalDurability::AlreadyDurable => None,
            None => self.append_wal(&cmd)?,
        };
        #[cfg(not(feature = "mesh"))]
        let wal_lsn = self.append_wal(&cmd)?;

        // Once the event is durable, publish the exact same operation into the
        // CRDT before touching projections. If a later store/XDP operation
        // fails, WAL and mesh still agree and reconciliation can repair the
        // projection.
        #[cfg(feature = "mesh")]
        if let Some(mesh) = &self.mesh_blocklist
            && let Some(mutation) = incoming_mesh_mutation.as_ref()
        {
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

        // Step 2: storage mutation.
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        match cmd.action {
            EnforceAction::Block => {
                self.commit_block_state(&cmd, now_ns)?;

                drop(_ckpt_guard);
                // Step 3: dataplane (barrier released — XDP stays outside).
                let xdp_applied = self.apply_block_projection(&cmd).await;
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
                    // F-9: the mirror of the block path. `unblock_ip`
                    // clears the block state under one lock and leaves
                    // the stat fields alone, so a detection flush in
                    // between is not rolled back. A missing or expired
                    // record is left alone: an unblock must not create
                    // a record for an IP there is no state for.
                    self.store.unblock_ip(&cmd.ip);
                    self.blocked_ips.remove(&cmd.ip);
                    self.drops_by_blocked.remove(&cmd.ip);
                    // Ponytail: mesh CRDT unbans — publish so dashboard reflects
                    // live unblock activity. A CIDR unblock is NOT an IP unban.
                    #[cfg(feature = "mesh")]
                    {
                        if let Some(mesh) = &self.mesh_blocklist {
                            if cmd.source != "mesh" {
                                // A local/operator unblock originates its own
                                // tombstone and publishes it to the peers.
                                pending_unban_deltas = mesh.record_unban(cmd.ip);
                            }
                            // An incoming mesh tombstone was already merged with
                            // its origin dot and must not be re-originated here.
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
                drop(_ckpt_guard);
                #[cfg(feature = "mesh")]
                if !pending_unban_deltas.is_empty()
                    && let Some(handle) = &self.mesh_handle
                {
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
