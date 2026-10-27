pub mod command;
pub mod error;
pub mod events;
pub mod ip_network;
pub mod util;

pub use command::{EnforceAction, EnforceCommand, EnforceResult, EnforcementError};
pub use error::{BlockReason, Durability, Result, RsError};
pub use events::{
    BlockDecision, ConnectionEvent, Http2Telemetry, HttpMethod, HttpVersion, L7Metadata,
};
pub use ip_network::IpNetwork;

// Enforcement provenance: external/community reputation is not an authoritative source.
pub use command::EvidenceSource;


/// Process-local monotonic nanosecond clock shared by crates that need to
/// compare freshness or enforce elapsed-time invariants. This is not wall time
/// and must not be persisted as a calendar timestamp.
pub fn monotonic_now_ns() -> u64 {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    EPOCH
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_nanos()
        .min(u64::MAX as u128) as u64
}
