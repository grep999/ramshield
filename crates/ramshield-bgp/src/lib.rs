/// RamShield BGP Engine — minimal eBGP speaker for DDoS mitigation signaling.
///
/// Implements only two orthodox BGP primitives that every Tier-1 carrier
/// supports natively in hardware:
///   - RTBH (RFC 7999): BLACKHOLE community 65535:666 + NO_EXPORT 65535:65281
///   - Flowspec (RFC 8955/8956): surgical L4 filtering via NLRI components
///
/// Does NOT import routes, calculate routing tables, or modify kernel FIB.
/// Maintains a single TCP/179 session and fires pre-formatted byte buffers
/// when triggered by the Schmitt trigger controller.
pub mod attributes;
pub mod config;
pub mod controller;
pub mod flowspec;
pub mod fsm;
pub mod messages;
pub mod rtbh;
pub mod session;

#[cfg(test)]
mod tests {
    use super::*;
}

pub use attributes::PathAttr;
pub use config::BgpConfig;
pub use flowspec::{FlowComponent, FlowSpecNLRI};
pub use fsm::{BgpFsm, BgpState, TransitionAction};
pub use messages::{BgpMessage, BgpMsgType};
pub use rtbh::{RtbhCommunity, announce_rtbh};
pub use session::BgpSession;

/// Result type alias for BGP operations.
pub type Result<T> = std::result::Result<T, BgpError>;

#[derive(Debug, thiserror::Error)]
pub enum BgpError {
    #[error("bad magic: {0:?}")]
    BadMagic([u8; 2]),
    #[error("bad version: {0}")]
    BadVersion(u8),
    #[error("bad message type: {0}")]
    BadType(u8),
    #[error("message too short: expected {expected}, got {got}")]
    TooShort { expected: usize, got: usize },
    #[error("hmac verification failed")]
    HmacFailed,
    #[error("bad flowspec component type: {0}")]
    BadComponent(u8),
    #[error("bad operator encoding: {0}")]
    BadOperator(u8),
    #[error("flowspec component ordering violation")]
    ComponentOrdering,
    #[error("invalid prefix length: {0}")]
    InvalidPrefixLen(u8),
    #[error("session error: {0}")]
    Session(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("addr parse error: {0}")]
    AddrParse(#[from] std::net::AddrParseError),
}
