use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::Arc;

mod env;
mod net;
mod sections;
mod validate;

pub use net::{is_loopback_bind, peer_is_trusted_proxy, xff_client};
pub use sections::{
    AutonomousConfig, DashboardConfig, DetectionConfig, EngineConfig, ForecastingConfig, IpcConfig,
    KeyRole, KeyRoleConfig, L7Rule, MeshConfig, NativeIngestConfig, SynproxyConfig, UpstreamConfig,
    WalConfig, XdpConfig,
};

use net::is_public_bind;

pub type ConfigHandle = Arc<ArcSwap<Config>>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub engine: EngineConfig,
    #[serde(default)]
    pub detection: DetectionConfig,
    #[serde(default)]
    pub xdp: XdpConfig,
    #[serde(default)]
    pub ipc: IpcConfig,
    #[serde(default)]
    pub forecasting: ForecastingConfig,
    #[serde(default)]
    pub wal: WalConfig,
    #[serde(default)]
    pub dashboard: DashboardConfig,
    #[serde(default)]
    pub mesh: MeshConfig,
    #[serde(default)]
    pub upstream: UpstreamConfig,
    #[serde(default)]
    pub autonomous: AutonomousConfig,
    #[serde(default)]
    pub native_ingest: NativeIngestConfig,
    #[serde(default)]
    pub synproxy: SynproxyConfig,
}

/// Sentinel used by the dashboard's GET /api/config for secret fields.
/// api_set_config rejects any patch containing it (POST-back of a viewed
/// config must never boot an auth-less server). pub so both sides share it.
pub const REDACTED_PLACEHOLDER: &str = "<redacted>";

impl Config {
    pub fn from_toml_file(path: &str) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Load config from file then apply environment variable overrides.
    /// Env vars take precedence: RAMSHIELD_ENGINE__RAM_LIMIT_MB=1024
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let mut cfg = Self::from_toml_file(path)?;
        cfg.apply_env_overrides()?;
        // P1 fix: file validation runs before overrides, so the final merged
        // configuration is validated again below. Typed env parsing itself is
        // fail-fast and returns an error before validation can be bypassed.
        // RAMSHIELD_IPC__TCP_ADDR=0.0.0.0:7890 (or dashboard addr) without
        // auth keys/hash therefore silently defeated the fail-closed
        // public-bind guard. Validate the FINAL config or fail startup.
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn into_handle(self) -> ConfigHandle {
        Arc::new(ArcSwap::from_pointee(self))
    }
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests;
