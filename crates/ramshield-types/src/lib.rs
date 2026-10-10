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
