# RamShield Config Module Readme

## Overview
The `ramshield-config` crate contains the centralized configuration system for the entire RamShield project. It defines the `Config` struct with all subsystem configurations and provides utilities for loading, validating, and applying environment overrides.

## Key Features

### 1. Centralized Configuration
The `Config` struct holds all subsystem configurations in one place:
- Engine: worker threads, RAM limits, shard count
- Detection: thresholds, windows, bloom filter size, L7/HTTP2 detection
- XDP: interface, mode, kernel program toggles
- IPC: TCP address, connection limits, timeouts, HMAC auth keys
- Forecasting: EWMA and Holt-Winters parameters, anomaly z-scores
- WAL: write-ahead log directory, durability mode
- Dashboard: HTTP address, session keys, auth hashes

### 2. Fail-Closed by Default
Every section has a `Default` implementation with a conservative fallback, and every section is `#[serde(default)]` on the `Config` struct, so an empty `config.toml` produces a working system. Sections that need a value to be safe - `[mesh]`, `[xdp]`, `[dashboard]`, `[ipc]`, `[wal]` - still validate on startup: a public bind without credentials is rejected rather than started.

### 3. Environment Override System
Environment variables use TOML key notation with double underscores for nested fields, and the name is the path of the field in `Config`:
```bash
RAMSHIELD_ENGINE__RAM_LIMIT_MB=256
RAMSHIELD_DETECTION__RPS_THRESHOLD=50000
RAMSHIELD_DETECTION__L7_ENABLED=true
RAMSHIELD_IPC__TCP_ADDR=127.0.0.1:7890
```
The variable names are not derived from the section names: an L7 setting lives under `detection`, so it is `RAMSHIELD_DETECTION__L7_ENABLED`, not `RAMSHIELD_L7__...`. `src/env.rs` names each variable explicitly, and an unknown or unparsable value is a startup error - an override is never silently ignored.

### 4. Validation Pipeline
`Config::from_toml_file` parses the file and then validates it; `Config::load` does the same and then applies the environment overrides and validates the merged result a second time, so an override cannot smuggle in a configuration the file would have been rejected for.

The checks in `src/validate.rs`:
- **Engine**: `ram_limit_mb >= 64`, `shard_count` a non-zero power of two, `worker_threads <= 256`
- **Detection**: `rps_threshold` and `promote_min_events` non-zero, `relative_factor` finite and `>= 1.0`, `relative_min_samples` in `1..=255`, `relative_min_breaches` non-zero, `bloom_bits >= 100_000`, `batch_max_events` in `1..=65536`, `batch_window_ms` in `1..=500`, `subnet_window_threshold >= 10`, `l7_rps_threshold` and `l7_http2_min_streams` non-zero, `l7_http2_reset_ratio_pct <= 100`, `l7_block_ttl_secs` non-zero, at most 64 `l7_rules`, no `route_hash` of 0, no `max_rps` of 0
- **Mesh** (when enabled): `node_id` non-zero, at most 256 peers, no empty peer address, non-empty `auth_key` of at least 16 characters, `peer_node_ids` and `peer_auth_keys` the same length as `peers`, no peer id of 0 or equal to the local `node_id`, no duplicate peer ids, no duplicate peer keys
- **Protection features**: at most 256 `trusted_overlay_cidrs`; `autonomous` requires `xdp.enabled` and `window_ms` in `1..=1000`; `native_ingest` requires an interface and a non-zero `max_events_per_sec`; `synproxy` requires 1 to 32 non-zero ports, `mss >= 536`, `wscale <= 14`, a non-zero `max_connections_per_source` and a `max_connections_total` at least that large; `upstream` requires a non-zero `link_capacity_mbps`, `saturation_pct` in `50..=99`, `poll_ms >= 250`, and no `/0` prefix when `bgp_mode` is not `none`
- **IPC, forecasting, dashboard**: `max_connections` in `1..=1024`, `forecasting.min_entropy` finite and `>= 0.0`, `dashboard.http_addr` non-empty, `max_password_length >= 1024`, `max_login_attempts` non-zero
- **Network exposure**: a public `dashboard.http_addr` requires an `admin_password_hash` and `tls_enabled`; a public `ipc.tcp_addr` requires `auth_keys` and `behind_tls_proxy`; `require_auth` with no `auth_keys` is rejected
- **Auth key shape**: every `ipc.auth_keys` entry is `key_id:hex_key`, the `key_id` is non-empty and unique, and the hex is even-length, hexadecimal only, and at least 32 characters

`Config::exposure_warnings` reports the exposures that are allowed but worth knowing about.

### 5. Hot-Reloading Support
`ConfigHandle` is an `Arc<ArcSwap<Config>>`, so a swap is atomic and readers never block:
```rust
let handle: ConfigHandle = Config::load("config.toml")?.into_handle();
let cfg = handle.load(); // current config, an Arc<Config>
handle.store(new_config); // atomic swap
```
`Config::load` takes the path of a file, not a directory.

## Module Structure

### Sections
- `src/sections/security.rs`: security-related configurations (Autonomous, Mesh, Native Ingest, Synproxy, Upstream)
- `src/sections/detection.rs`: detection configuration, including the L7/HTTP2 settings and `L7Rule`
- `src/sections/engine.rs`: engine configuration
- `src/sections/forecasting.rs`: forecasting configuration
- `src/sections/ipc.rs`: IPC configuration
- `src/sections/wal.rs`: write-ahead log configuration
- `src/sections/dashboard.rs`: dashboard configuration
- `src/sections/xdp.rs`: XDP configuration

### Core Files
- `src/lib.rs`: the `Config` struct, `from_toml_file`, `load`, `into_handle`
- `src/env.rs`: environment variable override application
- `src/validate.rs`: validation logic
- `src/net.rs`: network exposure and binding validation utilities

## Configuration Template
`config.baseline.toml` in the repository root is the canonical template and covers every field. The block below is the same shape, trimmed to the fields an operator usually touches. `crates/ramshield-config/tests/readme_template.rs` parses it and fails if it stops loading.

```toml
[engine]
worker_threads = 0
ram_limit_mb = 1024
shard_count = 256

[detection]
rps_threshold = 500
rate_window_secs = 10
promote_min_events = 4
batch_window_ms = 25
batch_max_events = 4096
pre_aggs_flush_interval_ms = 250
pre_aggs_max_size = 1_000_000
bloom_bits = 8_000_000
subnet_batch_threshold = 64
subnet_batch_min_events = 500
subnet_window_threshold = 12
batch_block_enabled = true
block_ttl_secs = 300
subnet_burst_ttl_secs = 60
pulse_window_secs = 5
pulse_threshold_samples = 3
l7_enabled = false
l7_rps_threshold = 500
l7_http2_min_streams = 32
l7_http2_reset_ratio_pct = 80
l7_block_ttl_secs = 300

# A route rule is an array of tables, and route_hash is required.
# [[detection.l7_rules]]
# route_hash = 123456789
# method = "Post"
# max_rps = 50
# max_latency_us = 250000
# cost_weight = 8.0
# baseline_latency_us = 100000
# max_effective_rps = 100

[ipc]
tcp_addr = "127.0.0.1:7890"
max_connections = 1024
max_line_length = 262_144
max_connection_bytes = 1_048_576
read_timeout_ms = 5000
write_timeout_ms = 5000
connection_idle_timeout_ms = 30_000
auth_keys = ["k1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"]
# Generate a 128-bit hex key (32 hex chars):
#   openssl rand -hex 16
#   or: python3 -c 'import secrets; print(secrets.token_hex(16))'
# Each entry is "key_id:hex_key"; hex must be even-length, at least 32 chars.
key_roles = [{ key_id = "k1", role = "Admin" }]
require_auth = true

[dashboard]
enabled = true
http_addr = "127.0.0.1:9999"
block_log_size = 1_000
session_ttl_secs = 28_800
max_login_attempts = 50
max_password_length = 1024
trusted_proxies = []
tls_enabled = false

[wal]
enabled = true
dir = "/var/lib/ramshield/wal"
durability = "GroupCommit"
compress = true
seg_max_bytes = 67_108_864
retention_max_bytes = 1_073_741_824

[forecasting]
enabled = true
ewma_alpha = 0.3
hw_beta = 0.1
hw_gamma = 0.1
seasonality_period = 60
anomaly_zscore = 3.0
min_entropy = 4.5

[xdp]
enabled = false
interface = "eth0"
mode = "drv"
require_native_for_autonomous = true
require_rss_rebalance = true
trusted_overlay_cidrs = []
allow_inband_fallback = true

[mesh]
enabled = false
node_id = 1
listen_addr = "127.0.0.1:7900"
peers = []
peer_node_ids = []
peer_auth_keys = []
auth_key = ""

[upstream]
enabled = false
interface = "eth0"
link_capacity_mbps = 0
saturation_pct = 90
poll_ms = 1000
webhook_url = ""
cooldown_secs = 60
bgp_mode = "none"
bgp_fifo = ""
protected_prefix = ""
bgp_community = ""

[autonomous]
enabled = false
syn_pps_per_cpu = 25000
udp_pps_per_cpu = 50000
packet_pps_per_cpu = 200000
window_ms = 100

[native_ingest]
enabled = false
interface = "eth0"
max_events_per_sec = 20000

[synproxy]
enabled = false
ports = [22, 53, 80, 443]
mss = 1460
wscale = 7
max_connections_per_source = 128
max_connections_total = 16384
new_connections_per_second = 50
new_connection_burst = 100
new_connections_total_per_second = 2000
new_connection_total_burst = 4000
```

Two things about the template that catch people out:
- The `wal.durability` values are `None`, `Flush`, `Fsync` and `GroupCommit`. There is no `hard`.
- The `forecasting` section has no `serde(default)` on its fields, so all seven of them must be present once the section appears. The same is true of the other sections: a field without a default is required.

## API Usage

```rust
use ramshield_config::{Config, ConfigHandle};

fn main() -> anyhow::Result<()> {
    // Load configuration from file, apply the environment overrides and
    // validate the merged result.
    let config = Config::load("config.toml")?;

    // Get handle for hot-reloading
    let handle: ConfigHandle = config.into_handle();

    // Access current configuration
    let cfg = handle.load();
    println!("Engine RAM limit: {} MB", cfg.engine.ram_limit_mb);

    Ok(())
}
```

## Module-specific Features

### Security Module (`src/sections/security.rs`)
- Contains the Mesh, Upstream, Autonomous, Native Ingest and Synproxy configurations
- No duplicate struct definitions

### Detection Module (`src/sections/detection.rs`)
- The L7/HTTP2 settings live on `DetectionConfig` as `l7_enabled`, `l7_rps_threshold`, `l7_http2_min_streams`, `l7_http2_reset_ratio_pct` and `l7_block_ttl_secs`
- `L7Rule` carries `route_hash` (required), an optional `method`, optional `max_rps`, `max_latency_us` and `max_effective_rps`, plus the required `cost_weight` and `baseline_latency_us`

### Environment Overrides (`src/env.rs`)
- Typed parsing of environment variables
- Fail-fast validation of invalid environment values
- Secrets never echoed in logs or error messages

### Validation (`src/validate.rs`)
- Validation of all configuration fields
- Fail-closed approach: invalid configs rejected before system startup
- Checks for network exposure risks (public dashboard binds, non-loopback IPC)

## Dependencies
- `serde`: TOML deserialization
- `toml`: TOML parsing
- `arc-swap`: the `ConfigHandle` atomic swap
- `anyhow`: error handling and validation
- `ramshield-types`: shared types - `Durability`, `HttpMethod` and the error types

## Testing
- 4 property-based fuzz tests in `tests/fuzz.rs`
- Random mutation of config fields to test validation coverage
- Integration tests verify `config.toml` and `config.stress.toml` loading

## Uniqueness

### 1. Fail-Closed Philosophy
Every section has a safe default, and a configuration that would expose an unauthenticated service is rejected rather than started.

### 2. TOML-Style Environment Variables
Consistent naming convention: `RAMSHIELD_SECTION__FIELD=value` for nested structures, where the section is the path of the field in `Config`.

### 3. Modular Architecture
Clear separation of concerns with each subsystem in its own module.

## Integration with Other Crates

- `ramshield-detection`: uses `config.detection`
- `ramshield-enforcement`: uses `config.detection` and the block TTLs
- `ramshield-xdp`: accesses `config.xdp` and `config.autonomous`
- `ramshield-mesh`: reads `config.mesh`
- `ramshield-storage`: reads `config.wal`
- `ramshield-forecasting`: reads `config.forecasting`

## Benchmarks
No hot-path benchmarks required - configuration is loaded once at startup and cached. Hot-reload via `ConfigHandle` is an O(1) pointer swap.
