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
use std::time::{SystemTime, UNIX_EPOCH};

pub type Dot = u64;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Element {
    pub key: String,
    pub dot: Dot,
    pub removed: bool,
}

/// Per-node logical timestamp: (node id, HLC logical sequence).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClusterDot {
    pub node_id: u32,
    pub counter: u32,
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
/// block dot for the same (ip,node) whose counter is <= this counter.
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

/// IP-keyed blocklist CRDT: ban dots per (ip, node), HLC-ordered so the
/// highest counter per node wins on merge. Internal map is
/// DashMap<(IpAddr, node_id), (seq, created_at_ms, expires_at_ms)>.
pub struct AworsetBlocklist {
    node_id: u32,
    hlc: Hlc,
    pub(crate) entries: DashMap<(IpAddr, u32), (u32, u64, u64)>,
    pub(crate) tombstones: DashMap<(IpAddr, u32), u32>,
    tombstone_times: DashMap<(IpAddr, u32), u64>,
}

impl AworsetBlocklist {
    pub fn new(node_id: u32) -> Self {
        Self {
            node_id,
            hlc: Hlc::new(),
            entries: DashMap::new(),
            tombstones: DashMap::new(),
            tombstone_times: DashMap::new(),
        }
    }

    /// Record a local ban. Returns the delta to broadcast to peers.
    pub fn record_ban(&self, ip: IpAddr, ttl_ms: u64, tier: u8) -> ClusterBlockDelta {
        let (created_at_ms, seq) = self.hlc.tick(0, 0);
        let expires_at_ms = if ttl_ms == 0 {
            u64::MAX
        } else {
            created_at_ms.saturating_add(ttl_ms)
        };

        let dot = ClusterDot {
            node_id: self.node_id,
            counter: seq,
        };
        self.entries
            .insert((ip, self.node_id), (seq, created_at_ms, expires_at_ms));

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
        self.hlc.tick(delta.created_at_ms, delta.dot.counter);
        let key = (delta.ip, delta.dot.node_id);
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
            .and_modify(|(existing_seq, existing_created, existing_exp)| {
                if delta.dot.counter > *existing_seq {
                    *existing_seq = delta.dot.counter;
                    *existing_created = delta.created_at_ms;
                    *existing_exp = delta.expires_at_ms;
                    inserted = true;
                }
            })
            .or_insert_with(|| {
                inserted = true;
                (delta.dot.counter, delta.created_at_ms, delta.expires_at_ms)
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

    /// Drop entries whose ban has expired (GC pass on the 250ms tick).
    ///
    /// F6-fix (audit /tmp/oo Frontier 2): entries are pruned only after
    /// TOMBSTONE_HORIZON_MS past expiry, not at the exact expiry ms.
    /// Premature deletion + delayed gossip = resurrection: a peer's stale
    /// ban delta arriving after local pruning is treated as NEW and
    /// re-bans the IP. The horizon must exceed max network delay + clock
    /// skew across the fleet.
    pub fn purge_expired(&self, now_ms: u64) {
        const TOMBSTONE_HORIZON_MS: u64 = 86_400_000;
        self.entries
            .retain(|_, (_, _, exp)| now_ms.saturating_sub(*exp) < TOMBSTONE_HORIZON_MS);
        self.tombstone_times.retain(|key, created| {
            let keep = now_ms.saturating_sub(*created) < TOMBSTONE_HORIZON_MS;
            if !keep { self.tombstones.remove(key); }
            keep
        });
    }

    /// Local unblock: create a tombstone delta for the observed local dot.
    /// Callers must gossip the returned delta; removing the local entry alone
    /// cannot make an AWORSet converge across peers.
    pub fn record_unban(&self, ip: IpAddr) -> Vec<ClusterUnblockDelta> {
        let observed: Vec<(u32, u32)> = self
            .entries
            .iter()
            .filter(|entry| entry.key().0 == ip)
            .map(|entry| (entry.key().1, entry.value().0))
            .collect();
        let mut deltas = Vec::with_capacity(observed.len());
        for (node_id, counter) in observed {
            let key = (ip, node_id);
            self.entries.remove(&key);
            self.tombstones
                .entry(key)
                .and_modify(|existing| *existing = (*existing).max(counter))
                .or_insert(counter);
            self.tombstone_times.insert(key, now_ms());
            deltas.push(ClusterUnblockDelta {
                ip,
                dot: ClusterDot { node_id, counter },
            });
        }
        deltas
    }

    /// Merge an observed-remove tombstone.
    pub fn merge_unblock_delta(&self, delta: &ClusterUnblockDelta) -> bool {
        self.hlc.tick(0, delta.dot.counter);
        let key = (delta.ip, delta.dot.node_id);
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
        if changed { self.tombstone_times.insert(key, now_ms()); }
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
    /// Bounded anti-entropy snapshot used by the authenticated transport.
    pub fn snapshot(&self, limit: usize) -> (Vec<ClusterBlockDelta>, Vec<ClusterUnblockDelta>) {
        let mut blocks = Vec::new();
        for e in self.entries.iter().take(limit) {
            blocks.push(ClusterBlockDelta {
                ip: e.key().0,
                dot: ClusterDot { node_id: e.key().1, counter: e.value().0 },
                created_at_ms: e.value().1,
                expires_at_ms: e.value().2,
                tier: 1,
            });
        }
        let mut unblocks = Vec::new();
        for e in self.tombstones.iter().take(limit.saturating_sub(blocks.len())) {
            unblocks.push(ClusterUnblockDelta {
                ip: e.key().0,
                dot: ClusterDot { node_id: e.key().1, counter: *e.value() },
            });
        }
        (blocks, unblocks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(finite.expires_at_ms.saturating_sub(finite.created_at_ms), 60_000);
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
        d2.dot.counter += 1;
        d2.expires_at_ms += 60_000;

        assert!(a.merge_delta(&d2), "higher counter must merge in");
        // Stale delta from node 2 must NOT overwrite.
        let mut d3 = d2.clone();
        d3.dot.counter -= 1;
        assert!(!a.merge_delta(&d3), "lower counter must be rejected");
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
    fn snapshot_carries_active_dot_and_tombstone() {
        let mesh = AworsetBlocklist::new(7);
        let ip = IpAddr::from([192, 0, 2, 7]);
        let ban = mesh.record_ban(ip, 60_000, 1);
        let (blocks, _) = mesh.snapshot(16);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].dot, ban.dot);
        let unblocks = mesh.record_unban(ip);
        assert_eq!(unblocks.len(), 1);
        let (_, tombstones) = mesh.snapshot(16);
        assert_eq!(tombstones.len(), 1);
        assert_eq!(tombstones[0].dot, ban.dot);
    }

}
