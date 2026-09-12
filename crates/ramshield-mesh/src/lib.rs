//! Fleet Federation via AWORSet CRDT & Hybrid Logical Clock (HLC)
//!
//! Distributed blocklist sync: Add-Wins Observed-Remove Set backed by
//! DashMap, with a burst-safe HLC (64-bit ms + 32-bit logical counter).

pub mod hlc;
pub mod aworset;