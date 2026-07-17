<div align="center">

# 🛡️ RamShield

**High-performance, RAM-first DDoS detection and mitigation engine**

[![CI](https://github.com/grep999/ramshield/actions/workflows/ci.yml/badge.svg)](https://github.com/grep999/ramshield/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Linux-lightgrey.svg)]()

*Millions of requests per second. Sub-50ms decisions. Zero external dependencies.*

[Getting Started](#-quick-start) · [Architecture](#-architecture) · [Documentation](docs/DOCUMENTATION.md) · [Contributing](CONTRIBUTING.md)

</div>

---

## Overview

RamShield sits in front of your web application and stops malicious traffic **before** it reaches your code. It processes millions of connection reports per second, scores threat levels in real time, and blocks abusive sources automatically.

Built in Rust for speed. Runs as a single binary with no databases, no Redis, no sidecars.

### Key Features

| Feature | Description |
|---------|-------------|
| ⚡ **Batch-first pipeline** | 50ms / 4096-event windows on a dedicated OS thread — no per-request overhead |
| 🧠 **Adaptive learning** | Holt-Winters forecasting predicts attack surges before thresholds are hit |
| 🔮 **Entropy analysis** | Shannon entropy detects coordinated botnets across /24 subnets |
| 💾 **RAM-budgeted** | Fixed memory cap you control; cold IPs never touch the store |
| 📊 **Live dashboard** | Dark-theme HTTP dashboard with SSE push, built on Axum |
| 🔌 **JSON/TCP protocol** | One line per event, batch API for high throughput — integrate from any language |
| 🎯 **CLI tool** | `ramshield-cli` for check, block, unblock, stats, info |
| 🚀 **Single binary** | No external dependencies at runtime |

---

## Architecture

```
                         ┌──────────────────────────────────┐
   Edge / Application    │         RamShield Daemon         │
   ───────────────────►  │                                  │
   TCP JSON IPC (:7890)  │  ┌──────────┐   ┌────────────┐  │
                         │  │  Engine   │──►│  Detection  │  │
                         │  │(orchestr.)│   │  (batch)    │  │
                         │  └────┬─────┘   └─────┬──────┘  │
                         │       │                │         │
                         │       ▼                ▼         │
                         │  ┌──────────┐   ┌────────────┐  │
                         │  │  Store    │◄──│ Forecaster │  │
                         │  │ DashMap   │   │ HW + Entropy│  │
                         │  └────┬─────┘   └────────────┘  │
                         │       │ BlockDecision            │
                         │  ┌────▼────┐                    │
                         │  │ Block   │                    │
                         │  │ Applier │                    │
                         │  └─────────┘                    │
                         │                                  │
                         │  Dashboard (:9999) ◄─ Axum/SSE  │
                         └──────────────────────────────────┘
```

**Data flow:** Edge reports → 2M event channel → batch thread (50ms window) → aggregation → promotion filter → IP scoring → block decision → sharded store

---

## Performance

| Metric | Value |
|--------|-------|
| Ingest throughput | 2M+ events/sec |
| Decision latency | < 50ms |
| Memory model | Fixed budget (default 512MB, production 8GB) |
| Shard count | 256–1024 (configurable) |
| Forecasting cost | O(1) per tick (reads counters, not full store) |
| Subnet blocking | O(1) per IP via reverse index |

Benchmarks available in [docs/BENCHMARKS.md](docs/BENCHMARKS.md). Run your own:

```bash
python3 scripts/attack_extreme.py burst --events 500000 --workers 256
```

---

## Quick Start

### 1. Build

```bash
git clone https://github.com/grep999/ramshield.git
cd ramshield/rs
cargo build --release
```

### 2. Run

```bash
# Development (512 MB RAM, 256 shards)
./target/release/ramshield config.toml

# Production (8 GB RAM, 1024 shards)
./target/release/ramshield config.stress.toml
```

### 3. Verify

```bash
curl http://127.0.0.1:7891/healthz
# {"status":"ok","uptime_secs":1,"ips_tracked":0,...}
```

### 4. Send traffic

```bash
# Single event
echo '{"type":"report_connection","ip":"1.2.3.4","bytes":512,"status_code":200,"proto_fp":0}' | nc localhost 7890

# Batch (high throughput)
echo '{"type":"report_connections","events":[{"ip":"1.2.3.4","bytes":512,"status_code":200},{"ip":"1.2.3.5","bytes":256,"status_code":404}]}' | nc localhost 7890
```

### 5. Check an IP

```bash
./target/release/ramshield-cli check 1.2.3.4
```

---

## Configuration

Two profiles included:

| File | RAM | Shards | Use Case |
|------|-----|--------|----------|
| `config.toml` | 512 MB | 256 | Development / small deployments |
| `config.stress.toml` | 8 GB | 1024 | Production / high-volume edge |

Override any setting with environment variables:

```bash
RAMSHIELD_ENGINE__RAM_LIMIT_MB=4096 \
RAMSHIELD_DETECTION__RPS_THRESHOLD=500 \
RAMSHIELD_IPC__TCP_ADDR=0.0.0.0:7890 \
RAMSHIELD_DASHBOARD__HTTP_ADDR=0.0.0.0:9999 \
./target/release/ramshield config.stress.toml
```

Full config reference in [docs/DOCUMENTATION.md](docs/DOCUMENTATION.md#9-configuration-reference).

---

## Dashboard

Built-in HTTP dashboard with dark theme:

```
http://127.0.0.1:9999
```

| Endpoint | Description |
|----------|-------------|
| `GET /` | Live dashboard (offline-capable) |
| `GET /healthz` | Health check + stats |
| `GET /api/stats` | Full snapshot JSON |
| `GET /api/sse` | Server-Sent Events (5s push) |
| `GET /api/events/batches` | Recent batch records |
| `GET /api/events/blocks` | Recent block events |
| `POST /api/config` | Hot-reload config |

---

## Integration

RamShield integrates with any edge proxy via TCP JSON:

```
Internet → Nginx/HAProxy → (access log / lua module)
                                │
                                ▼ report_connections
                           RamShield :7890
                                │
                                ▼ check_ip
                           Block or Allow
```

**Works with:** Nginx, HAProxy, Envoy, Go, Python, Lua, Rust, Node.js — anything that speaks TCP and JSON.

See [docs/DOCUMENTATION.md](docs/DOCUMENTATION.md) for full IPC protocol reference.

---

## Why RamShield?

| | Traditional (Redis + Lua) | iptables/nftables | RamShield |
|---|---|---|---|
| **Decision speed** | 100–500ms | < 1ms (packet) | < 50ms (application) |
| **DDoS detection** | Manual rules | None | Adaptive (Holt-Winters + entropy) |
| **Memory** | Unbounded | N/A | Fixed budget |
| **Subnet awareness** | Per-IP only | CIDR rules | /24 aggregation + reverse index |
| **Dependencies** | Redis, Lua runtime | Kernel modules | None (single binary) |
| **Dashboard** | External tools | None | Built-in |
| **Learning** | Static | Static | Adapts to traffic patterns |

---

## Roadmap

- [ ] TTL wheel background eviction (currently lazy)
- [ ] WAL persistence across restarts
- [ ] IPv6 subnet support
- [ ] Prometheus metrics export
- [ ] Webhook alerting (Slack, Discord, email)
- [ ] GeoIP-based regional rules
- [ ] Rule DSL for custom detection logic
- [ ] Multi-node consensus (Raft)
- [ ] Docker image + Helm chart

---

## Project Structure

```
rs/
├── src/
│   ├── main.rs              # Daemon entry
│   ├── cli.rs               # CLI tool
│   ├── engine/              # Core orchestrator + IPC
│   ├── detection/           # Batch pipeline + EWMA
│   ├── storage/             # DashMap + TTL + WAL
│   ├── forecasting/         # Holt-Winters + Shannon entropy
│   ├── dashboard/           # Axum HTTP + SSE
│   ├── metrics/             # Atomic counters
│   └── ipc/                 # TCP JSON protocol
├── scripts/                 # Attack simulators
├── docs/                    # Documentation
├── benches/                 # Benchmarks
└── .github/workflows/       # CI/CD
```

---

## Testing

```bash
# Unit + integration
cargo test

# Lint
cargo clippy --all-targets -- -D warnings

# Attack simulation
python3 scripts/attack_extreme.py burst --events 500000 --workers 256
python3 scripts/attack_extreme.py flood --duration 60 --mode volumetric
python3 scripts/attack_extreme.py interactive
```

---

## Requirements

- **Rust** 1.70+ (2021 edition)
- **Python 3.8+** (for attack simulators only)

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code standards, and PR process.

---

## Security

Report vulnerabilities via [SECURITY.md](SECURITY.md). Do not open public issues.

---

## Community

- [GitHub Discussions](https://github.com/grep999/ramshield/discussions)
- [Issues](https://github.com/grep999/ramshield/issues)

---

## License

[MIT](LICENSE)
