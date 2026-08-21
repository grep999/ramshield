use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RsError {
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("capacity exceeded: {limit_mb} MB")]
    CapacityExceeded { limit_mb: usize },
    #[error("serde error: {0}")]
    Serde(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("corrupt wal: {offset}")]
    CorruptWal { offset: u64 },
    #[error("record too large: {size} bytes (max {max})")]
    RecordTooLarge { size: usize, max: usize },
}

pub type Result<T> = std::result::Result<T, RsError>;

/// Canonical block-reason vocabulary. Wire format shared by IPC + WAL.
/// (src shape — the one live constructors use; crate's 7-variant draft deleted.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockReason {
    HighRps,
    SubnetBatch,
    ForecastAnomaly,
    EntropyAnomaly,
    ManualBlock,
}

impl BlockReason {
    /// Stable wire token used in EnforceCommand.reason / WalEntry.reason strings.
    pub fn as_str(&self) -> &'static str {
        match self {
            BlockReason::HighRps => "high_rps",
            BlockReason::SubnetBatch => "subnet_burst",
            BlockReason::ForecastAnomaly => "forecast_anomaly",
            BlockReason::EntropyAnomaly => "entropy_anomaly",
            BlockReason::ManualBlock => "manual",
        }
    }

    /// Inverse of `as_str`; mirrors src/enforcement parse_reason mapping.
    pub fn from_reason_str(r: &str) -> Option<Self> {
        match r {
            "high_rps" | "syn_flood" | "volumetric" | "slowloris" => Some(BlockReason::HighRps),
            "subnet_burst" => Some(BlockReason::SubnetBatch),
            "forecast_anomaly" => Some(BlockReason::ForecastAnomaly),
            "entropy_anomaly" | "anomaly" => Some(BlockReason::EntropyAnomaly),
            "manual" | "manual_unblock" | "manual_block" => Some(BlockReason::ManualBlock),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Durability {
    None,
    Flush,
    Fsync,
    GroupCommit,
}
