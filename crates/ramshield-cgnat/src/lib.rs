//! High-Performance Shared Memory Rule Table with OS Fallback
//!
//! Provides a memory-mapped rule table for sub-20ns reverse-proxy
//! lookups and Shannon-entropy analysis to prevent blackholing
//! shared infrastructure (CGNAT / corporate proxy).
//!
//! Designed for integration with the ramshield-analytics crate
//! (SubnetHll for IPv6 cardinality, host_bitmap for IPv4).

pub mod cgnat;
pub mod shm;

pub use cgnat::{
    CgnatGuard, CGNAT_TIER_ALLOW, CGNAT_TIER_BLOCK, CGNAT_TIER_CHALLENGE, CGNAT_TIER_XDP_DROP,
};
pub use shm::{ShmRuleEntry, ShmTableManager, FLAG_SHARED_INFRA, SHM_TABLE_CAPACITY};
