use thiserror::Error;

#[derive(Error, Debug)]
pub enum RamShieldError {
    #[error("Configuration Error: {0}")]
    ConfigError(String),
    #[error("Storage Error: {0}")]
    StorageError(String),
    #[error("Detection Error: {0}")]
    DetectionError(String),
    #[error("Forecasting Error: {0}")]
    ForecastingError(String),
    #[error("Dashboard Error: {0}")]
    DashboardError(String),
    #[error("IPC Error: {0}")]
    IpcError(String),
    #[error("Generic Error: {0}")]
    GenericError(String),
    #[error("Io Error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Serde Json Error: {0}")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("Toml De Error: {0}")]
    TomlDeError(#[from] toml::de::Error),
    #[error("System info Error: {0}")]
    SysinfoError(String),
    #[error("Other Error: {0}")]
    Other(#[from] anyhow::Error),
}
