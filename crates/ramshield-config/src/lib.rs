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
    pub xdp: XdpConfig,
    #[serde(default)]
    pub ipc: IpcConfig,
    #[serde(default)]
    pub forecasting: ForecastingConfig,
    #[serde(default)]
    pub wal: WalConfig,
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
    /// Unique IPs per /24 in one window required for a subnet batch block.
    /// Keyed on unique IPs (not raw events): one abuser at 500 events is a
    /// single offender; 50 IPs × 12 events is a swarm. Old default of 5 events
    /// blocked whole /24s on a single 10-event burst — CGNAT killer.
    pub subnet_batch_threshold: usize,
    /// /24 event volume in the same window, secondary gate: block requires
    /// BOTH unique_ips >= subnet_batch_threshold AND events >= this.
    #[serde(default = "default_subnet_batch_min_events")]
    pub subnet_batch_min_events: u64,
    pub batch_block_enabled: bool,
    pub block_ttl_secs: u64,
    /// TTL for subnet_burst blocks specifically. Shared egress /24s hold up to
    /// 253 hosts; inheriting the 1h per-IP TTL locked out whole CGNAT ranges
    /// for an hour. Short default — continued abuse re-fires from fresh events.
    #[serde(default = "default_subnet_burst_ttl_secs")]
    pub subnet_burst_ttl_secs: u64,
    pub bloom_bits: usize,
    /// Max events accumulated before a forced flush (high-traffic batching).
    #[serde(default = "default_batch_max_events")]
    pub batch_max_events: usize,
    /// Max wait (ms) before flushing a partial batch.
    #[serde(default = "default_batch_window_ms")]
    pub batch_window_ms: u64,
    /// Max wait (ms) before flushing the pre-aggregation buffer.
    #[serde(default = "default_pre_aggs_flush_interval_ms")]
    pub pre_aggs_flush_interval_ms: u64,
    /// Per-IP hits required in one window before full IpRecord tracking.
    #[serde(default = "default_promote_min")]
    pub promote_min_events: u32,
    /// /24 event count in one window that lowers promotion threshold for that subnet.
    #[serde(default = "default_subnet_window_threshold")]
    pub subnet_window_threshold: u64,
    /// Max unique IPs in the pre-aggregation buffer before flushing to main store.
    #[serde(default = "default_pre_aggs_max_size")]
    pub pre_aggs_max_size: usize,
}

fn default_batch_max_events() -> usize {
    4096
}
fn default_batch_window_ms() -> u64 {
    50
}
fn default_promote_min() -> u32 {
    8
}
fn default_subnet_batch_min_events() -> u64 {
    100
}
fn default_subnet_burst_ttl_secs() -> u64 {
    120
}
fn default_subnet_window_threshold() -> u64 {
    500
}
fn default_pre_aggs_max_size() -> usize {
    1_000_000
}
fn default_pre_aggs_flush_interval_ms() -> u64 {
    1000
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            rps_threshold: 1_000,
            rate_window_secs: 10,
            subnet_batch_threshold: 50,
            subnet_batch_min_events: default_subnet_batch_min_events(),
            batch_block_enabled: true,
            block_ttl_secs: 3_600,
            subnet_burst_ttl_secs: default_subnet_burst_ttl_secs(),
            bloom_bits: 8_000_000,
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
pub struct XdpConfig {
    /// Attach the XDP kernel program. When false, enforcement is in-band only.
    #[serde(default)]
    pub enabled: bool,
    /// Interface to attach to (e.g. "eth0", "lo").
    #[serde(default = "default_xdp_iface")]
    pub interface: String,
    /// "skb" (generic, works everywhere) or "drv" (native, production NICs).
    #[serde(default = "default_xdp_mode")]
    pub mode: String,
}
impl Default for XdpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interface: default_xdp_iface(),
            mode: default_xdp_mode(),
        }
    }
}
fn default_xdp_iface() -> String {
    "eth0".into()
}
fn default_xdp_mode() -> String {
    "skb".into()
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

fn default_max_connection_bytes() -> Option<usize> {
    Some(1_048_576)
}
fn default_read_timeout_ms() -> Option<u64> {
    Some(5000)
}
fn default_write_timeout_ms() -> Option<u64> {
    Some(5000)
}
fn default_connection_idle_timeout_ms() -> Option<u64> {
    Some(30_000)
}
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

/// WAL durability settings. Disabled by default — enable for crash-durable
/// block state (survives restarts, replays into store + XDP reconcile).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalConfig {
    pub enabled: bool,
    pub dir: String,
    pub durability: ramshield_types::Durability,
    pub compress: bool,
    /// Segment rotation threshold in bytes.
    pub seg_max_bytes: u64,
    /// Max total WAL size on disk. Oldest segments deleted first. 0 = unlimited.
    pub retention_max_bytes: u64,
}
impl Default for WalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dir: "/var/lib/ramshield/wal".into(),
            durability: ramshield_types::Durability::Flush,
            compress: true,
            seg_max_bytes: 64 * 1024 * 1024,
            retention_max_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    pub enabled: bool,
    pub http_addr: String,
    /// Block-history ring size served by `/api/history/blocks`.
    /// 40 was too small to be useful during floods — entries scrolled out
    /// in seconds. Default raised to 1000.
    #[serde(default = "default_block_log_size")]
    pub block_log_size: usize,
    /// Argon2 PHC hash of the admin password. When set, every dashboard
    /// route except /healthz and /login requires a valid session cookie.
    /// Generate: `echo -n 'pw' | argon2 "$(head -c16 /dev/urandom | xxd -p)" -id -e`
    #[serde(default)]
    pub admin_password_hash: Option<String>,
    /// Session lifetime in seconds (default 8h).
    #[serde(default = "default_session_ttl_secs")]
    pub session_ttl_secs: u64,
}
fn default_session_ttl_secs() -> u64 {
    28_800
}
fn default_block_log_size() -> usize {
    1_000
}
impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            http_addr: "127.0.0.1:9999".into(),
            block_log_size: default_block_log_size(),
            admin_password_hash: None,
            session_ttl_secs: default_session_ttl_secs(),
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
        cfg.apply_env_overrides();
        Ok(cfg)
    }

    /// Apply RAMSHIELD_*__FIELD environment overrides on top of any config.
    pub fn apply_env_overrides(&mut self) {
        // Engine overrides
        if let Ok(v) = std::env::var("RAMSHIELD_ENGINE__RAM_LIMIT_MB")
            && let Ok(parsed) = v.parse::<usize>()
        {
            self.engine.ram_limit_mb = parsed;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_ENGINE__WORKER_THREADS")
            && let Ok(parsed) = v.parse::<usize>()
        {
            self.engine.worker_threads = parsed;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_ENGINE__SHARD_COUNT")
            && let Ok(parsed) = v.parse::<usize>()
        {
            self.engine.shard_count = parsed.next_power_of_two();
        }

        // Detection overrides
        if let Ok(v) = std::env::var("RAMSHIELD_DETECTION__RPS_THRESHOLD")
            && let Ok(parsed) = v.parse::<u64>()
        {
            self.detection.rps_threshold = parsed;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DETECTION__PROMOTE_MIN_EVENTS")
            && let Ok(parsed) = v.parse::<u32>()
        {
            self.detection.promote_min_events = parsed;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DETECTION__BATCH_WINDOW_MS")
            && let Ok(parsed) = v.parse::<u64>()
        {
            self.detection.batch_window_ms = parsed;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DETECTION__SUBNET_WINDOW_THRESHOLD")
            && let Ok(parsed) = v.parse::<u64>()
        {
            self.detection.subnet_window_threshold = parsed;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DETECTION__BLOCK_TTL_SECS")
            && let Ok(parsed) = v.parse::<u64>()
        {
            self.detection.block_ttl_secs = parsed;
        }

        if let Ok(v) = std::env::var("RAMSHIELD_DETECTION__SUBNET_BURST_TTL_SECS")
            && let Ok(parsed) = v.parse::<u64>()
        {
            self.detection.subnet_burst_ttl_secs = parsed;
        }

        // IPC overrides
        if let Ok(v) = std::env::var("RAMSHIELD_IPC__TCP_ADDR") {
            self.ipc.tcp_addr = v;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_IPC__MAX_CONNECTIONS")
            && let Ok(parsed) = v.parse::<usize>()
        {
            self.ipc.max_connections = parsed;
        }

        // Dashboard overrides
        if let Ok(v) = std::env::var("RAMSHIELD_DASHBOARD__ENABLED")
            && let Ok(parsed) = v.parse::<bool>()
        {
            self.dashboard.enabled = parsed;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DASHBOARD__HTTP_ADDR") {
            self.dashboard.http_addr = v;
        }
        if let Ok(v) = std::env::var("RAMSHIELD_DASHBOARD__ADMIN_PASSWORD") {
            // Plaintext env convenience: hash at load, never store plaintext.
            use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
            let salt = SaltString::generate(&mut OsRng);
            if let Ok(hash) = argon2::Argon2::default().hash_password(v.as_bytes(), &salt) {
                self.dashboard.admin_password_hash = Some(hash.to_string());
            }
        }

        // Forecasting overrides
        if let Ok(v) = std::env::var("RAMSHIELD_FORECASTING__ENABLED")
            && let Ok(parsed) = v.parse::<bool>()
        {
            self.forecasting.enabled = parsed;
        }

        // ponytail: log-and-continue — env overrides are operator input;
        // invalid combos surface at validate() call sites that return Result.
        let _ = self.validate();
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
            anyhow::bail!(
                "detection.bloom_bits should be at least 100,000 for low false positive rate"
            );
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
        if self.detection.pre_aggs_max_size == 0 {
            anyhow::bail!("detection.pre_aggs_max_size must be > 0");
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
#[allow(unsafe_code)]
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
            "RAMSHIELD_IPC__TCP_ADDR",
            "RAMSHIELD_IPC__MAX_CONNECTIONS",
            "RAMSHIELD_DASHBOARD__ENABLED",
            "RAMSHIELD_DASHBOARD__HTTP_ADDR",
            "RAMSHIELD_FORECASTING__ENABLED",
        ];
        for k in &keys {
            unsafe {
                std::env::remove_var(k);
            }
        }
    }

    #[test]
    fn default_config_validates() {
        let cfg = Config::default();
        cfg.validate().unwrap();
    }

    #[test]
    fn subnet_burst_ttl_default_is_short_and_serde_defaults_apply() {
        // Regression: subnet batch blocks used to inherit block_ttl_secs (1h),
        // locking out whole /24s of shared egress for an hour.
        let cfg = Config::default();
        assert_eq!(cfg.detection.subnet_burst_ttl_secs, 120);
        assert!(cfg.detection.subnet_burst_ttl_secs < cfg.detection.block_ttl_secs);
        // Old TOML without the field must still parse (serde default) —
        // parse just the [detection] table; other tables have their own requireds.
        let parsed: DetectionConfig = toml::from_str(
            "rps_threshold = 100\nrate_window_secs = 10\nsubnet_batch_threshold = 50\nsubnet_batch_min_events = 100\nbatch_block_enabled = true\nblock_ttl_secs = 3600\nbloom_bits = 1000",
        )
        .unwrap();
        assert_eq!(parsed.subnet_burst_ttl_secs, 120);
    }

    #[test]
    #[serial]
    fn env_var_override_ram_limit() {
        clear_env_vars();
        unsafe {
            std::env::set_var("RAMSHIELD_ENGINE__RAM_LIMIT_MB", "1024");
        }
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
        unsafe {
            std::env::set_var("RAMSHIELD_DETECTION__RPS_THRESHOLD", "500");
        }
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
        unsafe {
            std::env::set_var("RAMSHIELD_ENGINE__RAM_LIMIT_MB", "not_a_number");
        }
        let tmpfile = "/tmp/ramshield_test_config.toml";
        std::fs::write(tmpfile, "").unwrap();

        // Should not panic; invalid env var is silently ignored
        let result = panic::catch_unwind(|| Config::load(tmpfile).unwrap());
        assert!(
            result.is_ok(),
            "Config::load should not panic on invalid env var"
        );
        assert_eq!(result.unwrap().engine.ram_limit_mb, 512); // default preserved
        clear_env_vars();
    }
}
