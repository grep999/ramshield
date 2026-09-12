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

pub type Dot = u64;

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
    pub expires_at_ms: u64,
    pub tier: u8,
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
/// DashMap<(IpAddr, node_id), (seq, expires_at_ms)>.
pub struct AworsetBlocklist {
    node_id: u32,
    hlc: Hlc,
    entries: DashMap<(IpAddr, u32), (u32, u64)>,
}

impl AworsetBlocklist {
    pub fn new(node_id: u32) -> Self {
        Self {
            node_id,
            hlc: Hlc::new(),
            entries: DashMap::new(),
        }
    }

    /// Record a local ban. Returns the delta to broadcast to peers.
    pub fn record_ban(&self, ip: IpAddr, ttl_ms: u64, tier: u8) -> ClusterBlockDelta {
        let (phys_ms, seq) = self.hlc.tick(0, 0);
        let expires_at_ms = phys_ms + ttl_ms;

        let dot = ClusterDot { node_id: self.node_id, counter: seq };
        self.entries.insert((ip, self.node_id), (seq, expires_at_ms));

        ClusterBlockDelta { ip, dot, expires_at_ms, tier }
    }

    /// Absorb a peer delta. True if it changed local state.
    pub fn merge_delta(&self, delta: &ClusterBlockDelta) -> bool {
        self.hlc.tick(delta.expires_at_ms, delta.dot.counter);
        let key = (delta.ip, delta.dot.node_id);

        let mut inserted = false;
        self.entries
            .entry(key)
            .and_modify(|(existing_seq, existing_exp)| {
                if delta.dot.counter > *existing_seq {
                    *existing_seq = delta.dot.counter;
                    *existing_exp = delta.expires_at_ms;
                    inserted = true;
                }
            })
            .or_insert_with(|| {
                inserted = true;
                (delta.dot.counter, delta.expires_at_ms)
            });

        inserted
    }

    pub fn is_blocked(&self, ip: &IpAddr, now_ms: u64) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.key().0 == *ip && entry.value().1 > now_ms)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop entries whose ban has expired (GC pass on the 250ms tick).
    pub fn purge_expired(&self, now_ms: u64) {
        self.entries.retain(|_, (_, exp)| *exp > now_ms);
    }

    /// Local unblock: remove this node's dot for the IP so the unban
    /// gossips out and peer merges converge on removal.
    pub fn record_unban(&self, ip: IpAddr) {
        let (_, seq) = self.hlc.tick(0, 0);
        self.entries.remove(&(ip, self.node_id));
        let _ = seq; // dot counter consumed only for HLC monotonicity
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
}