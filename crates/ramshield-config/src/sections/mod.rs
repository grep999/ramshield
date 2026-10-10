//! Per-section configuration structs and their serde defaults.

mod dashboard;
mod detection;
mod engine;
mod forecasting;
mod ipc;
mod security;
mod wal;
mod xdp;

pub use dashboard::DashboardConfig;
pub use detection::{DetectionConfig, L7Rule};
pub use engine::EngineConfig;
pub use forecasting::ForecastingConfig;
pub use ipc::{IpcConfig, KeyRole, KeyRoleConfig};
pub use security::{
    AutonomousConfig, MeshConfig, NativeIngestConfig, SynproxyConfig, UpstreamConfig,
};
pub use wal::WalConfig;
pub use xdp::XdpConfig;
