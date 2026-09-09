# ramshield-config

Configuration types, defaults, validation, and runtime reloading for every RamShield component. This crate owns the single `Config` struct that is deserialized from TOML (or constructed from defaults) and threaded through the entire system.

## Config struct hierarchy

```
Config
├── engine:     EngineConfig      — worker threads, RAM limit, shard count
├── detection:  DetectionConfig   — thresholds, windows, gate toggles
├── ipc:        IpcConfig         — bind address, auth keys, timeouts
├── forecasting: ForecastingConfig — EWMA alpha, Holt-Winters params, anomaly z-score
├── wal:        WalConfig         — segment size, retention count
├── dashboard:  DashboardConfig   — bind address, login limits, TLS toggle
└── xdp:        XdpConfig         — enabled flag, interface, mode (skb/drv)
```

Every sub-config implements `Default` so a zero-config binary starts with sane values. TOML keys map 1:1 to struct fields via serde, and environment variable overrides (`RAMSHIELD_ENGINE__RAM_LIMIT_MB=2048`) take precedence over file values.

## Environment override protocol

`Config::apply_env_overrides()` scans `RAMSHIELD_<SECTION>__<FIELD>` patterns and sets the matching field. The double-underscore separator prevents collisions with unrelated env vars. Booleans accept `true`/`false`; strings are taken verbatim. This mechanism lets Docker and Kubernetes inject secrets and tuning without mounting config files.

## Validation rules

`Config::validate()` enforces invariants that would cause runtime crashes or security holes:

- `engine.ram_limit_mb >= 128` — prevents degenerate tiny stores.
- `engine.shard_count` must be a power of 2 — required by DashMap's shard index modulo.
- `ipc.max_connections > 0`.
- `ipc.auth_keys` must be non-empty when `ipc.tcp_addr` binds a non-loopback interface — prevents open IPC servers.
- `dashboard.admin_password_hash` must be set when `dashboard.http_addr` binds a public interface — prevents open dashboards.
- Public binds without TLS trigger exposure warnings (logged at boot, not fatal).

`is_public_bind()` strips IPv6 brackets and checks whether the host part is `0.0.0.0`, `[*]`, or `[::]`. Localhost, `127.*`, and `::1` are always considered private.

## Public bind detection

Two functions power the security guard:

- `is_public_bind(addr: &str) -> bool` — parses the address, strips brackets, checks the host octets. Handles IPv4, IPv6, and bracketed star-notation.
- `exposure_warnings() -> Vec<String>` — returns human-readable warnings for any public bind that lacks the corresponding auth credential. Called at boot and logged via `tracing::warn!`.

## Config loading

`Config::load(path)` reads a TOML file, deserializes into `Config`, calls `apply_env_overrides()`, then `validate()`. The path is canonicalized before reading to prevent symlink traversal. On missing file, the default config is used (with env overrides still applied).

## Runtime hot-reload

`ConfigHandle` is `Arc<ArcSwap<Config>>`. The engine, IPC server, and dashboard each hold a clone. A file watcher (or signal handler) can swap the inner config atomically — readers see the new config on their next access without locks or restarts.

## Key types

```rust
pub struct Config {
    pub engine: EngineConfig,
    pub detection: DetectionConfig,
    pub ipc: IpcConfig,
    pub forecasting: ForecastingConfig,
    pub wal: WalConfig,
    pub dashboard: DashboardConfig,
    pub xdp: XdpConfig,
}

pub struct IpcConfig {
    pub tcp_addr: String,                    // default: "127.0.0.1:7890"
    pub max_connections: usize,              // default: 256
    pub max_connection_bytes: Option<usize>, // default: 1MB
    pub read_timeout_ms: Option<u64>,        // default: 5000
    pub write_timeout_ms: Option<u64>,       // default: 5000
    pub connection_idle_timeout_ms: Option<u64>, // default: 30000
    pub max_line_length: Option<usize>,      // default: 32MB
    pub auth_keys: Vec<String>,              // "key_id:hex_key" pairs
}

pub struct DashboardConfig {
    pub enabled: bool,                       // default: true
    pub http_addr: String,                   // default: "127.0.0.1:9999"
    pub admin_password_hash: Option<String>, // argon2 hash
    pub max_login_attempts: u32,             // default: 5
    pub max_password_length: usize,          // default: 128
    pub session_ttl_secs: u64,               // default: 3600
    pub block_log_size: usize,               // default: 10000
    pub require_tls: bool,                   // default: false
}

pub struct DetectionConfig {
    pub rps_threshold: u64,
    pub rate_window_secs: u64,
    pub subnet_batch_threshold: usize,   // default: 50 unique IPs
    pub subnet_batch_min_events: u64,    // default: 100
    pub batch_block_enabled: bool,
    pub block_ttl_secs: u64,
    pub pulse_window_secs: u64,
}
```

## Dependencies

Pure Rust — `serde`, `toml`, `anyhow`. No network, no async, no filesystem watching. This crate is the leaf of the dependency tree; every other crate depends on it.

## Tests

- Default values deserialize correctly from empty TOML.
- Env overrides set correct fields and don't clobber unrelated ones.
- Public bind detection covers IPv4, IPv6, bracketed star, localhost.
- Validation rejects degenerate configs (tiny RAM, missing auth on public bind).
- Config round-trip: serialize → deserialize → assert_eq.
