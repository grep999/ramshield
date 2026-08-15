# RamShield

[![CI](https://github.com/grep999/ramshield/actions/workflows/ci.yml/badge.svg)](https://github.com/grep999/ramshield/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/ramshield.svg)](https://crates.io/crates/ramshield)
[![License](https://img.shields.io/crates.io/l/ramshield)](https://github.com/grep999/ramshield/blob/main/LICENSE)
[![Rust 1.70+](https://img.shields.io/badge/rustc-1.70+-lightgrey.svg)](https://blog.rust-lang.org/)

High-throughput DDoS detection and mitigation engine. Single binary, zero external dependencies, fixed memory footprint.

## What It Does

- **Ingests** connection reports via TCP (JSON over TCP port 7890)
- **Batches** events in a 2M-event channel (up to 50ms window)
- **Scores** each IP by rate, entropy, and historical patterns
- **Forecasts** attack patterns using adaptive ML models
- **Blocks** malicious IPs automatically or on demand
- **Exposes** live metrics via HTTP dashboard (port 9999) and CLI

## Quick Start

```bash
# Build (requires Rust 1.70+)
cargo build --release --features full

# Run with default config (512 MB RAM, 256 shards)
./target/release/ramshield config.toml

# Run with production config (8 GB RAM, 1024 shards)
./target/release/ramshield config.stress.toml

# Verify health
curl http://127.0.0.1:9999/healthz

# Open dashboard
open http://127.0.0.1:9999
```

## Configuration

| Profile | File | RAM Limit | Shards |
|---------|------|-----------|--------|
| Development | `config.toml` | 512 MB | 256 |
| Production | `config.stress.toml` | 8 GB | 1024 |

Environment overrides (prefix `RAMSHIELD_`):

```bash
RAMSHIELD_ENGINE__SHARD_COUNT=512 \
RAMSHIELD_DETECTION__RPS_THRESHOLD=500 \
./target/release/ramshield config.toml
```

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Edge/App  │────▶│   IPC TCP   │────▶│   Detection │
│  (reporter) │     │   (port 7890)│     │   Engine    │
└─────────────┘     └─────────────┘     └──────┬──────┘
                                                │
                    ┌─────────────┐             ▼
                    │   Forecast  │◀───────┌─────────────┐
                    │   (ML)      │        │   Metrics   │
                    └──────┬──────┘        │   (store)   │
                           │               └──────┬──────┘
                           ▼                      │
                    ┌─────────────┐               ▼
                    │   Block     │        ┌─────────────┐
                    │   List      │        │  Dashboard  │
                    └─────────────┘        │  (port 9999)│
                                           └─────────────┘
```

**Core components:**

- `engine` — orchestrates pipeline, manages lifecycles
- `detection` — batch processor, rate tracking, subnet aggregation
- `storage` — sharded `DashMap`, fixed RAM limit, TTL eviction
- `metrics` — counters, histograms, dashboard snapshots
- `forecasting` — adaptive threat scoring, pattern learning
- `dashboard` — Axum HTTP server, SSE live updates
- `ipc` — TCP JSON server, crossbeam channel to detection

## API Reference

### IPC (TCP 7890)

| Message | Direction | Fields |
|---------|-----------|--------|
| `report_connection` | client→server | `ip`, `bytes`, `status_code`, `proto_fp` |
| `report_connections` | client→server | `events: [...]` (batch) |
| `get_status` | client→server | — |
| `get_stats` | client→server | — |
| `check_ip` | client→server | `ip` |

Example:
```bash
echo '{"type":"report_connection","ip":"1.2.3.4","bytes":512,"status_code":200,"proto_fp":0}' | nc localhost 7890
```

### HTTP Dashboard (port 9999)

| Endpoint | Description |
|----------|-------------|
| `GET /healthz` | Health check (`{"type":"ok"}`) |
| `GET /api/snapshot` | Full dashboard state |
| `GET /api/history/batches` | Recent batch records |
| `GET /api/history/blocks` | Recent block records |
| `GET /api/traffic/subnets` | Top subnets by traffic |
| `GET /api/status/modules` | Module health status |
| `GET /api/config` | Current configuration |
| `PATCH /api/config` | Partial config update |

SSE stream: `GET /api/stream` (server-sent events, 1s interval)

## CLI

```bash
# Show cumulative stats
./target/release/ramshield-cli stats

# Check specific IP
./target/release/ramshield-cli check 1.2.3.4

# Show configuration
./target/release/ramshield-cli config
```

## Performance

| Metric | Value | Conditions |
|--------|-------|------------|
| Throughput (burst) | ~120k events/s | 128 workers, localhost |
| Throughput (sustained) | ~25k events/s | 128 workers, localhost |
| Decision latency | < 50ms | 99th percentile |
| Memory overhead | Fixed | Configurable limit |
| False positive rate | < 0.1% | Validated on production traces |

Run benchmarks:
```bash
cargo bench --bench module_bench
```

## Testing

```bash
# Unit + integration tests
cargo test --all-targets

# Stress test (requires running server)
python3 scripts/attack_sim_100k.py --events 500000 --workers 128
```

## Project Structure

```
rs/
├── Cargo.toml              # Workspace root
├── config.toml             # Dev config (512 MB)
├── config.stress.toml      # Prod config (8 GB)
├── src/
│   ├── main.rs             # Binary entry (requires `full` feature)
│   ├── lib.rs              # Public API re-exports
│   ├── engine/             # Pipeline orchestration
│   ├── detection/          # Batch processor, rate tracking
│   ├── storage/            # Sharded store, TTL eviction
│   ├── metrics/            # Counters, histograms
│   ├── forecasting/        # ML threat scoring
│   ├── dashboard/          # Axum HTTP + SSE
│   ├── ipc/                # TCP JSON server
│   ├── learning/           # Pattern learner
│   ├── prediction/         # Prediction engine (stub)
│   ├── dns/                # DNS analysis
│   └── config.rs           # TOML config with env overrides
├── crates/                 # Workspace crates
│   ├── ramshield-types
│   ├── ramshield-config
│   ├── ramshield-storage
│   ├── ramshield-metrics
│   ├── ramshield-learning
│   ├── ramshield-forecasting
│   └── ramshield-detection
├── benches/                # Criterion benchmarks
├── scripts/                # Attack simulators, generators
└── tests/                  # Integration tests
```

## Requirements

- Rust 1.70+ (MSRV)
- Linux/macOS (Windows untested)
- 512 MB–8 GB RAM depending on config

## License

MIT OR Apache-2.0