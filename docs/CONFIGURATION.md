# Configuration

The configuration is TOML. Environment variables can override config values using:

```text
RAMSHIELD_<SECTION>__<KEY>
```

The canonical repository example is [`config.baseline.toml`](../config.baseline.toml). The schema lives in [`crates/ramshield-config/src/sections/`](../crates/ramshield-config/src/sections/) (one file per `[section]`, with its serde defaults); validation lives in [`crates/ramshield-config/src/validate.rs`](../crates/ramshield-config/src/validate.rs) and environment overrides in [`crates/ramshield-config/src/env.rs`](../crates/ramshield-config/src/env.rs).

The baseline file intentionally contains explicit values for development/testing. It is not a universal production tuning profile.

## `[engine]`

| Key | Baseline | Built-in default | Notes |
|---|---:|---:|---|
| `ram_limit_mb` | `1024` | `512` | Hard store budget; validation requires at least 64 MB. |
| `shard_count` | `256` | `256` | Must be a power of two. |
| `worker_threads` | `0` | `0` | `0` lets Tokio choose worker count. |

## `[detection]`

| Key | Baseline | Built-in default |
|---|---:|---:|
| `rps_threshold` | `500` | `500` |
| `rate_window_secs` | `10` | `10` |
| `promote_min_events` | `4` | `4` |
| `relative_enabled` | `false` | `false` |
| `relative_factor` | `5.0` | `5.0` |
| `relative_floor_rps` | `3.0` | `3.0` |
| `relative_min_samples` | `8` | `8` |
| `emergency_burst_threshold` | `500` | `500` |
| `batch_window_ms` | `25` | `25` |
| `batch_max_events` | `4096` | `4096` |
| `pre_aggs_flush_interval_ms` | `250` | `250` |
| `pre_aggs_max_size` | `1000000` | `1000000` |
| `bloom_bits` | `8000000` | `8000000` |
| `subnet_batch_threshold` | `64` | `64` |
| `subnet_batch_min_events` | `500` | `500` |
| `subnet_window_threshold` | `12` | `12` |
| `batch_block_enabled` | `true` | `true` |
| `block_ttl_secs` | `300` | `300` |
| `subnet_burst_ttl_secs` | `60` | `60` |
| `pulse_window_secs` | `5` | `5` |
| `pulse_threshold_samples` | `3` | `3` |

Use the baseline values only when they are appropriate for your workload. Detection thresholds are policy, not universal safe values.

## `[ipc]`

| Key | Baseline | Built-in default |
|---|---|---:|
| `tcp_addr` | `127.0.0.1:7890` | `127.0.0.1:7890` |
| `max_connections` | `128` | `1024` |
| `max_line_length` | `262144` | `None` |
| `max_connection_bytes` | `1048576` | `None` |
| `read_timeout_ms` | `5000` | `None` |
| `write_timeout_ms` | `5000` | `None` |
| `connection_idle_timeout_ms` | `30000` | `None` |
| `auth_keys` | `[]` | `[]` |
| `require_auth` | `false` | `false` |
| `require_auth_for_mutations` | `true` | `true` |

`require_auth_for_mutations` refuses a state-mutating request (`block_ip`,
`block_cidr`, `unblock_ip`, `unblock_cidr`, `flush`) that carries no verified
principal. It is only reachable when `auth_keys` is empty — with keys
configured, every frame carries a principal and the role table applies. Set it
to `false` to restore the unauthenticated operator path on a host where that is
an accepted risk; it cannot widen the exposure past loopback, because a
non-loopback bind still requires `auth_keys`.

For a non-loopback IPC bind, configuration validation requires authentication and also requires `behind_tls_proxy = true`.

## `[dashboard]`

| Key | Baseline | Built-in default |
|---|---:|---:|
| `enabled` | `true` | `true` |
| `http_addr` | `127.0.0.1:9999` | `127.0.0.1:9999` |
| `block_log_size` | `1000` | `1000` |
| `admin_password_hash` | unset | unset |
| `require_auth_for_config` | `true` | `true` |
| `session_ttl_secs` | `28800` | `28800` |
| `max_login_attempts` | `50` | `50` |
| `max_password_length` | `1024` | `1024` |
| `trusted_proxies` | `[]` | `[]` |
| `tls_enabled` | `false` | `false` |
| `cookie_secure` | unset | unset |

A non-loopback dashboard bind requires `admin_password_hash`.

RamShield has no built-in TLS listener. `tls_enabled` controls cookie behavior for an external HTTPS termination boundary; it does not enable a TLS server.

## `[wal]`

| Key | Baseline | Built-in default |
|---|---:|---:|
| `enabled` | `true` | `false` |
| `dir` | `/var/lib/ramshield/wal` | `/var/lib/ramshield/wal` |
| `durability` | `GroupCommit` | `Flush` |
| `compress` | `true` | `true` |
| `seg_max_bytes` | `67108864` | `67108864` |
| `retention_max_bytes` | `1073741824` | `536870912` |
| `allow_volatile_fallback` | `false` | `false` |

WAL is what provides block-state persistence across restarts. Keep its directory writable and protected.

When `enabled=true` and `allow_volatile_fallback=false` (default), WAL open or replay failure prevents daemon startup. Do not silently reconstruct an empty security state. Set `allow_volatile_fallback=true` only for explicit volatile/dev operation.

## `[xdp]`

| Key | Baseline | Built-in default |
|---|---|---|
| `enabled` | `true` | `false` |
| `interface` | `eth0` | `eth0` |
| `mode` | `skb` | `skb` |
| `allow_inband_fallback` | `false` | `false` |

When `enabled=true` and `allow_inband_fallback=false` (default), XDP attach failure prevents daemon startup (`/healthz` 503, `protection_state: failed`). Set `allow_inband_fallback=true` to run degraded instead (in-band enforcement only, `protection_state: degraded`).

Valid modes are the values currently accepted by the XDP implementation, including `skb` and `drv`.

## `[forecasting]`

| Key | Baseline | Built-in default |
|---|---:|---:|
| `enabled` | `true` | `true` |
| `ewma_alpha` | `0.3` | `0.3` |
| `hw_beta` | `0.1` | `0.1` |
| `hw_gamma` | `0.1` | `0.1` |
| `seasonality_period` | `60` | `3600` |
| `anomaly_zscore` | `3.0` | `2.5` |
| `min_entropy` | `4.5` | `2.0` |

## Environment overrides

Example:

```bash
export RAMSHIELD_DETECTION__RPS_THRESHOLD=1000
export RAMSHIELD_IPC__TCP_ADDR='127.0.0.1:7890'
```

Typed values are validated.

## Validation behavior

Validation happens as part of configuration loading/startup. Important current guards include:

- `engine.ram_limit_mb >= 64`;
- `engine.shard_count` is a non-zero power of two;
- positive detection thresholds and valid batch bounds;
- public dashboard binds require an admin password hash;
- public IPC binds require auth keys and a TLS proxy boundary;
- `require_auth = true` requires at least one auth key.

There is currently no standalone `ramshield config validate` CLI command. Invalid configuration should therefore be treated as a startup failure.

## Secrets

Do not commit:

- IPC HMAC keys;
- dashboard password hashes you intend to keep private;
- other deployment secrets.

Use an ignored overlay or environment variables.