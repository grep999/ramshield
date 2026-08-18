use crate::ip_network::IpNetwork;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionEvent {
    pub ip: IpAddr,
    pub timestamp_ns: u64,
    pub bytes: u64,
    pub status_code: u16,
    pub proto_fingerprint: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDecision {
    pub ip: IpAddr,
    pub reason: crate::error::BlockReason,
    pub ttl_secs: Option<u64>,
    /// Batch subnet that triggered the block (IPv4 /24 or IPv6 /64).
    pub batch_subnet: Option<IpNetwork>,
}
