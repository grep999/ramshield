// ── Shared types ────────────────────────────────────────────────────────────

use std::net::IpAddr;
use std::time::Instant;

/// An authentication event from an external source (syslog, TCP, stdin).
#[derive(Debug, Clone)]
pub struct AuthEvent {
    pub ip: IpAddr,
    pub timestamp: Instant,
    /// Service the attempt hit: "sshd", "nginx", "custom".
    pub source: String,
    /// true = login succeeded, false = failure.
    pub success: bool,
    pub username: Option<String>,
}

/// Command to block an IP (emitted by detection, consumed by enforcement).
#[derive(Debug, Clone)]
pub struct BlockCommand {
    pub ip: IpAddr,
    pub reason: BlockReason,
    pub ttl_secs: u64,
    pub context: String,
}

/// Why a block was issued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockReason {
    BruteForce,
    BehavioralAnomaly,
    Manual,
}

impl BlockReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            BlockReason::BruteForce => "brute_force",
            BlockReason::BehavioralAnomaly => "behavioral_anomaly",
            BlockReason::Manual => "manual",
        }
    }
}