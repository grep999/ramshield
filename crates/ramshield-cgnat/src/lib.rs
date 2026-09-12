//! High-Performance Shared Memory Rule Table with OS Fallback
//!
//! Provides a memory-mapped rule table for sub-20ns reverse-proxy
//! lookups and Shannon-entropy analysis to prevent blackholing
//! shared infrastructure (CGNAT / corporate proxy).

pub mod shm;
pub mod cgnat;
