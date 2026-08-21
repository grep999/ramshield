//! Enforcement command vocabulary — single source of truth.
//! Wire format: field names/order must stay stable (IPC + WAL consumers).

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforceCommand {
    pub decision_id: Uuid,
    pub policy_version: u64,
    pub source: String,
    pub actor: String,
    pub timestamp_utc: i64,
    pub ttl_seconds: u64,
    pub reason: String,
    pub ip: IpAddr,
    pub action: EnforceAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EnforceAction {
    Block,
    Unblock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforceResult {
    pub decision_id: Uuid,
    pub committed: bool,
    pub applied: bool,
    pub wal_lsn: Option<u64>,
    pub xdp_applied: bool,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum EnforcementError {
    #[error("WAL error: {0}")]
    Wal(String),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("XDP error: {0}")]
    Xdp(String),
    #[error("Duplicate decision_id: {0}")]
    Duplicate(Uuid),
    #[error("Invalid command: {0}")]
    InvalidCommand(String),
}
