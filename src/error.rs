use thiserror::Error;

#[derive(Debug, Error)]
pub enum RsError {
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("RAM limit reached ({limit_mb} MB)")]
    CapacityExceeded { limit_mb: usize },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(String),
    #[error("IPC error: {0}")]
    Ipc(String),
    #[error("corrupt WAL at offset {offset}")]
    CorruptWal { offset: u64 },
    #[error("engine is shutting down")]
    Shutdown,
}

pub type Result<T> = std::result::Result<T, RsError>;
