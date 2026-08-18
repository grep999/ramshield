use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    Block(EnforcementCommand),
    Unblock(EnforcementCommand),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforcementCommand {
    pub decision_id: Uuid,
    pub policy_version: u64,
    pub source: String,
    pub actor: String,
    pub timestamp_utc: i64,
    pub ttl_seconds: u64,
    pub reason: String,
    pub ip: IpAddr,
}
