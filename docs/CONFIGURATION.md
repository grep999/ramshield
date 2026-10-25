# Configuration

The configuration is TOML. Environment variables can override config values using:

```text
RAMSHIELD_<SECTION>__<KEY>
```

The canonical repository example is [`config.baseline.toml`](../config.baseline.toml). The schema lives in [`crates/ramshield-config/src/sections/`](../crates/ramshield-config/src/sections/) (one file per `[section]`, with its serde defaults); validation lives in [`crates/ramshield-config/src/validate.rs`](../crates/ramshield-config/src/validate.rs) and environment overrides in [`crates/ramshield-config/src/env.rs`](../crates/ramshield-config/src/env.rs).

The baseline file intentionally contains explicit values for development/testing. It is not a universal production tuning profile.

## `[engine]`

|| Key | Baseline | Built-in default | Notes |
|---|---:|---:|---|---|
|| `ram_limit_mb` | `1024` | `512` | Hard store budget; validation requires at least 64 MB. |
|| `shard_count` | `8` | `256` | Must be a power of two. |
|| `worker_threads` | `0` | `available_parallelism()` | `0` lets Tokio choose worker count; built-in default resolves to available core count. |

## `[detection]`

| Key | Baseline | Built-in default |
|---|---:|---:|
| `rps_threshold` | `500` | `1000` |
| `rate_window_secs` | `10` | `10` |
| `promote_min_events` | `4` | `8` |
| `relative_enabled` | `false` | `false` |
| `relative_factor` | `5.0` | `5.0` |
| `relative_floor_rps` | `3.0` | `3.0` |
| `relative_min_samples` | `8` | `8` |
| `emergency_burst_threshold` | `500` | `500` |
| `batch_window_ms` | `25` | `50` |
| `batch_max_events` | `4096` | `4096` |
| `pre_aggs_flush_interval_ms` | `250` | `1000` |
| `pre_aggs_max_size` | `1000000` | `1000000` |
| `bloom_bits` | `8000000` | `8000000` |
| `subnet_batch_threshold` | `64` | `50` |
| `subnet_batch_min_events` | `500` | `100` |
| `subnet_window_threshold` | `12` | `500` |
| `batch_block_enabled` | `true` | `true` |
| `block_ttl_secs` | `300` | `3600` |
| `subnet_burst_ttl_secs` | `60` | `120` |
| `pulse_window_secs` | `5` | `6` |
| `pulse_threshold_samples` | `3` | `2` |

Use the baseline values only when they are appropriate for your workload. Detection thresholds are policy, not universal safe values.

## `[ipc]`

|| Key | Baseline | Built-in default | Notes |
|---|---:|---:|---|---|
|| `tcp_addr` | `127.0.0.1:7890` | `127.0.0.1:7890` | TCP bind address. |
|| `max_connections` | `128` | `1024` | Max concurrent connections; serde default is 1024. |
|| `max_line_length` | `262144` | `1048576` | Max frame bytes (None → 1 MiB). |
|| `max_connection_bytes` | `1048576` | `1048576` | Max total bytes per connection (None → 1 MiB). |
|| `read_timeout_ms` | `5000` | `5000` | Read timeout. |
|| `write_timeout_ms` | `5000` | `5000` | Write timeout. |
|| `connection_idle_timeout_ms` | `30000` | `30000` | Idle timeout. |
|| `auth_keys` | `[]` | `[]` | HMAC keys as `key_id:hex`; empty = loopback dev default. |
|| `require_auth` | `false` | `false` | Require auth on loopback (CI/staging). |
|| `require_auth_for_mutations` | `true` | `true` | Refuse mutating requests without verified principal. |

`require_auth_for_mutations` refuses a state-mutating request (`block_ip`,
`block_cidr`, `unblock_ip`, `unblock_cidr`, `flush`) that carries no verified
principal. It is only reachable when `auth_keys` is empty — with keys
configured, every frame carries a principal and the role table applies. Set it
to `false` to restore the unauthenticated operator path on a host where that is
an accepted risk; it cannot widen the exposure past loopback, because a
non-loopback bind still requires `auth_keys`.

For a non-loopback IPC bind, configuration validation requires authentication and also requires `behind_tls_proxy = true`.

## `[dashboard]`

|| Key | Baseline | Built-in default | Notes |
|---|---:|---:|---|---|
|| `enabled` | `true` | `true` | Dashboard HTTP server; default true. |
|| `http_addr` | `127.0.0.1:9999` | `127.0.0.1:9999` | Bind address; default `127.0.0.1:9999`. |
|| `block_log_size` | `1000` | `1000` | Block-history ring size. |
|| `admin_password_hash` | unset | unset | Argon2 PHC hash of admin password. Set via overlay or env. |
|| `require_auth_for_config` | `true` | `true` | Require valid session for `POST /api/config`. |
|| `session_ttl_secs` | `28800` | `28800` | Session lifetime in seconds (default 8h). |
|| `max_login_attempts` | `50` | `50` | Max failed login attempts before lockout. |
|| `max_password_length` | `1024` | `1024` | Max accepted password length. |
|| `trusted_proxies` | `[]` | `[]` | Reverse proxies trusted for X-Forwarded-For. |
|| `tls_enabled` | `false` | `false` | RamShield has no built-in TLS stack. |
|| `cookie_secure` | unset | unset | Force or disable Secure flag on session cookies. |

A non-loopback dashboard bind requires `admin_password_hash`.

RamShield has no built-in TLS listener. `tls_enabled` controls cookie behavior for an external HTTPS termination boundary; it does not enable a TLS server.

## `[wal]`

|| Key | Baseline | Built-in default | Notes |
|---|---:|---:|---|---|
|| `enabled` | `true` | `false` | WAL off by default; enable for crash-durable block state. |
|| `dir` | `/tmp/ramshield-wal` | `/var/lib/ramshield/wal` | WAL storage directory; baseline uses /tmp, code default is /var/lib/ramshield/wal. |
|| `durability` | `GroupCommit` | `Flush` | GroupCommit = fsync amortized across concurrent commits; Flush = fdatasync per write. |
|| `compress` | `true` | `true` | LZ4 segment compression. |
|| `seg_max_bytes` | `67108864` | `67108864` | Segment rotation threshold in bytes (64 MiB). |
|| `retention_max_bytes` | `1073741824` | `536870912` | Max total WAL size on disk (0 = unlimited). |
|| `allow_volatile_fallback` | `false` | `false` | WAL enabled + open/replay failure = startup failure when false. |

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

||| Key | Baseline | Built-in default | Notes |
||---|---|---:|---:|---|
||| `enabled` | `true` | `true` | Forecasting enabled. |
||| `ewma_alpha` | `0.3` | `0.3` | EWMA smoothing factor. |
||| `hw_beta` | `0.1` | `0.1` | Holt-Winters beta. |
||| `hw_gamma` | `0.1` | `0.1` | Holt-Winters gamma. |
||| `seasonality_period` | `60` | `3_600` | Seasonality period in seconds; baseline 60s is very short for production. |
||| `anomaly_zscore` | `3.0` | `2.5` | Anomaly detection z-score threshold. |
||| `min_entropy` | `4.5` | `2.0` | Minimum entropy for anomaly scoring. |

## `[autonomous]`

||| Key | Baseline | Built-in default | Notes |
||---|---|---:|---:|---|
||| `enabled` | `false` | `false` | First-line packet protection in XDP; requires XDP enabled. |
||| `syn_pps_per_cpu` | `25000` | `25000` | Per-CPU SYN packets/sec budget. |
||| `udp_pps_per_cpu` | `50000` | `50000` | Per-CPU UDP packets/sec budget. |
||| `packet_pps_per_cpu` | `200000` | `200000` | Per-CPU packets/sec budget for all IP traffic. |
||| `window_ms` | `100` | `100` | Fixed kernel accounting window in ms. |

## `[native_ingest]`

||| Key | Baseline | Built-in default | Notes |
||---|---|---:|---:|---|
||| `enabled` | `false` | `false` | AF_PACKET observation; disabled by default. |
||| `interface` | `veth0` | `eth0` | Interface to observe; baseline uses veth0 (dev), code default is eth0. |
||| `max_events_per_sec` | `20000` | `20000` | Telemetry admission cap per second. |

## `[synproxy]`

||| Key | Baseline | Built-in default | Notes |
||---|---|---:|---:|---|
||| `enabled` | `false` | `false` | Linux kernel SYNPROXY for configured TCP ports. |
||| `ports` | `[22, 53, 80, 443]` | `[22, 53, 80, 443]` | Protected TCP ports. |
||| `mss` | `1460` | `1460` | TCP MSS for SYNPROXY. |
||| `wscale` | `7` | `7` | TCP window scale. |
||| `max_connections_per_source` | `128` | `128` | Max tracked connections per source IP. |
||| `max_connections_total` | `16384` | `16384` | Max total tracked connections. |
||| `new_connections_per_second` | `50` | `50` | New connections per source per second. |
||| `new_connection_burst` | `100` | `100` | Initial burst above steady rate. |
||| `new_connections_total_per_second` | `2000` | `2000` | Global new connection rate. |
||| `new_connection_total_burst` | `4000` | `4000` | Global initial connection burst. |

## `[mesh]`

||| Key | Baseline | Built-in default | Notes |
||---|---|---:|---:|---|
||| `enabled` | `false` | `false` | Fleet mesh CRDT synchronization; experimental. |
||| `node_id` | `1` | `1` | Node identifier; must be non-zero when enabled. |
||| `listen_addr` | `127.0.0.1:7900` | `127.0.0.1:7900` | Mesh listener address. |
||| `peers` | `[]` | `[]` | Peer socket addresses. |
||| `auth_key` | `""` | `""` | Local node HMAC key; compatibility only. |

## `[upstream]`

||| Key | Baseline | Built-in default | Notes |
||---|---|---:|---:|---|
||| `enabled` | `false` | `false` | Upstream saturation escalation (FlowSpec/RTBH). |
||| `interface` | `veth0` | `eth0` | Interface for saturation monitoring. |
||| `link_capacity_mbps` | `0` | `0` | Link capacity in Mbps; 0 disables saturation calculation. |
||| `saturation_pct` | `90` | `90` | Saturation threshold percentage (50-99). |
||| `poll_ms` | `1000` | `1000` | Poll interval in ms (>= 250). |
||| `webhook_url` | `""` | `None` | Optional webhook URL for escalation events. |
||| `cooldown_secs` | `60` | `60` | Cooldown between escalation announcements. |
||| `bgp_mode` | `none` | `none` | BGP mode: `none`, `flowspec`, or `rtbh`. |
||| `bgp_fifo` | `""` | `None` | BGP FIFO path; required when bgp_mode is not `none`. |
||| `protected_prefix` | `""` | `None` | Protected CIDR prefix; required for non-none BGP modes. |
||| `bgp_community` | `""` | `None` | RTBH blackhole community as `ASN:VALUE`. |

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