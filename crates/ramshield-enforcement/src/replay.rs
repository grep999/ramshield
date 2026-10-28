use super::*;

/// Replay WAL entries into the store: fold BlockIp/UnblockIp in LSN order to
/// the final block set, skipping blocks whose TTL already elapsed. Returns
/// the still-live blocks as `(ip, ttl_secs)` pairs — the caller re-arms the
/// enforcement TTL schedule with them (P1-4: blocks restored WITHOUT an
/// expiry card never expire; expirations/buckets are empty at boot).
/// Call before `run()` so the XDP reconciliation inside it picks the
/// recovered state up.
pub fn replay_wal_into_store(
    store: &Arc<Store>,
    wal: &Wal,
    min_lsn: u64,
) -> anyhow::Result<Vec<(IpAddr, u64)>> {
    replay_wal_into_store_seeded(store, wal, min_lsn, std::collections::HashMap::new())
}

/// Checkpoint-accelerated variant: the fold STARTS from `seed` (snapshot
/// block state: ip → (reason, since_ns, deadline_ns)) so an IP blocked in
/// the snapshot and untouched by the tail remains blocked, while a tail
/// UnblockIp correctly removes it. Tail blocks/Unblocks then fold on top.
pub fn replay_wal_into_store_seeded(
    store: &Arc<Store>,
    wal: &Wal,
    min_lsn: u64,
    seed: std::collections::HashMap<IpAddr, (String, u64, u64)>,
) -> anyhow::Result<Vec<(IpAddr, u64)>> {
    replay_wal_into_store_seeded_internal(store, wal, min_lsn, seed, None)
}

#[cfg(feature = "mesh")]
pub fn replay_wal_into_store_seeded_with_mesh(
    store: &Arc<Store>,
    wal: &Wal,
    min_lsn: u64,
    seed: std::collections::HashMap<IpAddr, (String, u64, u64)>,
    mesh_seed: Option<&ramshield_mesh::aworset::AworsetSnapshot>,
) -> anyhow::Result<Vec<(IpAddr, u64)>> {
    let mesh_seed = mesh_seed.map(|snapshot| MeshReplaySeed {
        blocks: snapshot
            .blocks
            .iter()
            .map(|delta| MeshReplayBlock {
                ip: delta.ip,
                node_id: delta.dot.node_id,
                incarnation: delta.dot.incarnation.to_string(),
                counter: delta.dot.counter,
                created_at_ms: delta.created_at_ms,
                expires_at_ms: (delta.expires_at_ms != u64::MAX).then_some(delta.expires_at_ms),
                tier: delta.tier,
            })
            .collect(),
        unblocks: snapshot
            .unblocks
            .iter()
            .map(|delta| MeshReplayUnblock {
                ip: delta.ip,
                node_id: delta.dot.node_id,
                incarnation: delta.dot.incarnation.to_string(),
                counter: delta.dot.counter,
            })
            .collect(),
    });
    replay_wal_into_store_seeded_internal(store, wal, min_lsn, seed, mesh_seed)
}

fn replay_wal_into_store_seeded_internal(
    store: &Arc<Store>,
    wal: &Wal,
    min_lsn: u64,
    seed: std::collections::HashMap<IpAddr, (String, u64, u64)>,
    mesh_seed: Option<MeshReplaySeed>,
) -> anyhow::Result<Vec<(IpAddr, u64)>> {
    let entries = if min_lsn > 0 {
        wal.replay_from(min_lsn)?
    } else {
        wal.replay()?
    };
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    let seed_ips: Vec<IpAddr> = seed.keys().copied().collect();
    // Reconstruct mesh provenance from the checkpoint seed plus the WAL tail.
    // Without a mesh seed, use the full retained WAL and let boot enforce that
    // retention has not pruned required history.
    let (mesh_touched, mesh_active) = replay_mesh_state(wal, min_lsn, mesh_seed.as_ref(), now_ns)?;
    // Keep absolute deadlines throughout the fold. Converting a snapshot
    // deadline to whole seconds here loses precision and can resurrect a
    // nearly-expired block. Only convert to remaining seconds at the final
    // scheduler hand-off.
    let mut blocked: std::collections::HashMap<IpAddr, (BlockReason, u64, Option<u64>)> = seed
        .into_iter()
        .map(|(ip, (reason, since, deadline_ns))| {
            let deadline = (deadline_ns > 0).then_some(deadline_ns);
            (ip, (reason_to_block_reason(&reason), since, deadline))
        })
        .collect();
    for entry in entries {
        match entry {
            WalEntry::BlockIp {
                ip,
                reason,
                ttl_secs,
                ts_ns,
            } => {
                if let Ok(ip) = ip.parse() {
                    let deadline = ttl_secs
                        .map(|secs| ts_ns.saturating_add(secs.saturating_mul(1_000_000_000)));
                    blocked.insert(ip, (reason_to_block_reason(&reason), ts_ns, deadline));
                }
            }
            WalEntry::UnblockIp { ip, .. } => {
                if let Ok(ip) = ip.parse() {
                    blocked.remove(&ip);
                }
            }
            // MeshMutation records are folded separately by origin dot below.
            // Folding them into this aggregate map would let one peer's tombstone
            // erase a concurrent block from another peer.
            WalEntry::MeshMutation { .. } => {}
            _ => {} // Insert/Delete/Checkpoint: traffic data, not block state
        }
    }

    // For any IP represented in mesh history, replace the snapshot's
    // aggregate mesh projection with the fold of all retained origin dots.
    for ip in mesh_touched {
        blocked.remove(&ip);
    }
    for (ip, state) in mesh_active {
        blocked.insert(ip, state);
    }

    let ram_lim = store.traffic.ram_limit_mb.load(Ordering::Relaxed).max(1) * 1024 * 1024;
    // Seed reconciliation: snapshot IPs that the tail UNBLOCKED must have their
    // store records (inserted by restore_from_snapshot) removed — the fold's
    // final loop only touches surviving entries, so without this a tail unblock
    // of a snapshot IP would leave a phantom block.
    for seed_ip in seed_ips {
        if !blocked.contains_key(&seed_ip) {
            store.remove(&seed_ip);
        }
    }
    let mut restored: Vec<(IpAddr, u64)> = Vec::new();
    for (ip, (reason, ts_ns, deadline_ns)) in blocked {
        // Expired absolute deadline → don't resurrect.
        if let Some(deadline) = deadline_ns
            && deadline <= now_ns
        {
            continue;
        }
        let rec = match store.get(&ip) {
            Some(Value::IpRecord(mut r)) => {
                r.block_state = BlockState::Blocked {
                    reason,
                    since_ns: ts_ns,
                },;
                r
            }
            _ => IpRecord {
                ip,
                request_count: 0,
                ewma_rps: 0.0,
                cusum_s: 0.0,
                baseline_rps: 0.0,
                prev_sample_hot: false,
                sample_count: 0,
                relative_breach_streak: 0,
                pulse_samples_in_window: 0,
                pulse_window_start_ns: 0,
                adaptive_baseline_rps: 0.0,
                adaptive_cusum_s: 0.0,
                adaptive_sample_count: 0,
                adaptive_bucket_id: 0,
                adaptive_bucket_packets: 0,
                adaptive_bucket_initialized: false,
                variance: 0.1,
                last_bucket_ns: 0,
                recovery_count: 0,
                bucket_accum_pps: 0.0,
                detector_mode: 0,
                adaptive_model_sample_count: 0,
                first_seen_ns: ts_ns,
                last_seen_ns: ts_ns,
                bytes_in: 0,
                status_dist: [0; 5],
                proto_fingerprint: 0,
                threat_score: 0.0,
                block_state: BlockState::Blocked {
                    reason,
                    since_ns: ts_ns,
                },
            },
        };
        store
            .insert(ip, Value::IpRecord(rec), None, ram_lim)
            .map_err(|e| anyhow::anyhow!("WAL replay insert {ip}: {e}"))?;
        // Scheduler APIs use whole seconds, so round UP only at this final
        // boundary. The authoritative expiration check above used the exact
        // absolute deadline. This avoids extending a block during state fold.
        let remaining = deadline_ns
            .map(|deadline| deadline.saturating_sub(now_ns).div_ceil(1_000_000_000))
            .unwrap_or(0);
        restored.push((ip, remaining));
    }
    info!(
        "WAL replay: restored {} live blocks ({} with TTL)",
        restored.len(),
        restored.iter().filter(|(_, t)| *t > 0).count()
    );
    Ok(restored)
}

/// Reconstruct the effective mesh IP projection from the full retained
/// origin-dot history. Checkpoint snapshots currently do not carry CRDT
/// provenance, so callers must refuse startup if retention has pruned history.
type MeshReplayProjection = (
    std::collections::HashSet<IpAddr>,
    std::collections::HashMap<IpAddr, (BlockReason, u64, Option<u64>)>,
);

#[derive(Clone, Default)]
struct MeshReplaySeed {
    blocks: Vec<MeshReplayBlock>,
    unblocks: Vec<MeshReplayUnblock>,
}

#[derive(Clone)]
struct MeshReplayBlock {
    ip: IpAddr,
    node_id: u32,
    incarnation: String,
    counter: u64,
    created_at_ms: u64,
    expires_at_ms: Option<u64>,
    tier: u8,
}

#[derive(Clone)]
struct MeshReplayUnblock {
    ip: IpAddr,
    node_id: u32,
    incarnation: String,
    counter: u64,
}

fn replay_mesh_state(
    wal: &Wal,
    min_lsn: u64,
    seed: Option<&MeshReplaySeed>,
    now_ns: u64,
) -> anyhow::Result<MeshReplayProjection> {
    type MeshKey = (IpAddr, u32, String);
    type ActiveMeshDot = (u64, u64, Option<u64>, u8);

    let mut touched = std::collections::HashSet::new();
    let mut active: std::collections::HashMap<MeshKey, ActiveMeshDot> =
        std::collections::HashMap::new();
    let mut tombstones: std::collections::HashMap<MeshKey, u64> = std::collections::HashMap::new();

    if let Some(seed) = seed {
        for delta in &seed.unblocks {
            let key = (delta.ip, delta.node_id, delta.incarnation.clone());
            touched.insert(delta.ip);
            tombstones
                .entry(key)
                .and_modify(|counter| *counter = (*counter).max(delta.counter))
                .or_insert(delta.counter);
        }
        for delta in &seed.blocks {
            let key = (delta.ip, delta.node_id, delta.incarnation.clone());
            touched.insert(delta.ip);
            if delta.counter > tombstones.get(&key).copied().unwrap_or(0) {
                active.insert(
                    key,
                    (
                        delta.counter,
                        delta.created_at_ms.saturating_mul(1_000_000),
                        delta.expires_at_ms.map(|ms| ms.saturating_mul(1_000_000)),
                        delta.tier,
                    ),
                );
            }
        }
    }

    let entries = if seed.is_some() {
        wal.replay_from(min_lsn)?
    } else {
        wal.replay()?
    };
    for entry in entries {
        let WalEntry::MeshMutation {
            schema_version,
            origin_node_id,
            origin_incarnation,
            origin_counter,
            ip,
            action,
            created_at_ms,
            expires_at_ms,
            tier,
            ..
        } = entry
        else {
            continue;
        };
        if schema_version != 1 || origin_node_id == 0 || origin_counter == 0 {
            anyhow::bail!(
                "unsupported or invalid mesh WAL event: schema={schema_version}, node={origin_node_id}, counter={origin_counter}"
            );
        }
        let ip: IpAddr = ip
            .parse()
            .map_err(|error| anyhow::anyhow!("invalid IP in mesh WAL event: {error}"))?;
        if ip.is_unspecified() {
            anyhow::bail!("unspecified IP in mesh WAL event");
        }
        let incarnation = uuid::Uuid::parse_str(&origin_incarnation).map_err(|error| {
            anyhow::anyhow!("invalid origin incarnation in mesh WAL event: {error}")
        })?;
        if incarnation.is_nil() {
            anyhow::bail!("nil origin incarnation in mesh WAL event");
        }
        touched.insert(ip);
        let key = (ip, origin_node_id, incarnation.to_string());
        match action {
            ramshield_storage::wal::MeshMutationAction::Block => {
                if origin_counter > tombstones.get(&key).copied().unwrap_or(0)
                    && active
                        .get(&key)
                        .is_none_or(|existing| origin_counter > existing.0)
                {
                    active.insert(
                        key,
                        (
                            origin_counter,
                            created_at_ms.saturating_mul(1_000_000),
                            expires_at_ms.map(|ms| ms.saturating_mul(1_000_000)),
                            tier,
                        ),
                    );
                }
            }
            ramshield_storage::wal::MeshMutationAction::Unblock => {
                let watermark = tombstones.entry(key.clone()).or_insert(0);
                *watermark = (*watermark).max(origin_counter);
                if active
                    .get(&key)
                    .is_some_and(|existing| existing.0 <= origin_counter)
                {
                    active.remove(&key);
                }
            }
        }
    }

    let mut by_ip: std::collections::HashMap<IpAddr, (BlockReason, u64, Option<u64>)> =
        std::collections::HashMap::new();
    for ((ip, _, _), (_, since_ns, deadline_ns, _tier)) in active {
        if deadline_ns.is_some_and(|deadline| deadline <= now_ns) {
            continue;
        }
        let candidate = (
            BlockReason::from_reason_str("mesh_final").unwrap_or(BlockReason::ManualBlock),
            since_ns,
            deadline_ns,
        );
        by_ip
            .entry(ip)
            .and_modify(|existing| {
                if candidate.1 > existing.1 {
                    *existing = candidate;
                }
            })
            .or_insert(candidate);
    }
    Ok((touched, by_ip))
}

pub fn replay_wal_cidrs(wal: &Wal) -> anyhow::Result<Vec<(IpNetwork, u64)>> {
    replay_wal_cidrs_seeded(wal, 0, std::collections::HashMap::new()).map(|r| r.0)
}

pub fn replay_wal_cidrs_from(wal: &Wal, min_lsn: u64) -> anyhow::Result<Vec<(IpNetwork, u64)>> {
    replay_wal_cidrs_seeded(wal, min_lsn, std::collections::HashMap::new()).map(|r| r.0)
}

/// Replay CIDR state from a checkpoint seed plus the WAL tail. The returned
/// second value is the final authoritative CIDR set; callers restoring a
/// snapshot must remove seeded CIDRs absent from this set before re-arming
/// the XDP projection. Deadlines remain absolute until the final scheduler
/// hand-off, matching the IP recovery path.
#[allow(clippy::type_complexity)] // ponytail: two parallel collections stay local; alias if callers grow.
pub fn replay_wal_cidrs_seeded(
    wal: &Wal,
    min_lsn: u64,
    seed: std::collections::HashMap<IpNetwork, Option<u64>>,
) -> anyhow::Result<(Vec<(IpNetwork, u64)>, std::collections::HashSet<IpNetwork>)> {
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut blocked: std::collections::HashMap<IpNetwork, (u64, Option<u64>)> = seed
        .into_iter()
        .filter(|(_, deadline)| deadline.map(|d| d > now_ns).unwrap_or(true))
        .map(|(network, deadline)| (network, (0, deadline)))
        .collect();
    let entries = if min_lsn > 0 {
        wal.replay_from(min_lsn)?
    } else {
        wal.replay()?
    };
    for entry in entries {
        match entry {
            WalEntry::BlockCidr {
                cidr,
                ttl_secs,
                ts_ns,
                ..
            } => {
                let deadline =
                    ttl_secs.map(|secs| ts_ns.saturating_add(secs.saturating_mul(1_000_000_000)));
                blocked.insert(cidr, (ts_ns, deadline));
            }
            WalEntry::UnblockCidr { cidr, .. } => {
                blocked.remove(&cidr);
            }
            _ => {}
        }
    }

    let mut final_cidrs = std::collections::HashSet::new();
    let mut restored = Vec::new();
    for (cidr, (_ts_ns, deadline)) in blocked {
        if let Some(deadline) = deadline {
            if deadline <= now_ns {
                continue;
            }
            final_cidrs.insert(cidr);
            let remaining = deadline.saturating_sub(now_ns).div_ceil(1_000_000_000);
            restored.push((cidr, remaining));
        } else {
            final_cidrs.insert(cidr);
            restored.push((cidr, 0));
        }
    }
    Ok((restored, final_cidrs))
}

#[cfg(test)]
mod mesh_wal_replay_tests {
    use super::{MeshReplayBlock, MeshReplaySeed, replay_mesh_state};
    use ramshield_storage::wal::{MeshMutationAction, Wal, WalEntry};
    use ramshield_types::Durability;
    use std::net::{IpAddr, Ipv4Addr};

    fn event(
        ip: IpAddr,
        node_id: u32,
        incarnation: uuid::Uuid,
        counter: u64,
        action: MeshMutationAction,
    ) -> WalEntry {
        WalEntry::MeshMutation {
            schema_version: 1,
            origin_node_id: node_id,
            origin_incarnation: incarnation.to_string(),
            origin_counter: counter,
            ip: ip.to_string(),
            action,
            created_at_ms: 1_800_000_000_000,
            expires_at_ms: None,
            tier: 2,
            signature: None,
            ts_ns: 1_800_000_000_000_000_000,
        }
    }

    #[test]
    fn checkpoint_seed_plus_wal_tail_preserves_other_origin_dot() {
        let dir =
            std::env::temp_dir().join(format!("ramshield-mesh-tail-{}", uuid::Uuid::new_v4()));
        let dir_string = dir.to_string_lossy().into_owned();
        let wal = Wal::open(&dir_string, false, Durability::Fsync, 1 << 20, 1 << 24).unwrap();
        let ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 103));
        let origin_a = uuid::Uuid::new_v4();
        let origin_b = uuid::Uuid::new_v4();
        let created_at_ms = 1_800_000_000_000;

        wal.append(&event(ip, 11, origin_a, 1, MeshMutationAction::Unblock))
            .unwrap();
        wal.sync().unwrap();

        let seed = MeshReplaySeed {
            blocks: vec![
                MeshReplayBlock {
                    ip,
                    node_id: 11,
                    incarnation: origin_a.to_string(),
                    counter: 1,
                    created_at_ms,
                    expires_at_ms: None,
                    tier: 1,
                },
                MeshReplayBlock {
                    ip,
                    node_id: 22,
                    incarnation: origin_b.to_string(),
                    counter: 1,
                    created_at_ms,
                    expires_at_ms: None,
                    tier: 2,
                },
            ],
            unblocks: Vec::new(),
        };
        let (touched, active) =
            replay_mesh_state(&wal, 0, Some(&seed), 1_800_000_000_000_000_000).unwrap();
        assert!(touched.contains(&ip));
        assert_eq!(active.len(), 1);
        assert!(active.contains_key(&ip));
        drop(wal);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unblock_only_removes_the_matching_origin_dot() {
        let dir =
            std::env::temp_dir().join(format!("ramshield-mesh-replay-{}", uuid::Uuid::new_v4()));
        let dir_string = dir.to_string_lossy().into_owned();
        let wal = Wal::open(&dir_string, false, Durability::Fsync, 1 << 20, 1 << 24).unwrap();
        let ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 99));
        let origin_a = uuid::Uuid::new_v4();
        let origin_b = uuid::Uuid::new_v4();

        for entry in [
            event(ip, 11, origin_a, 1, MeshMutationAction::Block),
            event(ip, 22, origin_b, 1, MeshMutationAction::Block),
            event(ip, 11, origin_a, 1, MeshMutationAction::Unblock),
        ] {
            wal.append(&entry).unwrap();
        }
        wal.sync().unwrap();

        let (touched, active) =
            replay_mesh_state(&wal, 0, None, 1_800_000_000_000_000_000).unwrap();
        assert!(touched.contains(&ip));
        assert!(
            active.contains_key(&ip),
            "origin B's concurrent block must survive origin A's tombstone"
        );

        wal.append(&event(ip, 22, origin_b, 1, MeshMutationAction::Unblock))
            .unwrap();
        wal.sync().unwrap();
        let (_, active) = replay_mesh_state(&wal, 0, None, 1_800_000_000_000_000_000).unwrap();
        assert!(
            !active.contains_key(&ip),
            "the IP becomes clear only after both active origin dots are removed"
        );
        drop(wal);
        let _ = std::fs::remove_dir_all(dir);
    }
}
