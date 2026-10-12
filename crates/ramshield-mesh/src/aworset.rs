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
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use uuid::Uuid;

pub type Dot = u64;
pub(crate) type BlockKey = (IpAddr, u32, Uuid);
pub(crate) type BlockState = (u64, u64, u64, u8);
type SnapshotKey = (IpAddr, u32, Uuid, u8);
/// Stable identity of an item in the deterministic anti-entropy ordering.
///
/// The cursor deliberately excludes the operation counter: counters can change
/// while a page walk is in progress, but the identity key remains stable.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct StateCursor {
    pub ip: IpAddr,
    pub node_id: u32,
    pub incarnation: Uuid,
    pub kind: StateKind,
}

/// Distinguishes a live block from a tombstone for the same CRDT key.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum StateKind {
    Block,
    Unblock,
}

/// One bounded snapshot page and its seek-based continuation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotPage {
    pub blocks: Vec<ClusterBlockDelta>,
    pub unblocks: Vec<ClusterUnblockDelta>,
    /// Last included key when more items remain; None means this pass is complete.
    pub next_cursor: Option<StateCursor>,
    pub has_more: bool,
    pub total: usize,
}

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
    next_dot_counter: AtomicU64,
    snapshot_cursor: AtomicUsize,
}

impl AworsetBlocklist {
    pub fn new(node_id: u32) -> Self {
        Self {
            node_id,
            incarnation: Uuid::new_v4(),
            hlc: Hlc::new(),
            entries: DashMap::new(),
            tombstones: DashMap::new(),
            next_dot_counter: AtomicU64::new(0),
            snapshot_cursor: AtomicUsize::new(0),
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
        if delta.dot.node_id == 0 || delta.dot.incarnation.is_nil() {
            return false;
        }
        self.hlc.tick(0, 0);
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
    /// Return a bounded anti-entropy page for a caller-owned cursor.
    ///
    /// Unlike `snapshot`, this method does not share cursor state between
    /// callers. A transport can therefore keep one cursor per peer and avoid
    /// letting a fast peer advance another peer's repair position. The cursor
    /// is an index into a deterministic sorted snapshot, not a durable
    /// acknowledgement; concurrent mutations may cause duplicates or skips,
    /// which periodic full rotations must repair.
    pub fn snapshot_page(&self, cursor: Option<&StateCursor>, limit: usize) -> SnapshotPage {
        enum SnapshotItem {
            Block(ClusterBlockDelta),
            Unblock(ClusterUnblockDelta),
        }

        let mut items: Vec<(StateCursor, SnapshotItem)> =
            Vec::with_capacity(self.entries.len() + self.tombstones.len());
        for entry in self.entries.iter() {
            let key = *entry.key();
            items.push((
                StateCursor {
                    ip: key.0,
                    node_id: key.1,
                    incarnation: key.2,
                    kind: StateKind::Block,
                },
                SnapshotItem::Block(ClusterBlockDelta {
                    ip: key.0,
                    dot: ClusterDot {
                        node_id: key.1,
                        incarnation: key.2,
                        counter: entry.value().0,
                    },
                    created_at_ms: entry.value().1,
                    expires_at_ms: entry.value().2,
                    tier: entry.value().3,
                }),
            ));
        }
        for tombstone in self.tombstones.iter() {
            let key = *tombstone.key();
            items.push((
                StateCursor {
                    ip: key.0,
                    node_id: key.1,
                    incarnation: key.2,
                    kind: StateKind::Unblock,
                },
                SnapshotItem::Unblock(ClusterUnblockDelta {
                    ip: key.0,
                    dot: ClusterDot {
                        node_id: key.1,
                        incarnation: key.2,
                        counter: *tombstone.value(),
                    },
                }),
            ));
        }
        items.sort_unstable_by(|left, right| left.0.cmp(&right.0));

        let total = items.len();
        if limit == 0 {
            return SnapshotPage {
                total,
                ..SnapshotPage::default()
            };
        }

        let start = match cursor {
            Some(cursor) => items.partition_point(|(key, _)| key <= cursor),
            None => 0,
        };
        let available = total.saturating_sub(start);
        let count = limit.min(available);
        let has_more = available > count;
        let next_cursor = if has_more {
            count
                .checked_sub(1)
                .and_then(|last| items.get(start + last))
                .map(|(key, _)| key.clone())
        } else {
            None
        };

        let mut blocks = Vec::with_capacity(count);
        let mut unblocks = Vec::with_capacity(count);
        for (_, item) in items.into_iter().skip(start).take(count) {
            match item {
                SnapshotItem::Block(block) => blocks.push(block),
                SnapshotItem::Unblock(unblock) => unblocks.push(unblock),
            }
        }
        SnapshotPage {
            blocks,
            unblocks,
            next_cursor,
            has_more,
            total,
        }
    }

    /// Return a bounded, rotating anti-entropy page.
    ///
    /// Items are sorted before paging so a stable state has deterministic order.
    /// The cursor advances between calls, preventing a fixed map prefix from
    /// starving keys beyond the per-sync budget. Concurrent state changes can
    /// cause duplicates or temporary skips; repeated rotations repair those
    /// changes, while delivery acknowledgement is a separate concern.
    pub fn snapshot(&self, limit: usize) -> (Vec<ClusterBlockDelta>, Vec<ClusterUnblockDelta>) {
        enum SnapshotItem {
            Block(ClusterBlockDelta),
            Unblock(ClusterUnblockDelta),
        }

        if limit == 0 {
            return (Vec::new(), Vec::new());
        }

        let mut items: Vec<(SnapshotKey, SnapshotItem)> =
            Vec::with_capacity(self.entries.len() + self.tombstones.len());
        for entry in self.entries.iter() {
            let key = *entry.key();
            items.push((
                (key.0, key.1, key.2, 0),
                SnapshotItem::Block(ClusterBlockDelta {
                    ip: key.0,
                    dot: ClusterDot {
                        node_id: key.1,
                        incarnation: key.2,
                        counter: entry.value().0,
                    },
                    created_at_ms: entry.value().1,
                    expires_at_ms: entry.value().2,
                    tier: entry.value().3,
                }),
            ));
        }
        for tombstone in self.tombstones.iter() {
            let key = *tombstone.key();
            items.push((
                (key.0, key.1, key.2, 1),
                SnapshotItem::Unblock(ClusterUnblockDelta {
                    ip: key.0,
                    dot: ClusterDot {
                        node_id: key.1,
                        incarnation: key.2,
                        counter: *tombstone.value(),
                    },
                }),
            ));
        }
        items.sort_unstable_by_key(|(key, _)| *key);

        let total = items.len();
        if total == 0 {
            return (Vec::new(), Vec::new());
        }
        let start = self.snapshot_cursor.fetch_add(limit, Ordering::Relaxed) % total;
        let count = limit.min(total);
        let mut blocks = Vec::with_capacity(count);
        let mut unblocks = Vec::with_capacity(count);
        for offset in 0..count {
            match &items[(start + offset) % total].1 {
                SnapshotItem::Block(block) => blocks.push(block.clone()),
                SnapshotItem::Unblock(unblock) => unblocks.push(unblock.clone()),
            }
        }
        (blocks, unblocks)
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
                .map(|v| *v),
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
    fn snapshot_rotates_through_more_entries_than_one_sync_budget() {
        const TOTAL: usize = 5_000;
        const PAGE: usize = 1_024;
        let mesh = AworsetBlocklist::new(7);
        for n in 0..TOTAL {
            let ip = IpAddr::V6(std::net::Ipv6Addr::from(n as u128));
            mesh.record_ban(ip, 60_000, 1);
        }

        let mut observed = std::collections::HashSet::new();
        for _ in 0..TOTAL.div_ceil(PAGE) {
            let (blocks, unblocks) = mesh.snapshot(PAGE);
            assert!(blocks.len() + unblocks.len() <= PAGE);
            observed.extend(blocks.into_iter().map(|delta| delta.ip));
        }
        assert_eq!(observed.len(), TOTAL);
    }

    #[test]
    fn snapshot_rotates_through_mixed_blocks_and_tombstones() {
        const EACH: usize = 2_500;
        const PAGE: usize = 1_024;
        let mesh = AworsetBlocklist::new(7);
        let block_incarnation = Uuid::new_v4();
        let tombstone_incarnation = Uuid::new_v4();
        for n in 0..EACH {
            let block_ip = IpAddr::V6(std::net::Ipv6Addr::from(n as u128));
            mesh.entries.insert(
                (block_ip, 7, block_incarnation),
                (n as u64 + 1, 0, u64::MAX, 2),
            );

            let tombstone_ip = IpAddr::V6(std::net::Ipv6Addr::from((n + 10_000) as u128));
            mesh.tombstones
                .insert((tombstone_ip, 7, tombstone_incarnation), n as u64 + 1);
        }

        let mut observed_blocks = std::collections::HashSet::new();
        let mut observed_tombstones = std::collections::HashSet::new();
        for _ in 0..(EACH * 2).div_ceil(PAGE) {
            let (blocks, unblocks) = mesh.snapshot(PAGE);
            assert!(blocks.len() + unblocks.len() <= PAGE);
            observed_blocks.extend(blocks.into_iter().map(|delta| delta.ip));
            observed_tombstones.extend(unblocks.into_iter().map(|delta| delta.ip));
        }
        assert_eq!(observed_blocks.len(), EACH);
        assert_eq!(observed_tombstones.len(), EACH);
    }

    #[test]
    fn snapshot_page_cursors_are_independent_and_bounded() {
        let mesh = AworsetBlocklist::new(7);
        for n in 0..5 {
            let ip = IpAddr::V6(std::net::Ipv6Addr::from(n as u128));
            mesh.record_ban(ip, 60_000, 1);
        }

        let peer_a_first = mesh.snapshot_page(None, 2);
        let peer_b_first = mesh.snapshot_page(None, 2);
        assert_eq!(peer_a_first, peer_b_first);
        assert_eq!(peer_a_first.total, 5);
        assert_eq!(peer_a_first.blocks.len() + peer_a_first.unblocks.len(), 2);
        assert!(peer_a_first.has_more);
        let peer_a_cursor = peer_a_first
            .next_cursor
            .clone()
            .expect("second page exists");

        let peer_a_second = mesh.snapshot_page(Some(&peer_a_cursor), 2);
        assert_eq!(peer_a_second.total, 5);
        assert_eq!(peer_a_second.blocks.len() + peer_a_second.unblocks.len(), 2);
        assert!(peer_a_second.has_more);
        assert_ne!(
            peer_a_first.blocks[0].ip, peer_a_second.blocks[0].ip,
            "advancing one peer cursor should select a different page"
        );
        assert_eq!(mesh.snapshot_page(None, 2), peer_b_first);
    }

    #[test]
    fn snapshot_page_seek_survives_mutations_before_and_after_cursor() {
        let mesh = AworsetBlocklist::new(7);
        for n in 0..6 {
            let ip = IpAddr::V6(std::net::Ipv6Addr::from(n as u128));
            mesh.record_ban(ip, 60_000, 1);
        }

        let first = mesh.snapshot_page(None, 2);
        let cursor = first.next_cursor.clone().expect("more items remain");
        let last_first_ip = first.blocks.last().expect("first page has blocks").ip;

        // Add one key on either side of the cursor, and remove an earlier key.
        mesh.record_ban(IpAddr::V6(std::net::Ipv6Addr::from(0u128)), 60_000, 1);
        mesh.record_ban(IpAddr::V6(std::net::Ipv6Addr::from(100u128)), 60_000, 1);
        mesh.record_unban(IpAddr::V6(std::net::Ipv6Addr::from(0u128)));

        let second = mesh.snapshot_page(Some(&cursor), 16);
        assert!(second.blocks.iter().all(|delta| {
            StateCursor {
                ip: delta.ip,
                node_id: delta.dot.node_id,
                incarnation: delta.dot.incarnation,
                kind: StateKind::Block,
            } > cursor
        }));
        assert!(
            second
                .blocks
                .iter()
                .any(|delta| { delta.ip == IpAddr::V6(std::net::Ipv6Addr::from(100u128)) })
        );
        assert!(!second.blocks.iter().any(|delta| delta.ip == last_first_ip));
        assert!(!second.has_more);
        assert!(second.next_cursor.is_none());
    }

    #[test]
    fn snapshot_page_covers_large_mixed_state_exactly_once() {
        const EACH: usize = 2_500;
        const LIMIT: usize = 4_096;
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
            mesh.tombstones
                .insert((tombstone_ip, 7, tombstone_incarnation), n as u64 + 1);
            expected_tombstones.insert(tombstone_ip);
        }

        let first = mesh.snapshot_page(None, LIMIT);
        assert_eq!(first.total, EACH * 2);
        assert_eq!(first.blocks.len() + first.unblocks.len(), LIMIT);
        assert!(first.has_more);
        let cursor = first.next_cursor.clone().expect("second page exists");

        let second = mesh.snapshot_page(Some(&cursor), LIMIT);
        assert_eq!(second.total, EACH * 2);
        assert_eq!(
            second.blocks.len() + second.unblocks.len(),
            EACH * 2 - LIMIT
        );
        assert!(!second.has_more);
        assert!(second.next_cursor.is_none());

        let observed_blocks: std::collections::HashSet<_> = first
            .blocks
            .into_iter()
            .chain(second.blocks)
            .map(|delta| delta.ip)
            .collect();
        let observed_tombstones: std::collections::HashSet<_> = first
            .unblocks
            .into_iter()
            .chain(second.unblocks)
            .map(|delta| delta.ip)
            .collect();

        assert_eq!(observed_blocks, expected_blocks);
        assert_eq!(observed_tombstones, expected_tombstones);
    }

    #[test]
    fn snapshot_carries_active_dot_and_tombstone() {
        let mesh = AworsetBlocklist::new(7);
        let ip = IpAddr::from([192, 0, 2, 7]);
        let ban = mesh.record_ban(ip, 60_000, 3);
        let (blocks, _) = mesh.snapshot(16);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].dot, ban.dot);
        assert_eq!(blocks[0].tier, ban.tier);
        let unblocks = mesh.record_unban(ip);
        assert_eq!(unblocks.len(), 1);
        let (_, tombstones) = mesh.snapshot(16);
        assert_eq!(tombstones.len(), 1);
        assert_eq!(tombstones[0].dot, ban.dot);
    }
}
