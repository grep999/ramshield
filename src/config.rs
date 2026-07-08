use std::env;
use serde::{Deserialize, Serialize};
use anyhow::Result;
use crate::error::RamShieldError;

const MB_MIN: usize = 64;
const SHARDS_MIN: usize = 16;

pub type ConfigHandle = std::sync::Arc<parking_lot::RwLock<Config>>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)] pub engine:      EngineConfig,
    #[serde(default)] pub detection:   DetectionConfig,
    #[serde(default)] pub storage:     StorageConfig,
    #[serde(default)] pub ipc:         IpcConfig,
    #[serde(default)] pub forecasting: ForecastingConfig,
    #[serde(default)] pub dashboard:   DashboardConfig,
    #[serde(default)] pub alerting:    AlertingConfig,
}

impl Config {
    pub fn load_from_file(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let mut cfg: Config = toml::from_str(&text)?;
        cfg.apply_env_overrides();
        cfg.validate()?;
        Ok(cfg)
    }

    /// Apply environment variable overrides using RAMSHIELD_ prefix.
    /// Example: RAMSHIELD_DETECTION__RPS_THRESHOLD=500
    pub fn apply_env_overrides(&mut self) {
        if let Ok(v) = env::var("RAMSHIELD_DETECTION__RPS_THRESHOLD") {
            if let Ok(parsed) = v.parse::<u64>() { self.detection.rps_threshold = parsed; }
        }
        if let Ok(v) = env::var("RAMSHIELD_ENGINE__RAM_LIMIT_MB") {
            if let Ok(parsed) = v.parse::<usize>() { self.engine.ram_limit_mb = parsed; }
        }
        if let Ok(v) = env::var("RAMSHIELD_ENGINE__SHARD_COUNT") {
            if let Ok(parsed) = v.parse::<usize>() { self.engine.shard_count = parsed; }
        }
        if let Ok(v) = env::var("RAMSHIELD_DETECTION__BATCH_MAX_EVENTS") {
            if let Ok(parsed) = v.parse::<usize>() { self.detection.batch_max_events = parsed; }
        }
        if let Ok(v) = env::var("RAMSHIELD_DASHBOARD__HTTP_ADDR") {
            self.dashboard.http_addr = v;
        }
        if let Ok(v) = env::var("RAMSHIELD_IPC__TCP_ADDR") {
            self.ipc.tcp_addr = v;
        }
        if let Ok(v) = env::var("RAMSHIELD_IPC__MAX_CONNECTIONS") {
            if let Ok(parsed) = v.parse::<usize>() { self.ipc.max_connections = parsed; }
        }
        if let Ok(v) = env::var("RAMSHIELD_IPC__AUTH_TOKEN") {
            self.ipc.auth_token = Some(v);
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.engine.ram_limit_mb < MB_MIN {
            anyhow::bail!(RamShieldError::ConfigError(
                format!("engine.ram_limit_mb must be at least {} MB", MB_MIN)));
        }
        if self.engine.shard_count == 0 || !self.engine.shard_count.is_power_of_two() {
            anyhow::bail!(RamShieldError::ConfigError("engine.shard_count must be a power of 2".into()));
        }
        if self.engine.shard_count < SHARDS_MIN {
            anyhow::bail!(RamShieldError::ConfigError(
                format!("engine.shard_count must be at least {}", SHARDS_MIN)));
        }
        if self.detection.rps_threshold == 0 {
            anyhow::bail!(RamShieldError::ConfigError("detection.rps_threshold must be > 0".into()));
        }
        if self.detection.batch_max_events == 0 {
            anyhow::bail!(RamShieldError::ConfigError("detection.batch_max_events must be > 0".into()));
        }
        if self.detection.batch_max_events > 65536 {
            anyhow::bail!(RamShieldError::ConfigError("detection.batch_max_events must be <= 65536".into()));
        }
        if self.detection.block_duration_ms == 0 {
            anyhow::bail!(RamShieldError::ConfigError("detection.block_duration_ms must be > 0".into()));
        }
        if self.ipc.max_connections == 0 {
            anyhow::bail!(RamShieldError::ConfigError("ipc.max_connections must be > 0".into()));
        }
        if self.alerting.rps_alert_threshold == 0 {
            anyhow::bail!(RamShieldError::ConfigError("alerting.rps_alert_threshold must be > 0".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    pub worker_threads: usize,
    pub ram_limit_mb:   usize,
    pub shard_count:    usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self { worker_threads: 4, ram_limit_mb: 512, shard_count: 256 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionConfig {
    pub rps_threshold: u64,
    pub batch_max_events: usize,
    /// How long to block an IP (ms) when threshold exceeded
    #[serde(default = "default_block_duration")]
    pub block_duration_ms: u64,
    /// EWMA smoothing factor (0-1)
    #[serde(default = "default_ewma_alpha")]
    pub ewma_alpha: f64,
    /// Threat score threshold for blocking (0-1)
    #[serde(default = "default_threat_threshold")]
    pub threat_threshold: f64,
}

fn default_block_duration() -> u64 { 60_000 }
fn default_ewma_alpha() -> f64 { 0.3 }
fn default_threat_threshold() -> f64 { 0.7 }

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            rps_threshold: 1_000,
            batch_max_events: 4096,
            block_duration_ms: 60_000,
            ewma_alpha: 0.3,
            threat_threshold: 0.7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub wal_enabled: bool,
    pub wal_path: String,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self { wal_enabled: false, wal_path: "./wal".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcConfig {
    pub enabled: bool,
    pub tcp_addr: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    /// Optional auth token — if set, clients must send {"auth_token":"..."} in requests
    #[serde(default)]
    pub auth_token: Option<String>,
    /// Connection idle timeout in seconds (0 = no timeout)
    #[serde(default = "default_ipc_timeout")]
    pub idle_timeout_secs: u64,
}

fn default_max_connections() -> usize { 256 }
fn default_ipc_timeout() -> u64 { 30 }

impl Default for IpcConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            tcp_addr: "127.0.0.1:7890".into(),
            max_connections: 256,
            auth_token: None,
            idle_timeout_secs: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastingConfig {
    pub enabled: bool,
    /// Forecast tick interval in seconds
    #[serde(default = "default_forecast_interval")]
    pub interval_secs: u64,
    /// Holt-Winters alpha (level smoothing)
    #[serde(default = "default_hw_alpha")]
    pub hw_alpha: f64,
    /// Holt-Winters beta (trend smoothing)
    #[serde(default = "default_hw_beta")]
    pub hw_beta: f64,
    /// Entropy threshold for anomaly flag (0-1)
    #[serde(default = "default_entropy_threshold")]
    pub entropy_threshold: f64,
}

fn default_forecast_interval() -> u64 { 5 }
fn default_hw_alpha() -> f64 { 0.3 }
fn default_hw_beta() -> f64 { 0.1 }
fn default_entropy_threshold() -> f64 { 0.7 }

impl Default for ForecastingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: 5,
            hw_alpha: 0.3,
            hw_beta: 0.1,
            entropy_threshold: 0.7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    pub enabled: bool,
    pub http_addr: String,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self { enabled: true, http_addr: "127.0.0.1:9999".into() }
    }
}

/// Enterprise alerting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertingConfig {
    pub enabled: bool,
    /// RPS threshold for triggering alerts
    pub rps_alert_threshold: u64,
    /// Entropy threshold for triggering alerts
    #[serde(default = "default_entropy_alert")]
    pub entropy_alert_threshold: f64,
    /// Minimum time between alerts for same IP (seconds)
    #[serde(default = "default_alert_cooldown")]
    pub alert_cooldown_secs: u64,
    /// Enable audit log
    #[serde(default = "default_audit_log")]
    pub audit_log_enabled: bool,
    /// Audit log path
    #[serde(default = "default_audit_log_path")]
    pub audit_log_path: String,
}

fn default_entropy_alert() -> f64 { 0.8 }
fn default_alert_cooldown() -> u64 { 60 }
fn default_audit_log() -> bool { true }
fn default_audit_log_path() -> String { "./audit.log".into() }

impl Default for AlertingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rps_alert_threshold: 5000,
            entropy_alert_threshold: 0.8,
            alert_cooldown_secs: 60,
            audit_log_enabled: true,
            audit_log_path: "./audit.log".into(),
        }
    }
}