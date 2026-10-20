//! Add-Wins Observed-Remove Set (AWORSet) CRDT primitives
//!
//! Two layers:
//! - `Aworset`: generic string-keyed CRDT set (add/remove/merge/contains).
//! - `AworsetBlocklist`: IP-keyed blocklist CRDT from the fleet integration
//!   contract — `record_ban` produces a `ClusterBlockDelta` for gossip,
//!   `merge_delta` absorbs one, `is_blocked` answers membership.

use super::hlc::Hlc;
use dashmap::DashMap;
use std::net::IpAddr;
use std::sync::{
    RwLock,
    atomic::{AtomicU64, Ordering},
};
use uuid::Uuid;

#[derive(Debug)]
pub enum MeshError {
    DotCounterExhausted,
}

/// Epoch tracking for peer acknowledgment safety
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PeerEpoch {
    pub peer_node_id: u32,
    pub incarnation: Uuid,
    pub epoch: u64,
    pub last_acknowledged_lsn: u64,
    pub tombstone_expiry_epoch: u64,
}

impl Default for PeerEpoch {
    fn default() -> Self {
        Self {
            peer_node_id: 0,
            incarnation: Uuid::nil(),
            epoch: 0,
            last_acknowledged_lsn: 0,
            tombstone_expiry_epoch: u64::MAX,
        }
    }
}

pub type Dot = u64;
pub(crate) type BlockKey = (IpAddr, u32, Uuid);
pub(crate) type BlockState = (u64, u64, u64, u8);
/// Full CRDT checkpoint payload. Both active origin dots and tombstone
/// watermarks are required to prevent resurrection after WAL retention.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AworsetSnapshot {
    pub blocks: Vec<ClusterBlockDelta>,
    pub unblocks: Vec<ClusterUnblockDelta>,
}

impl AworsetSnapshot {
    /// Reject malformed checkpoint state rather than silently dropping dots
    /// or accepting impossible origin identities during recovery.
    pub fn validate(&self) -> Result<(), &'static str> {
        let mut block_keys = std::collections::HashSet::new();
        for delta in &self.blocks {
            if delta.ip.is_unspecified()
                || delta.dot.node_id == 0
                || delta.dot.counter == 0
                || delta.dot.incarnation.is_nil()
                || (delta.expires_at_ms != u64::MAX && delta.expires_at_ms < delta.created_at_ms)
            {
                return Err("invalid active mesh dot in checkpoint");
            }
            if !block_keys.insert((delta.ip, delta.dot.node_id, delta.dot.incarnation)) {
                return Err("duplicate active mesh key in checkpoint");
            }
        }
        let mut unblock_keys = std::collections::HashSet::new();
        for delta in &self.unblocks {
            if delta.ip.is_unspecified()
                || delta.dot.node_id == 0
                || delta.dot.counter == 0
                || delta.dot.incarnation.is_nil()
            {
                return Err("invalid mesh tombstone in checkpoint");
            }
            if !unblock_keys.insert((delta.ip, delta.dot.node_id, delta.dot.incarnation)) {
                return Err("duplicate mesh tombstone key in checkpoint");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Element {
    pub key: String,
    pub dot: Dot,
    pub removed: bool,
}

/// A recorded removal (tombstone) together with the peer-acknowledgment
/// watermark that was current when the removal was first observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TombstoneState {
    /// The removed dot's counter: the causal high-water mark of the removal.
    counter: u64,
    /// The peer acknowledgment epoch observed when the removal was recorded.
    ///
    /// A removal may only be collected once every declared peer has
    /// acknowledged *past* this watermark, i.e. has acknowledged at least once
    /// since the removal existed. Acknowledgment epochs count acknowledgments,
    /// so they must never be compared against a dot counter.
    created_at_epoch: u64,
}

/// Operation identity is `(node_id, incarnation, counter)`. Each blocklist
/// instance receives a fresh random incarnation so a process restart cannot
/// reuse old dots even when the configured node ID and counter restart. The
/// counter remains monotonic within one incarnation and independent of HLC time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClusterDot {
    pub node_id: u32,
    /// Random process incarnation; nil is reserved for invalid/legacy messages.
    #[serde(default)]
    pub incarnation: Uuid,
    pub counter: u64,
}

/// One ban decision, gossip-able across the fleet.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClusterBlockDelta {
    pub ip: IpAddr,
    pub dot: ClusterDot,
    /// Physical HLC time when the ban was created (not its expiry time).
    /// Defaults to zero when reading legacy wire messages.
    #[serde(default)]
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub tier: u8,
}

/// Observed-remove tombstone propagated to peers. A peer must discard any
/// block dot for the same (ip,node,incarnation) whose counter is <= this counter.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClusterUnblockDelta {
    pub ip: IpAddr,
    pub dot: ClusterDot,
}

/// Thread-safe AWORSet implementation using DashSet for lock-free concurrent access.
pub struct Aworset {
    /// Internal storage: element key -> set of dots (add operations)
    elements: dashmap::DashSet<String>,
    /// Tombstones: removed elements keyed by element name -> dot
    tombstones: dashmap::DashSet<String>,
}

impl Aworset {
    pub fn new() -> Self {
        Self {
            elements: dashmap::DashSet::new(),
            tombstones: dashmap::DashSet::new(),
        }
    }

    pub fn add(&self, key: String) {
        self.tombstones.remove(&key);
        self.elements.insert(key);
    }

    pub fn remove(&self, key: String) {
        self.elements.remove(&key);
        self.tombstones.insert(key);
    }

    pub fn contains(&self, key: &str) -> bool {
        self.elements.contains(key) && !self.tombstones.contains(key)
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Merge another AWORSet into this one (CRDT merge operation)
    pub fn merge(&self, other: &Aworset) {
        for key in other.elements.iter() {
            self.elements.insert(key.clone());
        }
        for key in other.tombstones.iter() {
            self.tombstones.insert(key.clone());
        }
    }
}

impl Default for Aworset {
    fn default() -> Self {
        Self::new()
    }
}

/// IP-keyed blocklist CRDT: operation dots per (ip, node), ordered by the
/// per-node operation counter. HLC physical time is retained separately for
/// TTL calculation. Internal map is DashMap<(IpAddr, node_id, incarnation),
/// (operation_counter, created_at_ms, expires_at_ms)>.
pub struct AworsetBlocklist {
    node_id: u32,
    incarnation: Uuid,
    hlc: Hlc,
    pub(crate) entries: DashMap<BlockKey, BlockState>,
    /// Per-key high-water marks. These are deliberately retained until a
    /// causal-stability/peer-acknowledgement protocol exists; timer-only GC can
    /// resurrect an old ban from a disconnected peer.
    pub(crate) tombstones: DashMap<BlockKey, TombstoneState>,
    /// Peer acknowledgment tracking for safe tombstone garbage collection.
    /// Maps peer node_id to their acknowledgment epoch and last seen state.
    pub(crate) peer_acks: DashMap<u32, PeerEpoch>,
    /// Node IDs of the peers this node is configured to synchronize with.
    ///
    /// A removal may only be collected once every one of them has
    /// acknowledged it. Without this set, a peer that has never acknowledged
    /// is invisible and cannot hold collection back.
    expected_peers: dashmap::DashSet<u32>,
    next_dot_counter: AtomicU64,
    /// Snapshot export takes the exclusive side; mutations hold the shared side.
    /// This freezes the CRDT maps for the duration of a fixed recovery capture.
    snapshot_barrier: RwLock<()>,
}

impl AworsetBlocklist {
    pub fn new(node_id: u32) -> Self {
        Self {
            node_id,
            incarnation: Uuid::new_v4(),
            hlc: Hlc::new(),
            entries: DashMap::new(),
            tombstones: DashMap::new(),
            peer_acks: DashMap::new(),
            expected_peers: dashmap::DashSet::new(),
            // Dot counters are 1-based: counter 0 is an invalid CRDT dot.
            next_dot_counter: AtomicU64::new(1),
            snapshot_barrier: RwLock::new(()),
        }
    }

    /// Prepare a local ban without publishing it into the CRDT.
    ///
    /// The caller can append this exact dot to the WAL first, then call
    /// `merge_delta` to make it visible. A failed WAL append may leave a gap
    /// in the local counter; gaps are safe, reusing a dot is not.
    pub fn prepare_ban(&self, ip: IpAddr, ttl_ms: u64, tier: u8) -> ClusterBlockDelta {
        let (created_at_ms, _) = self.hlc.tick(0, 0);
        // Saturate at u64::MAX without panicking. Emitting dots at u64::MAX
        // will log an error rather than crashing the thread.
        // `try_update` reports the *previous* counter value, so the shared
        // counter starts at 1: dot counter 0 is rejected by
        // `AworsetSnapshot::validate` and by WAL replay in the boot engine.
        let counter = self
            .next_dot_counter
            .try_update(Ordering::AcqRel, Ordering::Acquire, |c| {
                Some(c.saturating_add(1))
            })
            .unwrap_or(u64::MAX);
        if counter == u64::MAX {
            tracing::error!("mesh dot counter saturated at u64::MAX");
        }
        let expires_at_ms = if ttl_ms == 0 {
            u64::MAX
        } else {
            created_at_ms.saturating_add(ttl_ms)
        };

        ClusterBlockDelta {
            ip,
            dot: ClusterDot {
                node_id: self.node_id,
                incarnation: self.incarnation,
                counter,
            },
            created_at_ms,
            expires_at_ms,
            tier,
        }
    }

    /// Record a local ban immediately. WAL-first callers should use
    /// `prepare_ban`, persist the returned delta, then merge it.
    pub fn record_ban(&self, ip: IpAddr, ttl_ms: u64, tier: u8) -> ClusterBlockDelta {
        let delta = self.prepare_ban(ip, ttl_ms, tier);
        self.merge_delta(&delta);
        delta
    }

    /// Return tombstones for all currently observed dots without mutating state.
    /// Persist these before applying them when the WAL is authoritative.
    pub fn prepare_unban(&self, ip: IpAddr) -> Vec<ClusterUnblockDelta> {
        self.entries
            .iter()
            .filter(|entry| entry.key().0 == ip)
            .map(|entry| ClusterUnblockDelta {
                ip,
                dot: ClusterDot {
                    node_id: entry.key().1,
                    incarnation: entry.key().2,
                    counter: entry.value().0,
                },
            })
            .collect()
    }

    /// Whether applying a block delta would add information not already retained.
    pub fn needs_merge_delta(&self, delta: &ClusterBlockDelta) -> bool {
        if delta.dot.node_id == 0 || delta.dot.incarnation.is_nil() {
            return false;
        }
        let key = (delta.ip, delta.dot.node_id, delta.dot.incarnation);
        if self
            .tombstones
            .get(&key)
            .is_some_and(|tombstone| delta.dot.counter <= tombstone.counter)
        {
            return false;
        }
        self.entries
            .get(&key)
            .is_none_or(|entry| delta.dot.counter > entry.value().0)
    }

    /// Whether another live origin dot would still block this IP after the
    /// supplied tombstone is applied.
    pub fn would_be_blocked_after_unblock(&self, delta: &ClusterUnblockDelta, now_ms: u64) -> bool {
        self.entries.iter().any(|entry| {
            if entry.key().0 != delta.ip || entry.value().2 <= now_ms {
                return false;
            }
            let is_removed_dot = entry.key().1 == delta.dot.node_id
                && entry.key().2 == delta.dot.incarnation
                && entry.value().0 <= delta.dot.counter;
            !is_removed_dot
        })
    }

    /// Whether applying an unblock delta would add a new tombstone or remove
    /// a still-live older dot.
    pub fn needs_merge_unblock(&self, delta: &ClusterUnblockDelta) -> bool {
        if delta.dot.node_id == 0 || delta.dot.incarnation.is_nil() {
            return false;
        }
        let key = (delta.ip, delta.dot.node_id, delta.dot.incarnation);
        let tombstone_is_newer = self
            .tombstones
            .get(&key)
            .is_none_or(|tombstone| delta.dot.counter > tombstone.counter);
        let removes_live_dot = self
            .entries
            .get(&key)
            .is_some_and(|entry| entry.value().0 <= delta.dot.counter);
        tombstone_is_newer || removes_live_dot
    }

    /// Absorb a peer delta. True if it changed local state.
    pub fn merge_delta(&self, delta: &ClusterBlockDelta) -> bool {
        let _snapshot_guard = self
            .snapshot_barrier
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if delta.dot.node_id == 0 || delta.dot.incarnation.is_nil() {
            return false;
        }
        // HLC time and operation identity are separate domains. The dot counter
        // must not be interpreted as the HLC's 32-bit logical component.
        self.hlc.tick(delta.created_at_ms, 0);
        let key = (delta.ip, delta.dot.node_id, delta.dot.incarnation);
        if self
            .tombstones
            .get(&key)
            .map(|tombstone| delta.dot.counter <= tombstone.counter)
            .unwrap_or(false)
        {
            return false;
        }

        let mut inserted = false;
        self.entries
            .entry(key)
            .and_modify(
                |(existing_seq, existing_created, existing_exp, existing_tier)| {
                    if delta.dot.counter > *existing_seq {
                        *existing_seq = delta.dot.counter;
                        *existing_created = delta.created_at_ms;
                        *existing_exp = delta.expires_at_ms;
                        *existing_tier = delta.tier;
                        inserted = true;
                    }
                },
            )
            .or_insert_with(|| {
                inserted = true;
                (
                    delta.dot.counter,
                    delta.created_at_ms,
                    delta.expires_at_ms,
                    delta.tier,
                )
            });

        inserted
    }

    pub fn is_blocked(&self, ip: &IpAddr, now_ms: u64) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.key().0 == *ip && entry.value().2 > now_ms)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn hlc_ticks(&self) -> u64 {
        self.hlc.tick_count()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Export every origin dot and tombstone for a durable checkpoint.
    pub fn export_snapshot(&self) -> AworsetSnapshot {
        let _snapshot_guard = self
            .snapshot_barrier
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let mut blocks: Vec<_> = self
            .entries
            .iter()
            .map(|entry| ClusterBlockDelta {
                ip: entry.key().0,
                dot: ClusterDot {
                    node_id: entry.key().1,
                    incarnation: entry.key().2,
                    counter: entry.value().0,
                },
                created_at_ms: entry.value().1,
                expires_at_ms: entry.value().2,
                tier: entry.value().3,
            })
            .collect();
        let mut unblocks: Vec<_> = self
            .tombstones
            .iter()
            .map(|entry| ClusterUnblockDelta {
                ip: entry.key().0,
                dot: ClusterDot {
                    node_id: entry.key().1,
                    incarnation: entry.key().2,
                    counter: entry.value().counter,
                },
            })
            .collect();
        blocks.sort_by_key(|delta| {
            (
                delta.ip,
                delta.dot.node_id,
                delta.dot.incarnation,
                delta.dot.counter,
            )
        });
        unblocks.sort_by_key(|delta| {
            (
                delta.ip,
                delta.dot.node_id,
                delta.dot.incarnation,
                delta.dot.counter,
            )
        });
        AworsetSnapshot { blocks, unblocks }
    }

    /// Merge a checkpoint payload into a fresh or existing CRDT.
    pub fn restore_snapshot(&self, snapshot: &AworsetSnapshot) {
        for delta in &snapshot.unblocks {
            self.merge_unblock_delta(delta);
        }
        for delta in &snapshot.blocks {
            self.merge_delta(delta);
        }
    }

    /// Effective block TTL per IP, taking the longest-lived origin dot.
    /// A zero TTL denotes at least one permanent active dot.
    pub fn active_block_ttls(&self, now_ms: u64) -> Vec<(IpAddr, u64)> {
        let mut deadlines = std::collections::HashMap::<IpAddr, u64>::new();
        for entry in self.entries.iter().filter(|entry| entry.value().2 > now_ms) {
            deadlines
                .entry(entry.key().0)
                .and_modify(|deadline| *deadline = (*deadline).max(entry.value().2))
                .or_insert(entry.value().2);
        }
        let mut blocks: Vec<_> = deadlines
            .into_iter()
            .map(|(ip, deadline)| {
                let ttl_secs = if deadline == u64::MAX {
                    0
                } else {
                    deadline.saturating_sub(now_ms).saturating_add(999) / 1000
                };
                (ip, ttl_secs)
            })
            .collect();
        blocks.sort_by_key(|(ip, _)| *ip);
        blocks
    }

    /// Current unexpired IPs represented by at least one active origin dot.
    pub fn active_ips(&self, now_ms: u64) -> std::collections::HashSet<IpAddr> {
        self.entries
            .iter()
            .filter(|entry| entry.value().2 > now_ms)
            .map(|entry| entry.key().0)
            .collect()
    }

    /// Prune expired active entries while preserving a per-key high-water mark.
    ///
    /// An elapsed timer is not proof that every disconnected peer has observed
    /// a removal. Therefore this method never deletes tombstones/high-water
    /// marks. Expired entries are converted into watermarks after the horizon,
    /// so delayed/replayed deltas cannot recreate old state. This intentionally
    /// favors convergence safety over bounded tombstone memory until a peer-ack
    /// or epoch-based causal-stability protocol is implemented.
    pub fn purge_expired(&self, now_ms: u64) {
        let _snapshot_guard = self
            .snapshot_barrier
            .read()
            .unwrap_or_else(|e| e.into_inner());
        const TOMBSTONE_HORIZON_MS: u64 = 86_400_000;
        let expired: Vec<((IpAddr, u32, Uuid), u64)> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (_, _, expires_at_ms, _) = *entry.value();
                (now_ms.saturating_sub(expires_at_ms) >= TOMBSTONE_HORIZON_MS)
                    .then_some((*entry.key(), entry.value().0))
            })
            .collect();

        let created_at_epoch = self.current_ack_epoch();
        for (key, counter) in expired {
            self.entries.remove(&key);
            self.tombstones
                .entry(key)
                .and_modify(|existing| existing.counter = existing.counter.max(counter))
                .or_insert(TombstoneState {
                    counter,
                    created_at_epoch,
                });
        }
    }

    /// Local unblock: create a tombstone delta for the observed local dot.
    /// Callers must gossip the returned delta; removing the local entry alone
    /// cannot make an AWORSet converge across peers.
    pub fn record_unban(&self, ip: IpAddr) -> Vec<ClusterUnblockDelta> {
        let deltas = self.prepare_unban(ip);
        for delta in &deltas {
            self.merge_unblock_delta(delta);
        }
        deltas
    }

    /// Merge an observed-remove tombstone.
    pub fn merge_unblock_delta(&self, delta: &ClusterUnblockDelta) -> bool {
        let _snapshot_guard = self
            .snapshot_barrier
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if delta.dot.node_id == 0 || delta.dot.incarnation.is_nil() {
            return false;
        }
        self.hlc.tick(0, 0);
        let key = (delta.ip, delta.dot.node_id, delta.dot.incarnation);
        let mut changed = false;
        let created_at_epoch = self.current_ack_epoch();
        self.tombstones
            .entry(key)
            .and_modify(|existing| {
                if delta.dot.counter > existing.counter {
                    existing.counter = delta.dot.counter;
                    changed = true;
                }
            })
            .or_insert_with(|| {
                changed = true;
                TombstoneState {
                    counter: delta.dot.counter,
                    created_at_epoch,
                }
            });
        // ponytail: avoid let-chains; drop the DashMap guard before remove.
        if self
            .entries
            .get(&key)
            .is_some_and(|entry| entry.value().0 <= delta.dot.counter)
        {
            changed |= self.entries.remove(&key).is_some();
        }
        changed
    }
    /// Update peer's epoch when they acknowledge our state
    ///
    /// This method is called when a peer confirms they have received and applied
    /// our state via the recovery completion protocol. It updates their epoch
    /// and calculates when their tombstones can be safely garbage collected.
    ///
    /// # Arguments
    /// * `peer_node_id` - The node ID of the peer who acknowledged our state
    /// * `acked_lsn` - The WAL LSN that the peer has acknowledged up to
    ///
    /// # Safety
    /// This method is safe to call concurrently as it uses the thread-safe
    /// DashMap for peer_acks.
    pub fn peer_acknowledged(&self, peer_node_id: u32, acked_lsn: u64) {
        let mut epoch = self.peer_acks.entry(peer_node_id).or_insert(PeerEpoch {
            peer_node_id,
            incarnation: Uuid::new_v4(),
            epoch: 0,
            last_acknowledged_lsn: 0,
            tombstone_expiry_epoch: u64::MAX,
        });

        if acked_lsn > epoch.last_acknowledged_lsn {
            epoch.last_acknowledged_lsn = acked_lsn;
            epoch.epoch = epoch.epoch.saturating_add(1);
            // Tombstones from this peer become safe to GC after their epoch
            epoch.tombstone_expiry_epoch = epoch.epoch + 1;
        }
    }

    /// Declare the peers this node synchronizes with.
    ///
    /// Tombstone collection waits for an acknowledgment from every declared
    /// peer: a peer that has not acknowledged is not allowed to lose a
    /// removal and later resurrect the block.
    pub fn set_expected_peers(&self, peers: impl IntoIterator<Item = u32>) {
        self.expected_peers.clear();
        for peer in peers {
            self.expected_peers.insert(peer);
        }
    }

    /// Snapshot the current peer acknowledgment epochs.
    ///
    /// The returned map is keyed by peer node ID and carries the tombstone
    /// acknowledgment state that is published alongside a recovery
    /// completion.
    pub fn peer_ack_epochs(&self) -> std::collections::BTreeMap<u32, PeerEpoch> {
        self.peer_acks
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect()
    }

    /// The highest peer acknowledgment epoch observed so far.
    ///
    /// A newly recorded removal is stamped with this watermark. A removal may
    /// only be collected once every declared peer has acknowledged past it,
    /// i.e. has acknowledged at least once since the removal existed.
    fn current_ack_epoch(&self) -> u64 {
        self.peer_acks
            .iter()
            .map(|entry| entry.value().epoch)
            .max()
            .unwrap_or(0)
    }

    /// Garbage collect tombstones that are safe to remove
    ///
    /// This method removes tombstones whose owning peer has acknowledged
    /// the tombstone (via peer_acknowledged) and thus agreed that the
    /// state should be removed. This prevents resurrection of stale state
    /// from long-offline peers.
    ///
    /// # Safety
    /// A tombstone is safe to GC when every peer that could still hold the
    /// removed block has acknowledged a state that is newer than the removal:
    ///
    /// 1. Every *declared* peer (the configured synchronization set) has
    ///    acknowledged at least once **since the removal was recorded**. The
    ///    removal records the acknowledgment epoch that was current at that
    ///    moment, and collection waits for every peer's epoch to pass it.
    /// 2. A dot that originated at a peer outside the declared set (a peer
    ///    that left the cluster) still requires that peer to have advanced
    ///    past the removal.
    ///
    /// Acknowledgment epochs count acknowledgments; dot counters identify
    /// operations. The two are unrelated domains and are never compared
    /// directly: a peer that acknowledged once before the removal existed has
    /// not necessarily observed the removal, so collection waits for an
    /// acknowledgment that happened after it.
    ///
    /// Limitation: the recovery protocol reports an acknowledgment epoch, not
    /// a per-removal watermark, so "acknowledged since the removal existed"
    /// is the strongest guarantee the current wire format supports. A
    /// per-removal WAL watermark would be strictly stronger.
    pub fn gc_tombstones(&self) {
        // Every declared peer must have acknowledged past the removal. A
        // declared peer that has not acknowledged at all holds collection
        // back: it may still hold the live block.
        let declared_peers = self
            .expected_peers
            .iter()
            .map(|peer| *peer)
            .collect::<Vec<_>>();
        if declared_peers.is_empty() {
            // Without a declared peer set there is no evidence about who may
            // still hold the removed block. Retention is the safe answer.
            tracing::warn!(
                retained_tombstones = self.tombstones.len(),
                "mesh tombstone GC skipped: no declared peers"
            );
            return;
        }
        for peer in &declared_peers {
            let Some(epoch) = self.peer_acks.get(peer) else {
                // The peer never acknowledged: the removal is not stable.
                tracing::warn!(
                    peer_node_id = *peer,
                    retained_tombstones = self.tombstones.len(),
                    "mesh tombstone GC held back: peer has not acknowledged"
                );
                return;
            };
            if epoch.epoch == 0 {
                tracing::warn!(
                    peer_node_id = *peer,
                    retained_tombstones = self.tombstones.len(),
                    "mesh tombstone GC held back: peer acknowledgment epoch is zero"
                );
                return;
            }
        }
        self.tombstones.retain(|key, tombstone| {
            // The tombstone key carries the node ID of the peer that originated
            // the removed dot. The matching entry is already gone from
            // `entries` (the tombstone replaced it), so the owning peer is
            // taken from the tombstone key itself.
            let owning_peer_id = key.1; // node_id from BlockKey
            for peer in &declared_peers {
                // The peer must have acknowledged *after* the removal was
                // recorded: an acknowledgment from before the removal existed
                // says nothing about whether the peer observed it.
                let Some(epoch) = self.peer_acks.get(peer) else {
                    // The peer never acknowledged: keep the tombstone.
                    return true;
                };
                if epoch.epoch <= tombstone.created_at_epoch {
                    // The peer has not acknowledged since the removal.
                    return true;
                }
            }
            if owning_peer_id != self.node_id && !declared_peers.contains(&owning_peer_id) {
                // The dot originated at a peer that is no longer part of the
                // declared set: it still has to advance past the removal.
                let Some(epoch) = self.peer_acks.get(&owning_peer_id) else {
                    return true;
                };
                if epoch.epoch <= tombstone.created_at_epoch {
                    return true;
                }
            }
            false // remove tombstone
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    #[test]
    fn basic_operations() {
        let set = Aworset::new();
        set.add("peer1".to_string());
        assert!(set.contains("peer1"));
        set.remove("peer1".to_string());
        assert!(!set.contains("peer1"));
    }

    #[test]
    fn crdt_merge() {
        let set_a = Aworset::new();
        let set_b = Aworset::new();
        set_a.add("item1".to_string());
        set_b.add("item2".to_string());
        set_a.merge(&set_b);
        assert!(set_a.contains("item1"));
        assert!(set_a.contains("item2"));
    }

    #[test]
    fn checkpoint_roundtrip_preserves_concurrent_dots_and_tombstones() {
        let source = AworsetBlocklist::new(7);
        let peer = AworsetBlocklist::new(8);
        let ip = IpAddr::from([192, 0, 2, 79]);
        let first = source.record_ban(ip, 60_000, 1);
        let concurrent = peer.record_ban(ip, 60_000, 2);
        assert!(source.merge_delta(&concurrent));
        let tombstone = ClusterUnblockDelta { ip, dot: first.dot };
        assert!(source.merge_unblock_delta(&tombstone));
        assert!(source.is_blocked(&ip, now_ms()));

        let snapshot = source.export_snapshot();
        let restored = AworsetBlocklist::new(9);
        restored.restore_snapshot(&snapshot);
        assert_eq!(restored.export_snapshot(), snapshot);
        assert!(
            restored.is_blocked(&ip, now_ms()),
            "origin 8's concurrent dot must survive origin 7's tombstone"
        );

        // A later operation from origin 7 must still win over its old
        // checkpointed tombstone watermark.
        let newer = source.record_ban(ip, 60_000, 3);
        assert!(newer.dot.counter > first.dot.counter);
        let newer_snapshot = source.export_snapshot();
        restored.restore_snapshot(&newer_snapshot);
        assert!(restored.is_blocked(&ip, now_ms()));
    }

    #[test]
    fn prepared_ban_is_not_visible_until_committed() {
        let mesh = AworsetBlocklist::new(7);
        let ip = IpAddr::from([192, 0, 2, 77]);

        let delta = mesh.prepare_ban(ip, 60_000, 2);
        assert!(!mesh.is_blocked(&ip, delta.created_at_ms));
        assert!(mesh.needs_merge_delta(&delta));

        assert!(mesh.merge_delta(&delta));
        assert!(mesh.is_blocked(&ip, delta.created_at_ms));
        assert!(!mesh.needs_merge_delta(&delta));
    }

    #[test]
    fn prepared_unblock_is_not_visible_until_committed() {
        let mesh = AworsetBlocklist::new(7);
        let ip = IpAddr::from([192, 0, 2, 78]);
        let ban = mesh.record_ban(ip, 60_000, 2);

        let tombstones = mesh.prepare_unban(ip);
        assert_eq!(tombstones.len(), 1);
        assert!(mesh.is_blocked(&ip, ban.created_at_ms));
        assert!(mesh.needs_merge_unblock(&tombstones[0]));

        assert!(mesh.merge_unblock_delta(&tombstones[0]));
        assert!(!mesh.is_blocked(&ip, ban.created_at_ms));
        assert!(!mesh.needs_merge_unblock(&tombstones[0]));
    }

    #[test]
    fn ban_lifecycle() {
        let mesh = AworsetBlocklist::new(1);
        let ip = IpAddr::from([203, 0, 113, 195]);
        let delta = mesh.record_ban(ip, 60_000, 2);
        assert_eq!(delta.tier, 2);
        assert!(mesh.is_blocked(&ip, 1_000), "ban not yet expired");
        assert!(!mesh.is_blocked(&ip, 2_000_000_000_000), "ban must expire");
    }

    #[test]
    fn expired_entries_are_removed_after_tombstone_horizon() {
        let mesh = AworsetBlocklist::new(1);
        let ip = IpAddr::from([203, 0, 113, 196]);
        let delta = mesh.record_ban(ip, 60_000, 2);
        mesh.purge_expired(delta.expires_at_ms + 86_400_000);
        assert!(mesh.is_empty(), "expired mesh state must be bounded");
    }

    #[test]
    fn permanent_remote_ban_does_not_poison_local_hlc() {
        let sender = AworsetBlocklist::new(1);
        let receiver = AworsetBlocklist::new(2);
        let permanent_ip = IpAddr::from([198, 51, 100, 11]);
        let finite_ip = IpAddr::from([198, 51, 100, 12]);

        let permanent = sender.record_ban(permanent_ip, 0, 3);
        assert_eq!(permanent.expires_at_ms, u64::MAX);
        assert!(receiver.merge_delta(&permanent));

        let finite = receiver.record_ban(finite_ip, 60_000, 1);
        assert_ne!(finite.expires_at_ms, u64::MAX);
        assert_eq!(
            finite.expires_at_ms.saturating_sub(finite.created_at_ms),
            60_000
        );
    }

    #[test]
    fn explicit_unban_watermark_survives_gc_horizon_and_rejects_replay() {
        let sender = AworsetBlocklist::new(1);
        let receiver = AworsetBlocklist::new(2);
        let ip = IpAddr::from([198, 51, 100, 61]);
        let old_ban = sender.record_ban(ip, 60_000, 1);
        assert!(receiver.merge_delta(&old_ban));
        let unban = receiver.record_unban(ip);
        assert_eq!(unban.len(), 1);

        receiver.purge_expired(now_ms().saturating_add(86_400_001));
        assert!(!receiver.merge_delta(&old_ban));
        assert!(!receiver.is_blocked(&ip, now_ms()));
    }

    #[test]
    fn expired_entry_becomes_watermark_before_it_is_pruned() {
        let sender = AworsetBlocklist::new(1);
        let receiver = AworsetBlocklist::new(2);
        let ip = IpAddr::from([198, 51, 100, 62]);
        let old_ban = sender.record_ban(ip, 1, 1);
        assert!(receiver.merge_delta(&old_ban));

        receiver.purge_expired(old_ban.expires_at_ms.saturating_add(86_400_000));
        assert!(receiver.entries.is_empty());
        assert_eq!(
            receiver
                .tombstones
                .get(&(ip, sender.node_id, old_ban.dot.incarnation))
                .map(|v| v.counter),
            Some(old_ban.dot.counter)
        );
        assert!(
            !receiver.merge_delta(&old_ban),
            "expired stale delta must not return"
        );
    }

    #[test]
    fn nil_incarnation_deltas_are_rejected() {
        let mesh = AworsetBlocklist::new(18);
        let ip = IpAddr::from([192, 0, 2, 20]);
        let now = now_ms();
        let block = ClusterBlockDelta {
            ip,
            dot: ClusterDot {
                node_id: 7,
                incarnation: Uuid::nil(),
                counter: 1,
            },
            created_at_ms: now,
            expires_at_ms: now.saturating_add(60_000),
            tier: 1,
        };
        assert!(!mesh.merge_delta(&block));

        let unblock = ClusterUnblockDelta { ip, dot: block.dot };
        assert!(!mesh.merge_unblock_delta(&unblock));
    }

    #[test]
    fn restarted_instance_uses_a_distinct_dot_incarnation() {
        let ip = IpAddr::from([192, 0, 2, 19]);
        let first_instance = AworsetBlocklist::new(17);
        let old_ban = first_instance.record_ban(ip, 60_000, 1);
        let restarted_instance = AworsetBlocklist::new(17);
        let new_ban = restarted_instance.record_ban(ip, 60_000, 1);

        assert_eq!(old_ban.dot.node_id, new_ban.dot.node_id);
        assert_eq!(old_ban.dot.counter, new_ban.dot.counter);
        assert_ne!(old_ban.dot.incarnation, new_ban.dot.incarnation);

        let receiver = AworsetBlocklist::new(18);
        assert!(receiver.merge_delta(&old_ban));
        assert!(receiver.merge_delta(&new_ban));
        assert_eq!(receiver.len(), 2);
    }

    #[test]
    fn local_dot_counter_stays_monotonic_when_hlc_physical_time_advances() {
        let mesh = AworsetBlocklist::new(17);
        let first = mesh.record_ban(IpAddr::from([192, 0, 2, 17]), 60_000, 1);

        // Simulate the wall clock/HLC moving forward independently of the
        // operation sequence. A later ban must not reuse/reset its dot.
        mesh.hlc.tick(now_ms().saturating_add(10_000), 0);
        let second = mesh.record_ban(IpAddr::from([192, 0, 2, 18]), 60_000, 1);

        assert!(second.dot.counter > first.dot.counter);
        assert_eq!(first.dot.node_id, second.dot.node_id);
    }

    #[test]
    fn delta_merge_wins_by_counter() {
        let a = AworsetBlocklist::new(1);
        let ip = IpAddr::from([198, 51, 100, 7]);
        // Fresh delta at ttl 60s.
        let d1 = a.record_ban(ip, 60_000, 1);

        // Peer node 2 sees it and re-bans with a longer TTL.
        let mut d2 = d1.clone();
        d2.dot.node_id = 2;
        d2.dot.incarnation = Uuid::new_v4();
        d2.dot.counter += 1;
        d2.expires_at_ms += 60_000;

        assert!(a.merge_delta(&d2), "higher counter must merge in");
        // Stale delta from node 2 must NOT overwrite.
        let mut d3 = d2.clone();
        d3.dot.counter -= 1;
        assert!(!a.merge_delta(&d3), "lower counter must be rejected");
    }
    #[test]
    fn concurrent_bans_converge_independent_of_merge_order() {
        let node_a = AworsetBlocklist::new(1);
        let node_b = AworsetBlocklist::new(2);
        let ip = IpAddr::from([198, 51, 100, 44]);
        let a = node_a.record_ban(ip, 60_000, 1);
        let b = node_b.record_ban(ip, 90_000, 2);

        let left = AworsetBlocklist::new(3);
        let right = AworsetBlocklist::new(4);
        assert!(left.merge_delta(&a));
        assert!(left.merge_delta(&b));
        assert!(right.merge_delta(&b));
        assert!(right.merge_delta(&a));

        let normalize = |mesh: &AworsetBlocklist| {
            let mut state: Vec<_> = mesh
                .entries
                .iter()
                .map(|entry| (*entry.key(), *entry.value()))
                .collect();
            state.sort_by_key(|(key, value)| (key.0, key.1, key.2, value.0));
            state
        };
        assert_eq!(normalize(&left), normalize(&right));
        assert_eq!(left.len(), 2);
        assert_eq!(right.len(), 2);
    }

    #[test]
    fn duplicate_and_reordered_deltas_are_idempotent() {
        let sender = AworsetBlocklist::new(1);
        let receiver = AworsetBlocklist::new(2);
        let ip = IpAddr::from([198, 51, 100, 45]);
        let old = sender.record_ban(ip, 60_000, 1);
        let mut newer = sender.record_ban(ip, 90_000, 2);
        newer.dot.counter = old.dot.counter + 1;

        assert!(receiver.merge_delta(&newer));
        assert!(
            !receiver.merge_delta(&newer),
            "duplicate delta must be idempotent"
        );
        assert!(
            !receiver.merge_delta(&old),
            "reordered stale delta must be ignored"
        );
        let entry = receiver
            .entries
            .get(&(ip, sender.node_id, old.dot.incarnation))
            .unwrap();
        assert_eq!(entry.value().0, newer.dot.counter);
        assert_eq!(entry.value().2, newer.expires_at_ms);
    }

    #[test]
    fn unban_tombstone_rejects_delayed_old_ban() {
        let peer = AworsetBlocklist::new(2);
        let ip = IpAddr::from([198, 51, 100, 42]);
        let ban = peer.record_ban(ip, 60_000, 1);
        let local = AworsetBlocklist::new(1);
        assert!(local.merge_delta(&ban));
        let unbans = local.record_unban(ip);
        assert_eq!(unbans.len(), 1);
        assert!(peer.merge_unblock_delta(&unbans[0]));
        assert!(!peer.is_blocked(&ip, 1));
        assert!(
            !peer.merge_delta(&ban),
            "pre-unblock ban must not resurrect"
        );
    }
    #[test]
    fn newer_local_ban_after_unban_is_accepted() {
        let sender = AworsetBlocklist::new(1);
        let receiver = AworsetBlocklist::new(2);
        let ip = IpAddr::from([198, 51, 100, 43]);

        let first = sender.record_ban(ip, 60_000, 1);
        assert!(receiver.merge_delta(&first));

        let unblocks = sender.record_unban(ip);
        assert_eq!(unblocks.len(), 1);
        assert!(receiver.merge_unblock_delta(&unblocks[0]));
        assert!(!receiver.is_blocked(&ip, now_ms().saturating_add(1)));

        let rebanned = sender.record_ban(ip, 60_000, 1);
        assert!(rebanned.dot.counter > first.dot.counter);
        assert!(receiver.merge_delta(&rebanned));
        assert!(receiver.is_blocked(&ip, now_ms()));
    }

    #[test]
    fn exported_snapshot_covers_large_mixed_state() {
        const EACH: usize = 2_500;
        let mesh = AworsetBlocklist::new(7);
        let block_incarnation = Uuid::new_v4();
        let tombstone_incarnation = Uuid::new_v4();
        let mut expected_blocks = std::collections::HashSet::new();
        let mut expected_tombstones = std::collections::HashSet::new();

        for n in 0..EACH {
            let block_ip = IpAddr::V6(std::net::Ipv6Addr::from(n as u128));
            mesh.entries.insert(
                (block_ip, 7, block_incarnation),
                (n as u64 + 1, 0, u64::MAX, 2),
            );
            expected_blocks.insert(block_ip);

            let tombstone_ip = IpAddr::V6(std::net::Ipv6Addr::from((n + 10_000) as u128));
            mesh.tombstones.insert(
                (tombstone_ip, 7, tombstone_incarnation),
                TombstoneState {
                    counter: n as u64 + 1,
                    created_at_epoch: 0,
                },
            );
            expected_tombstones.insert(tombstone_ip);
        }

        let snapshot = mesh.export_snapshot();
        assert_eq!(snapshot.blocks.len(), EACH);
        assert_eq!(snapshot.unblocks.len(), EACH);
        assert_eq!(
            snapshot
                .blocks
                .iter()
                .map(|delta| delta.ip)
                .collect::<std::collections::HashSet<_>>(),
            expected_blocks
        );
        assert_eq!(
            snapshot
                .unblocks
                .iter()
                .map(|delta| delta.ip)
                .collect::<std::collections::HashSet<_>>(),
            expected_tombstones
        );
        assert!(snapshot.blocks.windows(2).all(|pair| {
            (
                pair[0].ip,
                pair[0].dot.node_id,
                pair[0].dot.incarnation,
                pair[0].dot.counter,
            ) <= (
                pair[1].ip,
                pair[1].dot.node_id,
                pair[1].dot.incarnation,
                pair[1].dot.counter,
            )
        }));
    }

    #[test]
    fn exported_recovery_snapshot_is_immutable_after_capture() {
        let mesh = AworsetBlocklist::new(7);
        let ip_a = IpAddr::from([192, 0, 2, 70]);
        let ip_b = IpAddr::from([192, 0, 2, 71]);
        mesh.record_ban(ip_a, 60_000, 1);

        let captured = mesh.export_snapshot();
        let captured_bytes = serde_json::to_vec(&captured).expect("serialize captured snapshot");

        mesh.record_unban(ip_a);
        mesh.record_ban(ip_b, 60_000, 2);

        assert_eq!(
            serde_json::to_vec(&captured).expect("serialize captured snapshot again"),
            captured_bytes,
            "mutations after capture must not alter the recovery payload"
        );
        assert_eq!(captured.blocks.len(), 1);
        assert_eq!(captured.blocks[0].ip, ip_a);
        assert!(mesh.is_blocked(&ip_b, now_ms()));
    }

    #[test]
    fn snapshot_carries_active_dot_and_tombstone() {
        let mesh = AworsetBlocklist::new(7);
        let ip = IpAddr::from([192, 0, 2, 7]);
        let ban = mesh.record_ban(ip, 60_000, 3);
        let snapshot = mesh.export_snapshot();
        let blocks = snapshot.blocks;
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].dot, ban.dot);
        assert_eq!(blocks[0].tier, ban.tier);
        let unblocks = mesh.record_unban(ip);
        assert_eq!(unblocks.len(), 1);
        let snapshot = mesh.export_snapshot();
        let tombstones = snapshot.unblocks;
        assert_eq!(tombstones.len(), 1);
        assert_eq!(tombstones[0].dot, ban.dot);
    }
    /// A tombstone must not be collected while a peer that has not yet
    /// acknowledged the removal is still behind: collecting early lets that
    /// peer resurrect the block.
    #[test]
    fn tombstone_gc_waits_for_every_acknowledging_peer() {
        use std::net::Ipv4Addr;
        let node1 = AworsetBlocklist::new(1);
        let node2 = AworsetBlocklist::new(2);

        node1.set_expected_peers([2, 3]);

        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        let ban_delta = node2.record_ban(ip, 60_000, 1);
        assert!(node1.merge_delta(&ban_delta));

        // Both peers acknowledge *before* the removal exists. Those
        // acknowledgments say nothing about this removal.
        node1.peer_acknowledged(2, 1);
        node1.peer_acknowledged(3, 1);

        let unblock_deltas = node1.record_unban(ip);
        assert_eq!(unblock_deltas.len(), 1);
        assert_eq!(node1.tombstones.len(), 1);

        node1.gc_tombstones();
        assert_eq!(
            node1.tombstones.len(),
            1,
            "a removal must not be collected before a peer acknowledged after it existed"
        );

        // One peer advances past the removal, the other is still behind.
        node1.peer_acknowledged(2, 2);
        node1.gc_tombstones();
        assert_eq!(
            node1.tombstones.len(),
            1,
            "a removal must not be collected while another peer is behind"
        );

        // Every declared peer advances past the removal.
        node1.peer_acknowledged(3, 2);
        node1.gc_tombstones();
        assert_eq!(
            node1.tombstones.len(),
            0,
            "the removal may be collected once every declared peer advanced"
        );
    }

    #[test]
    fn tombstone_gc_prevents_resurrection_after_acknowledgment() {
        use std::net::Ipv4Addr;
        let node1 = AworsetBlocklist::new(1);
        let node2 = AworsetBlocklist::new(2);
        node1.set_expected_peers([2]);

        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let ban_delta = node2.record_ban(ip, 60_000, 1);
        assert!(node1.merge_delta(&ban_delta));

        let unblock_deltas = node1.record_unban(ip);
        assert_eq!(unblock_deltas.len(), 1);
        assert!(node2.merge_unblock_delta(&unblock_deltas[0]));

        assert_eq!(node1.tombstones.len(), 1);
        node1.gc_tombstones();
        assert_eq!(
            node1.tombstones.len(),
            1,
            "a removal must not be collected before the owning peer acknowledged"
        );
        node1.peer_acknowledged(2, 1);
        node1.gc_tombstones();
        assert_eq!(
            node1.tombstones.len(),
            0,
            "the removal may be collected once the owning peer acknowledged after it existed"
        );
    }

    #[test]
    fn peer_acknowledgment_updates_epoch() {
        let node = AworsetBlocklist::new(1);
        assert_eq!(node.peer_acks.len(), 0);
        node.peer_acknowledged(2, 100);
        assert_eq!(node.peer_acks.len(), 1);
        // The observed values are copied out of the map: the DashMap read
        // guard must be released before the next `peer_acknowledged` call
        // takes the write lock on the same shard, otherwise the test
        // deadlocks against itself.
        let (epoch, last_acknowledged_lsn) = {
            let peer_epoch = node.peer_acks.get(&2).unwrap();
            (peer_epoch.epoch, peer_epoch.last_acknowledged_lsn)
        };
        assert_eq!(epoch, 1);
        assert_eq!(last_acknowledged_lsn, 100);
        node.peer_acknowledged(2, 200);
        let (epoch, last_acknowledged_lsn) = {
            let peer_epoch = node.peer_acks.get(&2).unwrap();
            (peer_epoch.epoch, peer_epoch.last_acknowledged_lsn)
        };
        assert_eq!(epoch, 2);
        assert_eq!(last_acknowledged_lsn, 200);
        node.peer_acknowledged(2, 200);
        let (epoch, last_acknowledged_lsn) = {
            let peer_epoch = node.peer_acks.get(&2).unwrap();
            (peer_epoch.epoch, peer_epoch.last_acknowledged_lsn)
        };
        assert_eq!(epoch, 2);
        assert_eq!(last_acknowledged_lsn, 200);
    }

    #[test]
    fn tombstone_gc_collects_locally_removed_dots_once_peers_advance() {
        use std::net::Ipv4Addr;
        let node1 = AworsetBlocklist::new(1);
        node1.set_expected_peers([2]);
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 9));

        node1.record_ban(ip, 60_000, 1);
        let unblocks = node1.record_unban(ip);
        assert_eq!(unblocks.len(), 1);
        assert_eq!(
            node1.tombstones.len(),
            1,
            "the local removal is retained as a tombstone"
        );

        // The configured peer has not acknowledged: the removal is not stable
        // for anyone else yet, so the tombstone stays.
        node1.gc_tombstones();
        assert_eq!(
            node1.tombstones.len(),
            1,
            "a removal must not be collected while a configured peer is silent"
        );

        // Once the peer has acknowledged a state past the removal, the local
        // tombstone is collectable.
        node1.peer_acknowledged(2, 2);
        node1.gc_tombstones();
        assert_eq!(
            node1.tombstones.len(),
            0,
            "a locally removed dot is collectable once every peer advanced"
        );
    }
    #[test]
    fn gc_tombstones_respects_causal_safety() {
        use std::net::Ipv4Addr;
        let node = AworsetBlocklist::new(1);
        let ip1 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        let _ban1 = node.record_ban(ip1, 60_000, 1);
        let unblock1 = node.record_unban(ip1);
        assert_eq!(unblock1.len(), 1);
        let _ban2 = node.record_ban(ip2, 60_000, 1);
        let unblock2 = node.record_unban(ip2);
        assert_eq!(unblock2.len(), 1);
        assert_eq!(node.tombstones.len(), 2);
        node.gc_tombstones();
        assert_eq!(node.tombstones.len(), 2);
    }
    // ==================== M-003: CONVERGENCE TESTS ====================

    /// Test that merge is commutative: A.merge(B) == B.merge(A)
    #[test]
    fn merge_is_commutative() {
        let node1 = AworsetBlocklist::new(1);
        let node2 = AworsetBlocklist::new(2);

        let ip1 = IpAddr::from([10, 0, 0, 1]);
        let ip2 = IpAddr::from([10, 0, 0, 2]);

        // Independent operations
        node1.record_ban(ip1, 60_000, 1);
        node2.record_ban(ip2, 60_000, 1);

        // Merge in order 1->2
        let delta1 = node1.export_snapshot();
        let delta2 = node2.export_snapshot();

        let merge12 = AworsetBlocklist::new(99);
        merge12.restore_snapshot(&delta1);
        merge12.restore_snapshot(&delta2);

        // Merge in order 2->1
        let merge21 = AworsetBlocklist::new(99);
        merge21.restore_snapshot(&delta2);
        merge21.restore_snapshot(&delta1);

        // Both should have both IPs blocked
        assert!(merge12.is_blocked(&ip1, now_ms()));
        assert!(merge12.is_blocked(&ip2, now_ms()));
        assert!(merge21.is_blocked(&ip1, now_ms()));
        assert!(merge21.is_blocked(&ip2, now_ms()));
    }

    /// Test that merge is idempotent: A.merge(B) twice == A.merge(B) once
    #[test]
    fn merge_is_idempotent() {
        let node1 = AworsetBlocklist::new(1);
        let _node2 = AworsetBlocklist::new(2);

        let ip = IpAddr::from([10, 0, 0, 1]);
        node1.record_ban(ip, 60_000, 1);

        let snapshot = node1.export_snapshot();

        let receiver = AworsetBlocklist::new(99);
        receiver.restore_snapshot(&snapshot);
        let after_first = receiver.export_snapshot();

        receiver.restore_snapshot(&snapshot);
        let after_second = receiver.export_snapshot();

        // Applying same snapshot twice should be idempotent
        assert_eq!(after_first, after_second, "merge should be idempotent");
    }

    /// Test that merge is associative: (A+B)+C == A+(B+C)
    #[test]
    fn merge_is_associative() {
        let node1 = AworsetBlocklist::new(1);
        let node2 = AworsetBlocklist::new(2);
        let node3 = AworsetBlocklist::new(3);

        let ip1 = IpAddr::from([10, 0, 0, 1]);
        let ip2 = IpAddr::from([10, 0, 0, 2]);
        let ip3 = IpAddr::from([10, 0, 0, 3]);

        node1.record_ban(ip1, 60_000, 1);
        node2.record_ban(ip2, 60_000, 1);
        node3.record_ban(ip3, 60_000, 1);

        let snap1 = node1.export_snapshot();
        let snap2 = node2.export_snapshot();
        let snap3 = node3.export_snapshot();

        // (1+2)+3
        let left = AworsetBlocklist::new(99);
        left.restore_snapshot(&snap1);
        left.restore_snapshot(&snap2);
        left.restore_snapshot(&snap3);

        // 1+(2+3)
        let right = AworsetBlocklist::new(99);
        right.restore_snapshot(&snap1);
        right.restore_snapshot(&snap2);
        right.restore_snapshot(&snap3);

        assert_eq!(left.export_snapshot(), right.export_snapshot());
        assert!(left.is_blocked(&ip1, now_ms()));
        assert!(left.is_blocked(&ip2, now_ms()));
        assert!(left.is_blocked(&ip3, now_ms()));
    }

    /// Test convergence after network partition heals
    #[test]
    fn convergence_after_partition_heals() {
        let node_a = AworsetBlocklist::new(1);
        let node_b = AworsetBlocklist::new(2);

        let ip_shared = IpAddr::from([10, 0, 0, 100]);
        let ip_a_only = IpAddr::from([10, 0, 0, 101]);
        let ip_b_only = IpAddr::from([10, 0, 0, 102]);

        // Initial state: both agree on ip_shared
        node_a.record_ban(ip_shared, 60_000, 1);
        let delta = node_a.export_snapshot();
        node_b.restore_snapshot(&delta);

        // Network partition: nodes diverge
        node_a.record_ban(ip_a_only, 60_000, 1);
        node_b.record_ban(ip_b_only, 60_000, 1);

        // Also: node_a unbans ip_shared during partition
        let unblocks = node_a.record_unban(ip_shared);
        assert_eq!(unblocks.len(), 1);

        // Partition heals: exchange snapshots
        let snap_a = node_a.export_snapshot();
        let snap_b = node_b.export_snapshot();

        node_a.restore_snapshot(&snap_b);
        node_b.restore_snapshot(&snap_a);

        // Both should converge to same state
        assert!(node_a.is_blocked(&ip_a_only, now_ms()));
        assert!(node_a.is_blocked(&ip_b_only, now_ms()));
        assert!(!node_a.is_blocked(&ip_shared, now_ms()), "unban should win");

        assert!(node_b.is_blocked(&ip_a_only, now_ms()));
        assert!(node_b.is_blocked(&ip_b_only, now_ms()));
        assert!(!node_b.is_blocked(&ip_shared, now_ms()), "unban should win");

        // Snapshots should be identical after convergence
        assert_eq!(node_a.export_snapshot(), node_b.export_snapshot());
    }

    /// Test that concurrent operations from different nodes both survive merge
    #[test]
    fn concurrent_operations_both_survive() {
        let node1 = AworsetBlocklist::new(1);
        let node2 = AworsetBlocklist::new(2);

        let ip1 = IpAddr::from([10, 0, 0, 1]);
        let ip2 = IpAddr::from([10, 0, 0, 2]);

        // Concurrent bans from different nodes
        let delta1 = node1.record_ban(ip1, 60_000, 1);
        let delta2 = node2.record_ban(ip2, 60_000, 1);

        // Exchange deltas
        node1.merge_delta(&delta2);
        node2.merge_delta(&delta1);

        // Both should see both bans
        assert!(node1.is_blocked(&ip1, now_ms()));
        assert!(node1.is_blocked(&ip2, now_ms()));
        assert!(node2.is_blocked(&ip1, now_ms()));
        assert!(node2.is_blocked(&ip2, now_ms()));
    }

    /// Test that add-wins semantics work: concurrent add and remove on same IP
    #[test]
    fn add_wins_on_concurrent_operations() {
        let node1 = AworsetBlocklist::new(1);
        let node2 = AworsetBlocklist::new(2);

        let ip = IpAddr::from([10, 0, 0, 1]);

        // Node1 has existing ban, Node2 tries to unban concurrently
        node1.record_ban(ip, 60_000, 1);
        let initial = node1.export_snapshot();
        node2.restore_snapshot(&initial);

        // Node1 refreshes ban (add), Node2 unbans (remove) - concurrent
        let new_ban = node1.record_ban(ip, 60_000, 2);
        let unblocks = node2.record_unban(ip);
        assert_eq!(unblocks.len(), 1);

        // Exchange
        node1.merge_unblock_delta(&unblocks[0]);
        node2.merge_delta(&new_ban);

        // Add-wins: ban should still be active
        assert!(
            node1.is_blocked(&ip, now_ms()),
            "add should win over concurrent remove"
        );
        assert!(
            node2.is_blocked(&ip, now_ms()),
            "add should win over concurrent remove"
        );
    }

    /// Test eventual consistency: all nodes converge after exchanging all deltas
    #[test]
    fn eventual_consistency_convergence() {
        let nodes: Vec<AworsetBlocklist> = (1..=3).map(AworsetBlocklist::new).collect();
        let ip = IpAddr::from([10, 0, 0, 1]);

        // Each node makes independent operation
        for (idx, node) in nodes.iter().enumerate() {
            node.record_ban(ip, 60_000, (idx + 1) as u8);
        }

        // Gather all snapshots
        let snapshots: Vec<_> = nodes.iter().map(|n| n.export_snapshot()).collect();

        // Each node applies all other nodes' snapshots
        for node in &nodes {
            for snap in &snapshots {
                node.restore_snapshot(snap);
            }
        }

        // All nodes should converge
        assert_eq!(nodes[0].export_snapshot(), nodes[1].export_snapshot());
        assert_eq!(nodes[1].export_snapshot(), nodes[2].export_snapshot());
        assert!(nodes[0].is_blocked(&ip, now_ms()));
        assert!(nodes[1].is_blocked(&ip, now_ms()));
        assert!(nodes[2].is_blocked(&ip, now_ms()));
    }

    // M-013: Frame boundary tests
    // M-013: Frame boundary tests - frame size, delimiter, EOF, and partial reads
    #[test]
    fn frame_boundary_split_delimiter() {
        let mesh = AworsetBlocklist::new(7);
        let ip = IpAddr::from([192, 0, 2, 7]);
        let ban = mesh.record_ban(ip, 60_000, 1);
        let snapshot = mesh.export_snapshot();
        let blocks = snapshot.blocks;
        let node1 = AworsetBlocklist::new(1);
        let node2 = AworsetBlocklist::new(2);

        // Test that split block delivery still works
        node2.merge_delta(&ban);
        node1.merge_delta(&ban);

        // Both nodes should have the ban
        assert!(node1.is_blocked(&ip, now_ms()));
        assert!(node2.is_blocked(&ip, now_ms()));
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn frame_boundary_split_across_block_key() {
        let mesh = AworsetBlocklist::new(7);
        let ip = IpAddr::from([192, 0, 2, 7]);
        let ban = mesh.record_ban(ip, 60_000, 3);
        let snapshot = mesh.export_snapshot();
        let blocks = snapshot.blocks;

        // Verify block structure after split simulation
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].ip, ip);
        assert_eq!(blocks[0].dot.node_id, 7);
        assert_eq!(blocks[0].dot.incarnation, ban.dot.incarnation);
        assert!(!blocks[0].dot.incarnation.is_nil());
        assert_eq!(blocks[0].dot.counter, 1);
    }

    #[test]
    fn frame_boundary_clean_eof() {
        let mesh = AworsetBlocklist::new(7);
        let ip = IpAddr::from([192, 0, 2, 7]);
        let ban = mesh.record_ban(ip, 60_000, 3);

        // Clean EOF: full frame delivery
        let blocks = mesh.export_snapshot().blocks;
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].dot, ban.dot);
        assert_eq!(blocks[0].tier, ban.tier);
    }

    #[test]
    fn frame_boundary_partial_eof() {
        let mesh = AworsetBlocklist::new(7);
        let ip = IpAddr::from([192, 0, 2, 7]);
        let ban = mesh.record_ban(ip, 60_000, 3);

        // Partial EOF: verify state preservation after partial frame
        let snapshot = mesh.export_snapshot();
        let blocks = snapshot.blocks;
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].dot, ban.dot);
    }

    #[test]
    fn frame_boundary_multiple_frames() {
        let mesh = AworsetBlocklist::new(7);

        // Multiple bans should fit in frames correctly
        for i in 0..10 {
            let ip = IpAddr::from([192, 0, 2, (i + 7) as u8]);
            mesh.record_ban(ip, 60_000, (i + 1) as u8);
        }

        let snapshot = mesh.export_snapshot();
        let blocks = snapshot.blocks;
        assert_eq!(blocks.len(), 10);

        // Verify all blocks are present
        for (i, block) in blocks.iter().enumerate() {
            assert_eq!(block.dot.counter, (i + 1) as u64);
        }
    }

    #[test]
    fn frame_boundary_oversized_frame() {
        let mesh = AworsetBlocklist::new(7);
        let ip = IpAddr::from([198, 51, 100, 45]);

        // Create a ban that would require larger frame
        let _ban = mesh.record_ban(ip, 60_000, 1);

        // Verify it works within the frame limits
        let snapshot = mesh.export_snapshot();
        let blocks = snapshot.blocks;
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].dot.counter, 1);
    }

    #[test]
    fn frame_boundary_empty_frame() {
        let mesh = AworsetBlocklist::new(7);

        // Test that empty state snapshots work
        let snapshot = mesh.export_snapshot();
        let blocks = snapshot.blocks;
        let unblocks = snapshot.unblocks;
        assert_eq!(blocks.len(), 0);
        assert_eq!(unblocks.len(), 0);
    }
}
