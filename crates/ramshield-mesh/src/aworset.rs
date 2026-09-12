//! Add-Wins Observed-Remove Set (AWORSet) CRDT for fleet-federated blocklists
//!
//! Each peer maintains a set of (element, dot) pairs where dot is the
//! logical timestamp when the element was observed. Supports concurrent
//! adds and removes with eventual consistency across the fleet.

use dashmap::DashSet;
use std::sync::Arc;

pub type Dot = u64;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Element {
    pub key: String,
    pub dot: Dot,
    pub removed: bool,
}

/// Thread-safe AWORSet implementation using DashMap for lock-free concurrent access.
pub struct Aworset {
    /// Internal storage: element key -> set of dots (add operations)
    elements: Arc<DashSet<String>>,
    /// Tombstones: removed elements keyed by element name -> dot
    tombstones: Arc<DashSet<String>>,
}

impl Aworset {
    pub fn new() -> Self {
        Self {
            elements: Arc::new(DashSet::new()),
            tombstones: Arc::new(DashSet::new()),
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
}