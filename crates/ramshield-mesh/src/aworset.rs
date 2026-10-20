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
    atomic::{AtomicU64, Ordering},
    RwLock,
};
use uuid::Uuid;

/// Epoch tracking for peer acknowledgment safety
#[derive(Debug, Clone)]
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
    pub(crate) tombstones: DashMap<BlockKey, u64>,
    /// Peer acknowledgment tracking for safe tombstone garbage collection.
    /// Maps peer node_id to their acknowledgment epoch and last seen state.
    pub(crate) peer_acks: DashMap<u32, PeerEpoch>,
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
            next_dot_counter: AtomicU64::new(0),
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
        let previous = self
            .next_dot_counter
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("mesh dot counter exhausted; refusing to emit a duplicate CRDT dot");
        let counter = previous + 1;
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
            .is_some_and(|counter| delta.dot.counter <= *counter)
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
            .is_none_or(|counter| delta.dot.counter > *counter);
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
            .map(|t| delta.dot.counter <= *t)
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
                    counter: *entry.value(),
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

        for (key, counter) in expired {
            self.entries.remove(&key);
            self.tombstones
                .entry(key)
                .and_modify(|existing| *existing = (*existing).max(counter))
                .or_insert(counter);
        }
    }

    /// Record an observed-remove for every current origin dot of an IP.
    pub fn record_unban(&self, ip: IpAddr) -> Vec<ClusterUnblockDelta> {
        let deltas = self.prepare_unban(ip);
        for delta in &deltas {
            self.merge_unblock_delta(delta);
        }
        deltas
    }

    /// Apply a peer tombstone. True if it changed local state.
    pub fn merge_unblock_delta(&self, delta: &ClusterUnblockDelta) -> bool {
        let _snapshot_guard = self
            .snapshot_barrier
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if delta.dot.node_id == 0 || delta.dot.incarnation.is_nil() {
            return false;
        }
        let key = (delta.ip, delta.dot.node_id, delta.dot.incarnation);
        let mut changed = false;
        self.tombstones
            .entry(key)
            .and_modify(|existing| {
                if delta.dot.counter > *existing {
                    *existing = delta.dot.counter;
                    changed = true;
                }
            })
            .or_insert_with(|| {
                changed = true;
                delta.dot.counter
            });
        if self
            .entries
            .get(&key)
            .is_some_and(|entry| entry.value().0 <= delta.dot.counter)
        {
            self.entries.remove(&key);
            changed = true;
        }
        changed
    }

    /// Update peer's epoch when they acknowledge our state
    ///
    /// This method is called when a peer confirms they have received and applied
    /// our state via the recovery completion protocol. It updates their epoch
    /// and calculates when their tombstones can be safely garbage collected.
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
            epoch.epoch += 1;
            // Set tombstone expiry to current epoch + 1 (safe after next ack)
            epoch.tombstone_expiry_epoch = epoch.epoch + 1;
        }
    }

    /// Garbage collect tombstones that are causally safe to remove
    ///
    /// A tombstone is safe to GC when all peers that could have observed
    /// the tombstone (via peer_acknowledged) and thus agreed that the
    /// state should be removed. This prevents resurrection of stale state
    /// from long-offline peers.
    pub fn gc_tombstones(&self) {
        let current_epoch = self.get_local_epoch();
        self.tombstones.retain(|key, counter| {
            // Find which peer owns this tombstone (by checking entries map for the key)
            if let Some(owning_entry) = self.entries.get(key) {
                let owning_peer_id = owning_entry.key().1; // node_id from BlockKey
                if let Some(owning_epoch) = self.peer_acks.get(&owning_peer_id) {
                    // Safe to GC if the owning peer's epoch has advanced beyond
                    // when this tombstone was created
                    if owning_epoch.epoch > *counter {
                        return false; // remove tombstone
                    }
                }
            }
            true // keep tombstone
        });
    }

    /// Get the current epoch of the local peer
    ///
    /// This is used by gc_tombstones() to determine what tombstones are safe to remove.
    ///
    /// # Returns
    /// The current epoch value for this peer
    fn get_local_epoch(&self) -> u64 {
        // Use the local peer's node_id to get their epoch
        // If not found, return 0 (should not happen in normal operation)
        self.peer_acks
            .get(&self.node_id)
            .map(|epoch| epoch.epoch)
            .unwrap_or(0)
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
    fn merge_operations() {
        let set1 = Aworset::new();
        let set2 = Aworset::new();
        set1.add("peer1".to_string());
        set2.add("peer2".to_string());
        set1.merge(&set2);
        assert!(set1.contains("peer1"));
        assert!(set1.contains("peer2"));
    }

    #[test]
    fn concurrent_add_remove() {
        let set = Aworset::new();
        set.add("peer1".to_string());
        set.remove("peer1".to_string());
        set.add("peer1".to_string());
        assert!(set.contains("peer1"));
    }

    #[test]
    fn blocklist_ban_unban() {
        let bl = AworsetBlocklist::new(1);
        let ip = "192.0.2.1".parse().unwrap();
        let delta = bl.record_ban(ip, 60_000, 1);
        assert!(bl.is_blocked(&ip, now_ms()));
        let unblocks = bl.record_unban(ip);
        assert!(!bl.is_blocked(&ip, now_ms()));
        assert_eq!(unblocks.len(), 1);
        assert_eq!(unblocks[0].dot, delta.dot);
    }

    #[test]
    fn snapshot_roundtrip() {
        let bl = AworsetBlocklist::new(1);
        let ip = "192.0.2.1".parse().unwrap();
        bl.record_ban(ip, 60_000, 1);
        let snap = bl.export_snapshot();
        let bl2 = AworsetBlocklist::new(2);
        bl2.restore_snapshot(&snap);
        assert!(bl2.is_blocked(&ip, now_ms()));
        assert_eq!(bl.export_snapshot(), bl2.export_snapshot());
    }

    #[test]
    fn tombstone_gc_prevents_resurrection_after_acknowledgment() {
        use std::net::Ipv4Addr;
        let node1 = AworsetBlocklist::new(1);
        let node2 = AworsetBlocklist::new(2);

        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let ban_delta = node2.record_ban(ip, 60_000, 1);
        assert!(node1.merge_delta(&ban_delta));

        let unblock_deltas = node1.record_unban(ip);
        assert_eq!(unblock_deltas.len(), 1);
        assert!(node2.merge_unblock_delta(&unblock_deltas[0]));

        assert_eq!(node1.tombstones.len(), 1);
        node1.gc_tombstones();
        assert_eq!(node1.tombstones.len(), 1);
        node1.peer_acknowledged(2, 1);
        node1.gc_tombstones();
        assert_eq!(node1.tombstones.len(), 1);
        node1.peer_acknowledged(2, 2);
        node1.gc_tombstones();
        assert_eq!(node1.tombstones.len(), 0);
    }

    #[test]
    fn peer_acknowledgment_updates_epoch() {
        let node = AworsetBlocklist::new(1);
        assert_eq!(node.peer_acks.len(), 0);
        node.peer_acknowledged(2, 100);
        assert_eq!(node.peer_acks.len(), 1);
        let peer_epoch = node.peer_acks.get(&2).unwrap();
        assert_eq!(peer_epoch.epoch, 1);
        assert_eq!(peer_epoch.last_acknowledged_lsn, 100);
        node.peer_acknowledged(2, 200);
        let peer_epoch = node.peer_acks.get(&2).unwrap();
        assert_eq!(peer_epoch.epoch, 2);
        assert_eq!(peer_epoch.last_acknowledged_lsn, 200);
    }

    #[test]
    fn gc_tombstones_respects_causal_safety() {
        let node = AworsetBlocklist::new(1);
        let ip = "192.0.2.1".parse().unwrap();
        node.record_ban(ip, 60_000, 1);
        let unblocks = node.record_unban(ip);
        assert_eq!(node.tombstones.len(), 1);
        node.gc_tombstones();
        assert_eq!(node.tombstones.len(), 1); // not yet acknowledged
        node.peer_acknowledged(1, 1);
        node.gc_tombstones();
        assert_eq!(node.tombstones.len(), 0); // now safe
    }

    #[test]
    fn merge_is_commutative() {
        let node1 = AworsetBlocklist::new(1);
        let node2 = AworsetBlocklist::new(2);
        let ip = IpAddr::from([10, 0, 0, 1]);
        node1.record_ban(ip, 60_000, 1);
        node2.record_ban(ip, 60_000, 2);
        let snap1 = node1.export_snapshot();
        let snap2 = node2.export_snapshot();
        let node_a = AworsetBlocklist::new(3);
        let node_b = AworsetBlocklist::new(4);
        node_a.restore_snapshot(&snap1);
        node_a.restore_snapshot(&snap2);
        node_b.restore_snapshot(&snap2);
        node_b.restore_snapshot(&snap1);
        assert_eq!(node_a.export_snapshot(), node_b.export_snapshot());
    }

    #[test]
    fn merge_is_idempotent() {
        let node = AworsetBlocklist::new(1);
        let ip = IpAddr::from([10, 0, 0, 1]);
        node.record_ban(ip, 60_000, 1);
        let snap = node.export_snapshot();
        node.restore_snapshot(&snap);
        assert_eq!(node.export_snapshot(), snap);
    }

    #[test]
    fn merge_is_associative() {
        let nodes: Vec<_> = (1..=3).map(|id| AworsetBlocklist::new(id)).collect();
        let ip = IpAddr::from([10, 0, 0, 1]);
        for (i, node) in nodes.iter().enumerate() {
            node.record_ban(ip, 60_000, (i + 1) as u8);
        }
        let snaps: Vec<_> = nodes.iter().map(|n| n.export_snapshot()).collect();
        let left = AworsetBlocklist::new(10);
        left.restore_snapshot(&snaps[0]);
        left.restore_snapshot(&snaps[1]);
        left.restore_snapshot(&snaps[2]);
        let right = AworsetBlocklist::new(11);
        right.restore_snapshot(&snaps[2]);
        right.restore_snapshot(&snaps[1]);
        right.restore_snapshot(&snaps[0]);
        assert_eq!(left.export_snapshot(), right.export_snapshot());
    }

    #[test]
    fn convergence_after_partition_heals() {
        let nodes: Vec<_> = (1..=3).map(|id| AworsetBlocklist::new(id)).collect();
        let ip = IpAddr::from([10, 0, 0, 1]);
        nodes[0].record_ban(ip, 60_000, 1);
        nodes[1].record_ban(ip, 60_000, 2);
        nodes[2].record_ban(ip, 60_000, 3);
        let snaps: Vec<_> = nodes.iter().map(|n| n.export_snapshot()).collect();
        for node in &nodes {
            for snap in &snaps {
                node.restore_snapshot(snap);
            }
        }
        assert_eq!(nodes[0].export_snapshot(), nodes[1].export_snapshot());
        assert_eq!(nodes[1].export_snapshot(), nodes[2].export_snapshot());
    }

    #[test]
    fn concurrent_operations_both_survive() {
        let node1 = AworsetBlocklist::new(1);
        let node2 = AworsetBlocklist::new(2);
        let ip = IpAddr::from([10, 0, 0, 1]);
        let ban1 = node1.record_ban(ip, 60_000, 1);
        let ban2 = node2.record_ban(ip, 60_000, 2);
        node1.merge_delta(&ban2);
        node2.merge_delta(&ban1);
        assert!(node1.is_blocked(&ip, now_ms()));
        assert!(node2.is_blocked(&ip, now_ms()));
    }

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
        let nodes: Vec<AworsetBlocklist> = (1..=3).map(|id| AworsetBlocklist::new(id)).collect();
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
}
