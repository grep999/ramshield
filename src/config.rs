use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub type ConfigHandle = Arc<ArcSwap<Config>>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)] pub engine:      EngineConfig,
    #[serde(default)] pub detection:   DetectionConfig,
    #[serde(default)] pub storage:     StorageConfig,
    #[serde(default)] pub ipc:         IpcConfig,
    #[serde(default)] pub forecasting: ForecastingConfig,
    #[serde(default)] pub dashboard:   DashboardConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    pub worker_threads: usize,
    pub ram_limit_mb:   usize,
    pub shard_count:    usize,
}
impl Default for EngineConfig {
    fn default() -> Self { Self { worker_threads: 0, ram_limit_mb: 512, shard_count: 256 } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionConfig {
    pub rps_threshold:           u64,
    pub rate_window_secs:        u64,
    pub subnet_batch_threshold:  usize,
    pub batch_block_enabled:     bool,
    pub block_ttl_secs:          u64,
    pub ttl_wheel_resolution_ms: u64,
    pub ttl_wheel_size:          usize,
    pub bloom_bits:              usize,
    pub history_cap:             usize,
    pub pattern_similarity_threshold: f32,
    /// Max events accumulated before a forced flush (high-traffic batching).
    #[serde(default = "default_batch_max_events")]
    pub batch_max_events:        usize,
    /// Max wait (ms) before flushing a partial batch.
    #[serde(default = "default_batch_window_ms")]
    pub batch_window_ms:         u64,
    /// Per-IP hits required in one window before full IpRecord tracking.
    #[serde(default = "default_promote_min")]
    pub promote_min_events:      u32,
    /// /24 event count in one window that lowers promotion threshold for that subnet.
    #[serde(default = "default_subnet_window_threshold")]
    pub subnet_window_threshold: u64,
}

fn default_batch_max_events() -> usize { 4096 }
fn default_batch_window_ms() -> u64 { 50 }
fn default_promote_min() -> u32 { 8 }
fn default_subnet_window_threshold() -> u64 { 500 }
fn default_history_cap() -> usize { 32 }
fn default_pattern_similarity_threshold() -> f32 { 0.8 }

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            rps_threshold: 1_000, rate_window_secs: 10, subnet_batch_threshold: 5,
            batch_block_enabled: true, block_ttl_secs: 3_600,
            ttl_wheel_resolution_ms: 100, ttl_wheel_size: 36_000, bloom_bits: 8_000_000,
            history_cap: default_history_cap(),
            pattern_similarity_threshold: default_pattern_similarity_threshold(),
            batch_max_events: default_batch_max_events(),
            batch_window_ms: default_batch_window_ms(),
            promote_min_events: default_promote_min(),
            subnet_window_threshold: default_subnet_window_threshold(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub wal_enabled:       bool,
    pub wal_path:          String,
    pub wal_sync:          String,
    pub wal_segment_bytes: u64,
    pub wal_compress:      bool,
}
impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            wal_enabled: false, wal_path: "./wal".into(), wal_sync: "none".into(),
            wal_segment_bytes: 64 * 1024 * 1024, wal_compress: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcConfig {
    pub tcp_addr:        String,
    pub max_connections: usize,
}
impl Default for IpcConfig {
    fn default() -> Self { Self { tcp_addr: "127.0.0.1:7890".into(), max_connections: 256 } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastingConfig {
    pub enabled:            bool,
    pub ewma_alpha:         f64,
    pub hw_beta:            f64,
    pub hw_gamma:           f64,
    pub seasonality_period: usize,
    pub anomaly_zscore:     f64,
    pub min_entropy:        f64,
}
impl Default for ForecastingConfig {
    fn default() -> Self {
        Self {
            enabled: true, ewma_alpha: 0.3, hw_beta: 0.1, hw_gamma: 0.1,
            seasonality_period: 3_600, anomaly_zscore: 2.5, min_entropy: 2.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    pub enabled:   bool,
    pub http_addr: String,
}
impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            enabled:   true,
            http_addr: "127.0.0.1:7891".into(),
        }
    }
}

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

        // Engine overrides
        if let Ok(v) = std::env::var("RAMSHIELD_ENGINE__RAM_LIMIT_MB") {
            if let Ok(parsed) = v.parse::<usize>() {
                cfg.engine.ram_limit_mb = parsed;
            }
        }
        if let Ok(v) = std::env::var("RAMSHIELD_ENGINE__WORKER_THREADS") {
            if let Ok(parsed) = v.parse::<usize>() {
                cfg.engine.worker_threads = parsed;
            }
        }
        if let Ok(v) = std::env::var("RAMSHIELD_ENGINE__SHARD_COUNT") {
            if let Ok(parsed) = v.parse::<usize>() {
                cfg.engine.shard_count = parsed.next_power_of_two();
            }
        }

        // Detection overrides
        if let Ok(v) = std::env::var("RAMSHIELD_DETECTION__RPS_THRESHOLD") {
            if let Ok(parsed) = v.parse::<u64>() {
                cfg.detection.rps_threshold = parsed;
            }
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DETECTION__PROMOTE_MIN_EVENTS") {
            if let Ok(parsed) = v.parse::<u32>() {
                cfg.detection.promote_min_events = parsed;
            }
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DETECTION__BATCH_WINDOW_MS") {
            if let Ok(parsed) = v.parse::<u64>() {
                cfg.detection.batch_window_ms = parsed;
            }
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DETECTION__SUBNET_WINDOW_THRESHOLD") {
            if let Ok(parsed) = v.parse::<u64>() {
                cfg.detection.subnet_window_threshold = parsed;
            }
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DETECTION__BLOCK_TTL_SECS") {
            if let Ok(parsed) = v.parse::<u64>() {
                cfg.detection.block_ttl_secs = parsed;
            }
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DETECTION__HISTORY_CAP") {
            if let Ok(parsed) = v.parse::<usize>() {
                cfg.detection.history_cap = parsed;
            }
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DETECTION__PATTERN_SIMILARITY_THRESHOLD") {
            if let Ok(parsed) = v.parse::<f32>() {
                cfg.detection.pattern_similarity_threshold = parsed;
            }
        }

        // IPC overrides
        if let Ok(v) = std::env::var("RAMSHIELD_IPC__TCP_ADDR") {
            cfg.ipc.tcp_addr = v;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_IPC__MAX_CONNECTIONS") {
            if let Ok(parsed) = v.parse::<usize>() {
                cfg.ipc.max_connections = parsed;
            }
        }

        // Dashboard overrides
        if let Ok(v) = std::env::var("RAMSHIELD_DASHBOARD__ENABLED") {
            if let Ok(parsed) = v.parse::<bool>() {
                cfg.dashboard.enabled = parsed;
            }
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DASHBOARD__HTTP_ADDR") {
            cfg.dashboard.http_addr = v;
        }

        // Storage overrides
        if let Ok(v) = std::env::var("RAMSHIELD_STORAGE__WAL_ENABLED") {
            if let Ok(parsed) = v.parse::<bool>() {
                cfg.storage.wal_enabled = parsed;
            }
        }
        if let Ok(v) = std::env::var("RAMSHIELD_STORAGE__WAL_PATH") {
            cfg.storage.wal_path = v;
        }

        // Forecasting overrides
        if let Ok(v) = std::env::var("RAMSHIELD_FORECASTING__ENABLED") {
            if let Ok(parsed) = v.parse::<bool>() {
                cfg.forecasting.enabled = parsed;
            }
        }

        cfg.validate()?;
        Ok(cfg)
    }

    /// Validate configuration with sensible bounds and error messages.
    pub fn validate(&self) -> anyhow::Result<()> {
        // Engine config validation
        if self.engine.ram_limit_mb < 64 {
            anyhow::bail!("engine.ram_limit_mb must be at least 64 MB");
        }
        if self.engine.shard_count == 0 || !self.engine.shard_count.is_power_of_two() {
            anyhow::bail!("engine.shard_count must be a power of 2");
        }

        // Detection config validation
        if self.detection.rps_threshold == 0 {
            anyhow::bail!("detection.rps_threshold must be > 0");
        }
        if self.detection.promote_min_events == 0 {
            anyhow::bail!("detection.promote_min_events must be > 0");
        }
        if self.detection.bloom_bits < 100_000 {
            anyhow::bail!("detection.bloom_bits should be at least 100,000 for low false positive rate");
        }
        if self.detection.batch_max_events == 0 || self.detection.batch_max_events > 65536 {
            anyhow::bail!("detection.batch_max_events must be between 1 and 65536");
        }
        if self.detection.batch_window_ms == 0 || self.detection.batch_window_ms > 500 {
            anyhow::bail!("detection.batch_window_ms must be between 1 and 500 ms");
        }
        if self.detection.subnet_window_threshold < 10 {
            anyhow::bail!("detection.subnet_window_threshold should be at least 10");
        }
        if !(0.0..=1.0).contains(&self.detection.pattern_similarity_threshold) {
            anyhow::bail!("detection.pattern_similarity_threshold must be in range [0.0, 1.0]");
        }

        // IPC config validation
        if self.ipc.max_connections == 0 {
            anyhow::bail!("ipc.max_connections must be > 0");
        }
        if self.ipc.max_connections > 1_000_000 {
            anyhow::bail!("ipc.max_connections should not exceed 1,000,000");
        }

        // Forecasting config validation
        if self.forecasting.enabled {
            if !(0.0..=1.0).contains(&self.forecasting.ewma_alpha) {
                anyhow::bail!("forecasting.ewma_alpha must be in range [0.0, 1.0]");
            }
            if self.forecasting.seasonality_period == 0 {
                anyhow::bail!("forecasting.seasonality_period must be > 0");
            }
            if self.forecasting.anomaly_zscore < 1.0 {
                anyhow::bail!("forecasting.anomaly_zscore should be at least 1.0");
            }
        }

        // Dashboard config validation
        if self.dashboard.http_addr.is_empty() {
            anyhow::bail!("dashboard.http_addr must not be empty");
        }

        Ok(())
    }

    pub fn into_handle(self) -> ConfigHandle {
        Arc::new(ArcSwap::from_pointee(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[cfg(test)]
    fn clear_env_vars() {
        let keys = [
            "RAMSHIELD_ENGINE__RAM_LIMIT_MB",
            "RAMSHIELD_ENGINE__WORKER_THREADS",
            "RAMSHIELD_ENGINE__SHARD_COUNT",
            "RAMSHIELD_DETECTION__RPS_THRESHOLD",
            "RAMSHIELD_DETECTION__PROMOTE_MIN_EVENTS",
            "RAMSHIELD_DETECTION__BATCH_WINDOW_MS",
            "RAMSHIELD_DETECTION__SUBNET_WINDOW_THRESHOLD",
            "RAMSHIELD_DETECTION__BLOCK_TTL_SECS",
            "RAMSHIELD_DETECTION__HISTORY_CAP",
            "RAMSHIELD_DETECTION__PATTERN_SIMILARITY_THRESHOLD",
            "RAMSHIELD_IPC__TCP_ADDR",
            "RAMSHIELD_IPC__MAX_CONNECTIONS",
            "RAMSHIELD_DASHBOARD__ENABLED",
            "RAMSHIELD_DASHBOARD__HTTP_ADDR",
            "RAMSHIELD_STORAGE__WAL_ENABLED",
            "RAMSHIELD_STORAGE__WAL_PATH",
            "RAMSHIELD_FORECASTING__ENABLED",
        ];
        for k in &keys {
            std::env::remove_var(k);
        }
    }

    #[test]
    fn default_config_validates() {
        let cfg = Config::default();
        cfg.validate().unwrap();
    }

    #[test]
    #[serial]
    fn env_var_override_ram_limit() {
        clear_env_vars();
        std::env::set_var("RAMSHIELD_ENGINE__RAM_LIMIT_MB", "1024");
        let tmpfile = "/tmp/ramshield_test_config.toml";
        std::fs::write(tmpfile, "").unwrap();
        let cfg = Config::load(tmpfile).unwrap();
        assert_eq!(cfg.engine.ram_limit_mb, 1024);
        clear_env_vars();
    }

    #[test]
    #[serial]
    fn env_override_detection_rps() {
        clear_env_vars();
        std::env::set_var("RAMSHIELD_DETECTION__RPS_THRESHOLD", "500");
        let tmpfile = "/tmp/ramshield_test_config.toml";
        std::fs::write(tmpfile, "").unwrap();
        let cfg = Config::load(tmpfile).unwrap();
        assert_eq!(cfg.detection.rps_threshold, 500);
        clear_env_vars();
    }

    #[test]
    #[serial]
    fn env_override_invalid_ignored() {
        use std::panic;
        clear_env_vars();
        std::env::set_var("RAMSHIELD_ENGINE__RAM_LIMIT_MB", "not_a_number");
        let tmpfile = "/tmp/ramshield_test_config.toml";
        std::fs::write(tmpfile, "").unwrap();
        
        // Should not panic; invalid env var is silently ignored
        let result = panic::catch_unwind(|| {
            Config::load(tmpfile).unwrap()
        });
        assert!(result.is_ok(), "Config::load should not panic on invalid env var");
        assert_eq!(result.unwrap().engine.ram_limit_mb, 512); // default preserved
        clear_env_vars();
    }
}
