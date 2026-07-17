# RamShield - High-Performance Rust-based Rate Limiting and Threat Detection

RamShield is a robust, concurrent, and highly optimized security service written in Rust, designed to protect web applications and APIs from various forms of malicious traffic, including DDoS attacks, brute-force attempts, and scraping. It operates as a real-time, in-memory detection and blocking engine, leveraging advanced data structures and asynchronous programming to maintain high throughput and low latency.

## Key Features

-   **High-Performance Architecture**: Built with Tokio's multi-threaded runtime, DashMap for concurrent storage, and `crossbeam_channel` for efficient inter-thread communication, ensuring optimal performance under heavy load.
-   **Configurable Detection Engine**: Features a batch-first processing pipeline that aggregates connection events, applies Exponentially Weighted Moving Average (EWMA) for rate tracking, and utilizes a Bloom filter for probabilistic IP tracking.
-   **Adaptive Threat Intelligence**: Incorporates Holt-Winters time series forecasting and Shannon entropy analysis to detect anomalies and identify emerging threats.
-   **WAL-based Persistence**: Write-Ahead Log (WAL) ensures recovery of blocked IP states across restarts, preventing loss of critical security decisions.
-   **Resource-Aware Storage**: An in-memory store with configurable RAM limits (`ram_limit_mb`) and a Time-To-Live (TTL) wheel for efficient eviction of expired entries. Supports bounded history for IP records to prevent unbounded memory growth.
-   **IPC for Integration**: A TCP-based JSON Inter-Process Communication (IPC) server allows external systems to interact with RamShield for reporting events, querying stats, and managing blocks.
-   **Real-time Dashboard**: An `Axum`-based HTTP server provides a live dashboard (`/api/snapshot`, `/api/stream`) for monitoring system health, traffic metrics, and active blocks. The dashboard runs on a dedicated thread to ensure responsiveness even under peak load.
-   **CLI Tooling**: A command-line interface (`ramshield-cli`) for convenient interaction with the running RamShield instance, including checking IP status, manually blocking/unblocking, and retrieving global statistics.
-   **Comprehensive Testing**: Extensive unit tests, integration tests, and Python-based attack simulators (`attack_nexus.py`, `stress_test.py`) to validate functionality and performance under various attack scenarios.

## Technical Details

<<<<<<< HEAD
-   **Rust 2021 Edition**: Leverages modern Rust features for safety and performance.
-   **Concurrency Model**: Achieves fearless concurrency through careful use of `Arc`, `RwLock`, `Mutex`, and `Atomic` types, adhering to Rust's ownership and borrowing rules.
-   **Error Handling**: Uses `thiserror` for library-level error definitions and `anyhow` for application-level error handling, ensuring robust and explicit error propagation.
-   **Configurability**: All major parameters are configurable via TOML files (`config.toml`, `config.stress.toml`) and can be overridden using environment variables (e.g., `RAMSHIELD_ENGINE__RAM_LIMIT_MB`).
=======
| Capability | Implementation |
|------------|----------------|
| **High-throughput ingest** | `crossbeam_channel` (2M cap) + dedicated batch thread — no Tokio spin |
| **Batch-first detection** | 50 ms / 4096-event windows; aggregates in memory before touching shared state |
| **Adaptive threat scoring** | EWMA RPS (α=0.3) + 5xx error rate → composite threat score |
| **Accurate time handling** | `now_ms()` returns milliseconds (was `now_secs()` returns seconds); `ts_ms` field now uses milliseconds for downstream consumers |
| **Accurate time handling** | `now_ms()` returns milliseconds (was `now_secs()` returns seconds); `ts_ms` field now uses milliseconds for downstream consumers |
| **Subnet-scale detection** | /24 counters on every batch; hot-subnet blocking + entropy anomaly detection |
| **Forecasting engine** | Holt-Winters (level/trend/seasonality) + Shannon entropy — O(1) per tick |
| **RAM budget enforcement** | Hard `ram_limit_mb`; insert-first-then-check net-growth accounting |
| **Graceful shutdown** | Ctrl+C drains batch channel, flushes pending events, closes IPC connections |
| **Config validation** | Startup bounds-checking on all numeric config keys |
| **Real-time dashboard** | Axum HTTP + SSE (5 s interval) at `:9999` — Dynatrace-inspired dark UI |
| **IPC API** | TCP JSON (port 7890); single-event + batch endpoints; auth token optional |
| **CLI tool** | `ramshield-cli` — check, block, unblock, stats, info |
| **Enterprise alerting** | Multi-severity (INFO/WARNING/HIGH/CRITICAL), cooldown, audit log (SOC2-ready) |
| **Promotion filter** | Bloom filter + hot-subnet + min-hits gating — cold IPs never touch the store |
| **Subnet reverse index** | O(1) IP→subnet lookups for hot-subnet blocking (10,000× speedup) |
| **Bounded forecasting** | Entropy blocks use top-128 threat sample — O(1) regardless of store size |
| **WAL persistence (optional)** | LZ4-compressed segments, configurable sync — not yet wired to Engine |

---

## Quick Start

```bash
# 1. Build (release mode)
cd rs
cargo build --release

# 2. Run with default config (512 MB RAM, 256 shards)
./target/release/ramshield config.toml

# 3. Or production config (8 GB RAM, 1024 shards)
./target/release/ramshield config.stress.toml

# 4. Verify health
curl http://127.0.0.1:9999/healthz

# 5. Open dashboard
# Browser → http://127.0.0.1:9999
```

---

## Binaries

| Binary | Purpose |
|--------|---------|
| `ramshield` | Long-running daemon — detection, forecasting, IPC, dashboard, alerting |
| `ramshield-cli` | Operator CLI — `check`, `block`, `unblock`, `stats`, `info` |

---

## Configuration

Two profiles ship with the repo:

| File | Profile | RAM | Shards | Use Case |
|------|---------|-----|--------|----------|
| `config.toml` | Default | 512 MB | 256 | Dev / small deployments |
| `config.stress.toml` | Production | 8 GB | 1024 | High-volume edge / stress tests |

**Environment overrides** (prefix `RAMSHIELD_`, double-underscore for nesting):

```bash
RAMSHIELD_ENGINE__RAM_LIMIT_MB=4096 \
RAMSHIELD_DETECTION__RPS_THRESHOLD=500 \
RAMSHIELD_IPC__TCP_ADDR=0.0.0.0:7890 \
RAMSHIELD_DASHBOARD__HTTP_ADDR=0.0.0.0:9999 \
./target/release/ramshield config.stress.toml
```

---

## IPC Protocol (TCP JSON, port 7890)

One JSON object per line (`\n` terminated). Request `type` uses `snake_case`.

| Request | Purpose | Example |
|---------|---------|---------|
| `report_connection` | Single event (legacy) | `{"type":"report_connection","ip":"1.2.3.4","bytes":512,"status_code":200,"proto_fp":0}` |
| `report_connections` | **Batch** (high throughput) | `{"type":"report_connections","events":[{"ip":"1.2.3.4","bytes":512,"status_code":200,"proto_fp":0}, ...]}` |
| `check_ip` | Query block status + threat score | `{"type":"check_ip","ip":"1.2.3.4"}` |
| `block_ip` | Manual block | `{"type":"block_ip","ip":"1.2.3.4","reason":"manual","ttl_secs":3600}` |
| `unblock_ip` | Remove block | `{"type":"unblock_ip","ip":"1.2.3.4"}` |
| `get_stats` | Global snapshot | `{"type":"get_stats"}` |
| `get_ip_stats` | Detailed IP record | `{"type":"get_ip_stats","ip":"1.2.3.4"}` |
| `flush` | Clear all state | `{"type":"flush"}` |

**Batch response:** `{"type":"batch_ok","accepted":N,"rejected":M}`

**Auth (optional):** Set `ipc.auth_token` in config → clients must include `"auth_token": "..."` in every request.

---

## Dashboard API (HTTP, port 9999)

| Endpoint | Description |
|----------|-------------|
| `GET /` | Dynatrace-inspired dark-theme dashboard (offline-capable, no CDN) |
| `GET /healthz` | `{status, uptime_secs, ips_tracked, events_ingested, blocks_active}` |
| `GET /api/stats` | Full `DashboardSnapshot` JSON (all KPIs, modules, history) |
| `GET /api/sse` | Server-Sent Events — pushes snapshot every 5 s |
| `GET /api/metrics` | Alias for `/api/stats` |
| `GET /api/events/batches` | Last 80 batch records |
| `GET /api/events/blocks` | Last 40 block events |
| `GET /api/modules` | Per-module stats (IPC, Detection, Forecasting, Storage) |
| `GET /api/config` | Full config (read) |
| `POST /api/config` | Full config replace (requires complete section objects) |
| `GET /api/config/:section` | Single section |
| `POST /api/config/:section` | Patch single section |
| `GET /api/export/stats` | Export snapshot |
| `GET /api/export/blocks` | Export block log |

**No-cache headers** on all HTML/API responses — prevents stale browser caching.

---

## CLI Reference

```bash
./target/release/ramshield-cli --addr 127.0.0.1:7890 <command>

Commands:
  check <ip>              # Block status + threat score + EWMA RPS
  block <ip> [--reason manual] [--ttl 3600]
  unblock <ip>
  stats                   # Global snapshot (same as /api/stats)
  info <ip>               # Detailed IP record
```

---

## Architecture Overview

```
                    ┌─────────────────────────────────────────────┐
   Edge / Scripts   │              ramshield daemon               │
  ───────────────►  │                                             │
   TCP JSON IPC     │  ┌─────────┐    ┌──────────────────────┐  │
   (port 7890)      │  │ Engine  │───►│ DetectionEngine      │  │
                    │  └────┬────┘    │  (batch thread)      │  │
                    │       │         └────────┬─────────────┘  │
                    │       │                  │                │
                    │       ▼                  ▼                │
                    │  ┌─────────┐    ┌──────────────────────┐  │
                    │  │ Store   │◄───│ Forecaster           │  │
                    │  │ DashMap │    │ (Tokio timers: HW +  │  │
                    │  └─────────┘    │  entropy, 1s/5s)     │  │
                    │       ▲         └──────────────────────┘  │
                    │       │ BlockDecision                    │
                    │  ┌────┴────┐                              │
                    │  │ Block   │                              │
                    │  │ applier │                              │
                    │  └─────────┘                              │
                    └─────────────────────────────────────────────┘
                              ▲                ▲
                              │                │
                    ┌─────────┴──────┐  ┌───────┴────────┐
                    │  Dashboard     │  │  ramshield-cli │
                    │  (Axum + SSE)  │  │  (TCP JSON)    │
                    │  port 9999     │  │  port 7890     │
                    └────────────────┘  └────────────────┘
```

**Runtime components:**
1. **IPC Server** (Tokio) — accepts TCP, one JSON request/line
2. **Event Channel** (crossbeam, 2M cap) — decouples ingest from detection
3. **Batch Processor** (dedicated `std::thread`) — buffers 50 ms / 4096 events, aggregates, promotes, merges
4. **Subnet Batch Loop** (Tokio, 500 ms) — reads hot `/24` prefixes, blocks member IPs via reverse index
5. **Forecaster** (Tokio, 1 s / 5 s) — Holt-Winters on global rate; Shannon entropy on subnet distribution
6. **Block Applier** (Tokio) — writes `BlockDecision` into store
7. **Alerting Engine** (Tokio, 5 s) — multi-severity alerts, cooldown, audit log
8. **Dashboard** (Axum, dedicated thread) — HTTP + SSE, serves static HTML from binary

---
>>>>>>> 5b4bb0e (Refactor: metrics to ms, detection timing; config tuning)

## Project Structure

```
rs/
├── src/
│   ├── main.rs          # Server entry point (binary)
│   ├── cli.rs           # CLI admin tool (binary)
│   ├── lib.rs           # Library root
│   ├── config.rs        # TOML config with validation
│   ├── error.rs         # RsError enum (thiserror)
│   ├── engine/          # Core orchestrator (Engine struct)
│   ├── detection/       # Batch-first detection pipeline
│   │   ├── batch.rs     # IP aggregation logic
│   │   └── rate_tracker.rs  # EWMA + threshold checks
│   ├── storage/         # DashMap store + WAL + TTL wheel
│   │   ├── blob_store.rs
│   │   ├── ttl_wheel.rs
│   │   └── wal.rs
│   ├── metrics/         # Atomic counters + dashboard snapshot
│   ├── forecasting/     # Holt-Winters + Shannon entropy
│   ├── dns/             # DNS monitoring + forecaster
│   │   └── forecasting/
│   ├── learning/        # Pattern learner
│   ├── prediction/      # Prediction engine
│   ├── dashboard/       # Axum HTTP server + SSE
│   ├── ipc/             # TCP JSON protocol
│   └── util/            # BoundedVecDeque, DataProcessor
├── scripts/             # Python attack simulators
│   ├── attack_sim_100k.py
│   ├── attack_nexus.py
│   ├── attack_extreme.py
│   └── stress_test.py
├── config.toml          # Default configuration (512 MB, 256 shards)
├── config.stress.toml   # Production-tuned configuration (8 GB, 1024 shards)
└── Cargo.toml           # Project dependencies and metadata
```

## Getting Started

### Prerequisites

-   Rustup: Follow instructions at [rustup.rs](https://rustup.rs/)
-   Python 3: For running attack simulation scripts.

### Build

Navigate to the `rs` directory and build the project:

```bash
cd rs
cargo build --release
```

This will produce the main `ramshield` binary and `ramshield-cli` in `target/release/`.

### Run

To run RamShield with the default configuration:

```bash
cd rs
./target/release/ramshield ./config.toml
```

For a production-tuned configuration:

```bash
cd rs
./target/release/ramshield ./config.stress.toml
```

Override configuration values using environment variables:

```bash
RAMSHIELD_DETECTION__RPS_THRESHOLD=500 RAMSHIELD_ENGINE__RAM_LIMIT_MB=4096 ./target/release/ramshield ./config.stress.toml
```

### Dashboard

Access the real-time dashboard in your browser: `http://127.0.0.1:7891`

Check service health:

```bash
curl http://127.0.0.1:7891/healthz
```

### CLI Usage

Get current system statistics:

```bash
./rs/target/release/ramshield-cli stats
```

Check IP status:

```bash
./rs/target/release/ramshield-cli check 192.168.1.100
```

Manually block an IP:

```bash
./rs/target/release/ramshield-cli block 192.168.1.100 --reason "manual override" --ttl 3600
```

### Attack Simulation

Use the provided Python scripts to simulate traffic and test RamShield's performance:

```bash
# Example: Run a 1 million event attack
python3 rs/scripts/attack_sim_100k.py --events 1000000 --workers 64
```

## Contributing

Contributions are welcome! Please ensure your code adheres to the Idiomatic Rust Coding Standards and passes the Self-Healing Protocol defined in `AGENTS.md`.

## License

This project is licensed under the MIT License.
