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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlockReason {
    Ddos,
    Scan,
    Manual,
    ForecastAnomaly,
    EntropyAnomaly,
    SubnetBatch,
    HighRps(u64),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Durability {
    None,
    Flush,
    Fsync,
    GroupCommit,
}
