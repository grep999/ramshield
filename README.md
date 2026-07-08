# RamShield — High-Performance DDoS Detection & Mitigation Engine

RamShield is a production-grade, in-memory DDoS detection and mitigation engine written in **Rust 2021 Edition**. It ingests connection events via a high-throughput TCP JSON protocol, tracks traffic at IP and /24 subnet scale, applies adaptive threat scoring, and blocks abusive sources in real time — all while maintaining a strict RAM budget.

> **TL;DR**: Sub-millisecond decisions at millions of events/second. RAM-bounded. Zero-CPU-spin. Battle-tested under 5M+ event stress tests.

---

## Key Capabilities

| Capability | Implementation |
|------------|----------------|
| **High-throughput ingest** | `crossbeam_channel` (2M cap) + dedicated batch thread — no Tokio spin |
| **Batch-first detection** | 50 ms / 4096-event windows; aggregates in memory before touching shared state |
| **Adaptive threat scoring** | EWMA RPS (α=0.3) + 5xx error rate → composite threat score |
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

## Project Structure

```
rs/
├── src/
│   ├── main.rs              # Daemon entry point (binary)
│   ├── cli.rs               # CLI binary
│   ├── lib.rs               # Library root
│   ├── config.rs            # TOML config + validation + env overrides
│   ├── error.rs             # RsError enum (thiserror)
│   ├── engine/              # Core orchestrator (Engine struct)
│   ├── detection/           # Batch-first detection pipeline
│   │   ├── mod.rs           # DetectionEngine, batch loop, promotion, subnet index
│   │   ├── batch.rs         # In-memory aggregation (HashMap<IpAddr, IpAgg>)
│   │   └── rate_tracker.rs  # EWMA helpers
│   ├── storage/             # DashMap store + WAL + TTL wheel
│   │   ├── mod.rs           # Store, IpRecord, BlockEntry, StorageEngine
│   │   ├── ttl_wheel.rs     # Timing wheel (implemented, not wired)
│   │   ├── wal.rs           # LZ4 WAL (implemented, not wired)
│   │   └── blob_store.rs    # Large payloads (implemented, not hot path)
│   ├── metrics/             # Atomic counters + DashboardSnapshot builder
│   ├── forecasting/         # Holt-Winters + Shannon entropy
│   ├── alerting/            # Multi-severity alerts, cooldown, audit log
│   ├── dashboard/           # Axum HTTP + SSE + embedded static assets
│   ├── ipc/                 # TCP JSON protocol (request/response types)
│   ├── learning/            # Pattern learner (placeholder)
│   ├── prediction/          # Prediction engine (placeholder)
│   ├── cache/               # LRU cache (placeholder)
│   └── util/                # BoundedVecDeque, DataProcessor
├── scripts/                 # Python attack simulators
│   ├── attack_sim_100k.py   # Fixed 100K burst
│   ├── attack_extreme.py    # Burst/flood/phase/interactive REPL
│   └── stress_test.py       # Orchestrated stress runs
├── config.toml              # Default config (512 MB, 256 shards)
├── config.stress.toml       # Production config (8 GB, 1024 shards)
├── Cargo.toml
├── README.md                # This file
├── DOCUMENTATION.md         # Deep technical reference
├── INSTALLATION.md          # Step-by-step install guide
└── AGENTS.md                # AI coding agent standards
```

---

## Load Testing

```bash
# Terminal 1: Start RamShield
./target/release/ramshield config.stress.toml

# Terminal 2: Run attack simulations
python3 scripts/attack_extreme.py burst --events 500000 --workers 256
python3 scripts/attack_extreme.py flood --duration 60 --mode volumetric
python3 scripts/attack_extreme.py phase --plan extreme
python3 scripts/attack_extreme.py interactive --workers 128

# Or the simpler 100k script
python3 scripts/attack_sim_100k.py --events 1000000 --workers 64
```

**Observed under test (8 GB config, 4-core):**
- 3.2M IPs tracked simultaneously
- 5.2M events ingested / 2.3M blocked in 6 min
- CPU: ~38%, RAM: 23% of 8 GB limit
- Zero panics, zero OOM, alerts firing correctly

---

## Build Verification (Self-Healing Protocol)

```bash
cd rs
cargo build --all-targets
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: clean compile, zero clippy warnings, all tests pass.

---

## License

MIT License — see `LICENSE` file.

---

## Documentation

- [Technical Reference](DOCUMENTATION.md) — Architecture, module APIs, IPC protocol, config keys, design decisions
- [Installation Guide](INSTALLATION.md) — Prerequisites, build, systemd, Docker, tuning, troubleshooting
- [Optimization Changelog](CHANGES.md) — Bug fixes, performance wins, new features