//! Enforcement command vocabulary — single source of truth.
//! Wire format: field names/order must stay stable (IPC + WAL consumers).

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use crate::IpNetwork;

/// Provenance of an enforcement decision. Community reputation is deliberately
/// not representable here: RamShield decisions must originate from local
/// telemetry, trusted RamShield fleet evidence, or an explicit operator.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceSource {
    #[default]
    LocalSignals,
    FleetSignals,
    Operator,
}

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
    #[serde(default)]
    pub cidr: Option<IpNetwork>,
    pub action: EnforceAction,
    /// First-party provenance. External/community reputation is intentionally
    /// excluded from the enforcement vocabulary.
    #[serde(default)]
    pub evidence_source: EvidenceSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EnforceAction {
    Block,
    Unblock,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforcement_provenance_excludes_external_reputation() {
        let local = EvidenceSource::LocalSignals;
        let fleet = EvidenceSource::FleetSignals;
        let operator = EvidenceSource::Operator;
        assert_ne!(local, fleet);
        assert_ne!(local, operator);
        assert_ne!(fleet, operator);
        let encoded = serde_json::to_string(&fleet).unwrap();
        assert_eq!(encoded, "\"FleetSignals\"");
    }

    #[test]
    fn cidr_target_preserves_normalized_network() {
        let cidr = IpNetwork::new("192.0.2.17".parse().unwrap(), 24).unwrap();
        assert_eq!(cidr.addr.to_string(), "192.0.2.0");
    }
}
