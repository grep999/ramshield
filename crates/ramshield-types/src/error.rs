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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlockReason {
    HighRps,
    SubnetBatch,
    ForecastAnomaly,
    EntropyAnomaly,
    ManualBlock,
    L7HighRps,
    L7Cost,
    Http2StreamAbuse,
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
            BlockReason::L7HighRps => "l7_high_rps",
            BlockReason::L7Cost => "l7_cost",
            BlockReason::Http2StreamAbuse => "http2_stream_abuse",
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
            "l7_high_rps" => Some(BlockReason::L7HighRps),
            "l7_cost" => Some(BlockReason::L7Cost),
            "http2_stream_abuse" => Some(BlockReason::Http2StreamAbuse),
            // Mesh gossip emits its own reason strings; no canonical variant
            // exists (wire-shared enum). ManualBlock fallback is the intended
            // safe behavior — recognizing the tokens quiets spurious WARNs.
            "mesh_final" | "mesh_purge" | "mesh-final" | "mesh-purge" => {
                Some(BlockReason::ManualBlock)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Durability {
    None,
    Flush,
    Fsync,
    #[default]
    GroupCommit,
}

#[cfg(test)]
mod block_reason_tests {
    use super::BlockReason;

    #[test]
    fn l7_cost_reason_round_trips() {
        assert_eq!(
            BlockReason::from_reason_str("l7_cost"),
            Some(BlockReason::L7Cost)
        );
        assert_eq!(BlockReason::L7Cost.as_str(), "l7_cost");
    }
}
