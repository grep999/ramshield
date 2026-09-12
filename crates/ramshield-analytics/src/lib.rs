//! High-throughput streaming analytics with bounded memory.
//!
//! HyperLogLog for cardinality estimation per subnet (IPv6) in 1024 bytes.
//! Decaying Count-Min Sketch for heavy-hitter frequency tracking (512 KiB).
//! Exponentially Weighted Welford for dynamic baseline (32 bytes).
//! Replaces unbounded HashSet<IpAddr> detection with O(1) space.

pub mod hll;
pub mod cms;
pub mod welford;
