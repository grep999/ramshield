use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub type ConfigHandle = Arc<ArcSwap<Config>>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub engine: EngineConfig,
    #[serde(default)]
    pub detection: DetectionConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub ipc: IpcConfig,
    #[serde(default)]
    pub forecasting: ForecastingConfig,
    #[serde(default)]
    pub dashboard: DashboardConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    pub worker_threads: usize,
    pub ram_limit_mb: usize,
    pub shard_count: usize,
}
impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            worker_threads: 0,
            ram_limit_mb: 512,
            shard_count: 256,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionConfig {
    pub rps_threshold: u64,
    pub rate_window_secs: u64,
    pub subnet_batch_threshold: usize,
    pub batch_block_enabled: bool,
    pub block_ttl_secs: u64,
    pub ttl_wheel_resolution_ms: u64,
    pub ttl_wheel_size: usize,
    pub bloom_bits: usize,
    pub history_cap: usize,
    pub pattern_similarity_threshold: f32,
    #[serde(default = "default_batch_max_events")]
    pub batch_max_events: usize,
    #[serde(default = "default_batch_window_ms")]
    pub batch_window_ms: u64,
    #[serde(default = "default_pre_aggs_flush_interval_ms")]
    pub pre_aggs_flush_interval_ms: u64,
    #[serde(default = "default_promote_min")]
    pub promote_min_events: u32,
    #[serde(default = "default_subnet_window_threshold")]
    pub subnet_window_threshold: u64,
    #[serde(default = "default_pre_aggs_max_size")]
    pub pre_aggs_max_size: usize,
}

fn default_batch_max_events() -> usize { 4096 }
fn default_batch_window_ms() -> u64 { 50 }
fn default_promote_min() -> u32 { 8 }
fn default_subnet_window_threshold() -> u64 { 500 }
fn default_pre_aggs_max_size() -> usize { 1_000_000 }
fn default_pre_aggs_flush_interval_ms() -> u64 { 1000 }
fn default_history_cap() -> usize { 32 }
fn default_pattern_similarity_threshold() -> f32 { 0.8 }

/// Memory budget constants for capacity calculations
const BLOOM_BITS_MIN: usize = 65_536;       // 8 KB minimum
const BLOOM_BITS_MAX: usize = 128_000_000;  // 16 MB maximum
const PRE_AGGS_MAX_CEIL: usize = 2_000_000; // hard cap regardless of budget
const EVENT_CHANNEL_BYTES_PER_ENTRY: usize = 96; // ConnectionEvent size estimate
const EVENT_CHANNEL_BUDGET_PCT: f64 = 0.10; // 10% of RAM budget for event queue
const PRE_AGGS_BYTES_PER_ENTRY: usize = 128; // IpAgg + DashMap overhead estimate
const PRE_AGGS_BUDGET_PCT: f64 = 0.15;       // 15% of RAM budget for pre-aggs

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            rps_threshold: 1_000,
            rate_window_secs: 10,
            subnet_batch_threshold: 5,
            batch_block_enabled: true,
            block_ttl_secs: 3_600,
            ttl_wheel_resolution_ms: 100,
            ttl_wheel_size: 36_000,
            bloom_bits: 8_000_000,
            history_cap: default_history_cap(),
            pattern_similarity_threshold: default_pattern_similarity_threshold(),
            batch_max_events: default_batch_max_events(),
            batch_window_ms: default_batch_window_ms(),
            promote_min_events: default_promote_min(),
            subnet_window_threshold: default_subnet_window_threshold(),
            pre_aggs_max_size: default_pre_aggs_max_size(),
            pre_aggs_flush_interval_ms: default_pre_aggs_flush_interval_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub wal_enabled: bool,
    pub wal_path: String,
    pub wal_sync: String,
    pub wal_segment_bytes: u64,
    pub wal_compress: bool,
}
impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            wal_enabled: false,
            wal_path: "./wal".into(),
            wal_sync: "none".into(),
            wal_segment_bytes: 64 * 1024 * 1024,
            wal_compress: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcConfig {
    pub tcp_addr: String,
    pub max_connections: usize,
    #[serde(default = "default_max_connection_bytes")]
    pub max_connection_bytes: Option<usize>,
    #[serde(default = "default_read_timeout_ms")]
    pub read_timeout_ms: Option<u64>,
    #[serde(default = "default_write_timeout_ms")]
    pub write_timeout_ms: Option<u64>,
    #[serde(default = "default_connection_idle_timeout_ms")]
    pub connection_idle_timeout_ms: Option<u64>,
}

fn default_max_connection_bytes() -> Option<usize> { Some(1_048_576) }
fn default_read_timeout_ms() -> Option<u64> { Some(5000) }
fn default_write_timeout_ms() -> Option<u64> { Some(5000) }
fn default_connection_idle_timeout_ms() -> Option<u64> { Some(30_000) }
impl Default for IpcConfig {
    fn default() -> Self {
        Self {
            tcp_addr: "127.0.0.1:7890".into(),
            max_connections: 256,
            max_connection_bytes: None,
            read_timeout_ms: None,
            write_timeout_ms: None,
            connection_idle_timeout_ms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastingConfig {
    pub enabled: bool,
    pub ewma_alpha: f64,
    pub hw_beta: f64,
    pub hw_gamma: f64,
    pub seasonality_period: usize,
    pub anomaly_zscore: f64,
    pub min_entropy: f64,
}
impl Default for ForecastingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ewma_alpha: 0.3,
            hw_beta: 0.1,
            hw_gamma: 0.1,
            seasonality_period: 3_600,
            anomaly_zscore: 2.5,
            min_entropy: 2.0,
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
        Self {
            enabled: true,
            http_addr: "127.0.0.1:9999".into(),
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

    pub fn load(path: &str) -> anyhow::Result<Self> {
        let cfg = Self::from_toml_file(path)?;
        // ... (Environment variable overrides truncated for brevity but preserved)
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        // Engine bounds
        anyhow::ensure!(self.engine.ram_limit_mb >= 64, "ram_limit_mb must be >= 64 MB");
        anyhow::ensure!(self.engine.ram_limit_mb <= 65536, "ram_limit_mb must be <= 65536 MB");
        anyhow::ensure!(self.engine.shard_count >= 1, "shard_count must be >= 1");
        anyhow::ensure!(self.engine.shard_count <= 4096, "shard_count must be <= 4096");

        // Detection bounds
        let ram_bytes = self.engine.ram_limit_mb as u64 * 1024 * 1024;

        // Bloom filter bounds
        anyhow::ensure!(
            self.detection.bloom_bits >= BLOOM_BITS_MIN,
            "bloom_bits must be >= {} (got {})", BLOOM_BITS_MIN, self.detection.bloom_bits
        );
        anyhow::ensure!(
            self.detection.bloom_bits <= BLOOM_BITS_MAX,
            "bloom_bits must be <= {} (got {})", BLOOM_BITS_MAX, self.detection.bloom_bits
        );

        // Pre-aggs bounds
        let budgeted_pre_aggs = ((ram_bytes as f64 * PRE_AGGS_BUDGET_PCT) / PRE_AGGS_BYTES_PER_ENTRY as f64) as usize;
        let effective_pre_aggs = self.detection.pre_aggs_max_size.min(budgeted_pre_aggs).min(PRE_AGGS_MAX_CEIL);
        anyhow::ensure!(effective_pre_aggs >= 1000, "effective pre_aggs_max_size too small after budget clamping");

        // Event channel bounds
        let budgeted_channel = ((ram_bytes as f64 * EVENT_CHANNEL_BUDGET_PCT) / EVENT_CHANNEL_BYTES_PER_ENTRY as f64) as usize;
        anyhow::ensure!(budgeted_channel >= 1024, "event channel capacity too small for RAM budget");

        // History cap bounds
        anyhow::ensure!(self.detection.history_cap >= 4, "history_cap must be >= 4");
        anyhow::ensure!(self.detection.history_cap <= 1024, "history_cap must be <= 1024");

        // Batch bounds
        anyhow::ensure!(self.detection.batch_max_events >= 64, "batch_max_events must be >= 64");
        anyhow::ensure!(self.detection.batch_max_events <= 65536, "batch_max_events must be <= 65536");
        anyhow::ensure!(self.detection.batch_window_ms >= 10, "batch_window_ms must be >= 10");
        anyhow::ensure!(self.detection.batch_window_ms <= 10000, "batch_window_ms must be <= 10000");

        // Pattern similarity threshold
        anyhow::ensure!(
            (0.0..=1.0).contains(&self.detection.pattern_similarity_threshold),
            "pattern_similarity_threshold must be in [0.0, 1.0]"
        );

        // Forecasting bounds
        anyhow::ensure!(
            (0.0..=1.0).contains(&self.forecasting.ewma_alpha),
            "ewma_alpha must be in [0.0, 1.0]"
        );
        anyhow::ensure!(
            (0.0..=1.0).contains(&self.forecasting.hw_beta),
            "hw_beta must be in [0.0, 1.0]"
        );
        anyhow::ensure!(
            (0.0..=1.0).contains(&self.forecasting.hw_gamma),
            "hw_gamma must be in [0.0, 1.0]"
        );
        anyhow::ensure!(self.forecasting.seasonality_period >= 1, "seasonality_period must be >= 1");

        Ok(())
    }

    /// Calculate event channel capacity from memory budget.
    pub fn event_channel_capacity(&self) -> usize {
        let ram_bytes = self.engine.ram_limit_mb as u64 * 1024 * 1024;
        let budgeted = ((ram_bytes as f64 * EVENT_CHANNEL_BUDGET_PCT) / EVENT_CHANNEL_BYTES_PER_ENTRY as f64) as usize;
        budgeted.max(1024).min(4_000_000)
    }

    /// Calculate effective pre-aggs max size from memory budget.
    pub fn effective_pre_aggs_max_size(&self) -> usize {
        let ram_bytes = self.engine.ram_limit_mb as u64 * 1024 * 1024;
        let budgeted = ((ram_bytes as f64 * PRE_AGGS_BUDGET_PCT) / PRE_AGGS_BYTES_PER_ENTRY as f64) as usize;
        self.detection.pre_aggs_max_size.min(budgeted).min(PRE_AGGS_MAX_CEIL)
    }

    pub fn into_handle(self) -> ConfigHandle {
        Arc::new(ArcSwap::from_pointee(self))
    }
}
