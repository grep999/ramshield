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

    /// Record a local ban. Returns the delta to broadcast to peers.
    pub fn record_ban(&self, ip: IpAddr, ttl_ms: u64, tier: u8) -> ClusterBlockDelta {
        let (created_at_ms, _) = self.hlc.tick(0, 0);
        // Never reuse an operation dot when physical time advances. Exhaustion
        // fails closed instead of wrapping and silently violating CRDT identity.
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

        let dot = ClusterDot {
            node_id: self.node_id,
            incarnation: self.incarnation,
            counter,
        };
        self.entries.insert(
            (ip, self.node_id, self.incarnation),
            (counter, created_at_ms, expires_at_ms, tier),
        );

        ClusterBlockDelta {
            ip,
            dot,
            created_at_ms,
            expires_at_ms,
            tier,
        }
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
        let observed: Vec<(u32, Uuid, u64)> = self
            .entries
            .iter()
            .filter(|entry| entry.key().0 == ip)
            .map(|entry| (entry.key().1, entry.key().2, entry.value().0))
            .collect();
        let mut deltas = Vec::with_capacity(observed.len());
        for (node_id, incarnation, counter) in observed {
            let key = (ip, node_id, incarnation);
            self.entries.remove(&key);
            self.tombstones
                .entry(key)
                .and_modify(|existing| *existing = (*existing).max(counter))
                .or_insert(counter);
            deltas.push(ClusterUnblockDelta {
                ip,
                dot: ClusterDot {
                    node_id,
                    incarnation,
                    counter,
                },
            });
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
    }

    #[test]
    fn snapshot_rotates_through_sorted_state() {
        let mesh = AworsetBlocklist::new(1);
        let ips: Vec<_> = (1..10)
            .map(|i| IpAddr::from([203, 0, 113, i]))
            .collect();
        for ip in &ips {
            mesh.record_ban(*ip, 60_000, 1);
        }
        let (page1, _) = mesh.snapshot(3);
        let (page2, _) = mesh.snapshot(3);
        let (page3, _) = mesh.snapshot(3);
        assert_eq!(page1.len() + page2.len() + page3.len(), 9);
        let mut all = page1;
        all.extend(page2);
        all.extend(page3);
        all.sort_by_key(|d| d.ip);
        assert_eq!(all.len(), 9);
    }

    #[test]
    fn snapshot_includes_tombstones() {
        let mesh = AworsetBlocklist::new(1);
        let ip = IpAddr::from([203, 0, 113, 200]);
        let ban = mesh.record_ban(ip, 60_000, 1);
        let _ = mesh.record_unban(ip);
        let (_, tombstones) = mesh.snapshot(16);
        assert_eq!(tombstones.len(), 1);
        assert_eq!(tombstones[0].dot, ban.dot);
    }
}
