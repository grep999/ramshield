# RamShield

High-performance traffic defense for web services. Written in Rust.

---

## What it does

RamShield sits in front of your web application and stops malicious traffic before it reaches your code. It processes millions of requests per second, makes blocking decisions in under 50 milliseconds, and keeps your service running during attacks.

**Use it to:**
- Block DDoS attacks and credential-stuffing attempts
- Stop scrapers and automated abuse
- Protect APIs without adding latency
- See live traffic health on a built-in dashboard

---

## Why it's different

| Aspect | Typical solutions | RamShield |
|--------|-------------------|-----------|
| **Decision speed** | 100–500 ms | < 50 ms |
| **Memory** | Unbounded, grows with traffic | Fixed budget you set |
| **Dependencies** | Redis, databases, external services | None — single binary |
| **Architecture** | Request-by-request | Batch-first, multi-core |
| **Learning** | Static rules | Adapts to traffic patterns |

---

## How it works

1. **Ingest** — Your edge or app sends connection events to RamShield over a TCP socket (JSON, one line per event or batched)
2. **Batch** — Events accumulate in a 2-million-event channel for up to 50 ms, then process together — no per-request overhead
3. **Score** — Each IP gets a threat score combining request rate, error rate, and subnet behavior
4. **Forecast** — Holt-Winters time-series models predict surges; Shannon entropy detects coordinated botnets
5. **Block** — Bad IPs are written to a sharded in-memory store with TTL; cold IPs never touch memory (Bloom filter gate)
6. **Observe** — Dashboard (HTTP + SSE), stats API, CLI, and optional alerting show you everything in real time

---

## Quick start

```bash
# 1. Build
cargo build --release

# 2. Run (default: 512 MB RAM, 256 shards)
./target/release/ramshield config.toml

# 3. Or production config (8 GB RAM, 1024 shards)
./target/release/ramshield config.stress.toml

# 4. Verify
curl http://127.0.0.1:9999/healthz

# 5. Open dashboard
# http://127.0.0.1:9999
```

---

## Configuration

Two profiles included:

| File | RAM | Shards | For |
|------|-----|--------|-----|
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

---

## Integration

**Send events (batch, high throughput):**
```json
{"type":"report_connections","events":[
  {"ip":"1.2.3.4","bytes":512,"status_code":200,"proto_fp":0},
  {"ip":"1.2.3.4","bytes":1024,"status_code":200,"proto_fp":0}
]}
```

**Check an IP:**
```json
{"type":"check_ip","ip":"1.2.3.4"}
```

**Get global stats:**
```json
{"type":"get_stats"}
```

Port: `7890` (TCP, newline-delimited JSON). Optional auth token in config.

---

## Dashboard API (port 9999)

| Endpoint | Description |
|----------|-------------|
| `GET /` | Live dashboard (dark theme, offline-capable) |
| `GET /healthz` | `{status, uptime_secs, ips_tracked, events_ingested, blocks_active}` |
| `GET /api/stats` | Full snapshot JSON |
| `GET /api/sse` | Server-Sent Events — pushes snapshot every 5 s |
| `GET /api/events/batches` | Recent batch records |
| `GET /api/events/blocks` | Recent block events |
| `GET /api/status/modules` | Per-module health |
| `GET /api/config` / `POST /api/config` | Read / write config at runtime |

All responses include `Cache-Control: no-cache`.

---

## CLI tool

```bash
./target/release/ramshield-cli --addr 127.0.0.1:7890 <command>

Commands:
  check <ip>              # Block status + threat score + rate
  block <ip> [--reason manual] [--ttl 3600]
  unblock <ip>
  stats                   # Global snapshot
  info <ip>               # Detailed IP record
```

---

## Architecture highlights

- **Single binary** — no external dependencies at runtime
- **Batch-first pipeline** — 50 ms / 4,096-event windows; dedicated OS thread, no async overhead in hot path
- **Sharded DashMap** — 256–1,024 shards, lock-free reads, scales with cores
- **TTL wheel** — O(1) expiry, no scanning
- **Subnet reverse index** — O(1) IP→/24 lookup; 10,000× faster subnet blocking
- **Bounded forecasting** — Holt-Winters + entropy on top-128 samples; O(1) regardless of store size
- **Graceful shutdown** — Ctrl+C drains channels, flushes blocks, closes connections
- **WAL persistence (optional)** — LZ4-compressed segments; not yet wired to engine

---

## Testing

```bash
# Unit + integration tests
cargo test

# Simulated attack (1M events, 64 workers)
python3 scripts/attack_sim_100k.py --events 1000000 --workers 64
```

---

## Project structure

```
rs/
├── src/
│   ├── main.rs          # Daemon entry point
│   ├── cli.rs           # CLI tool entry point
│   ├── lib.rs           # Library root
│   ├── config.rs        # TOML config with validation
│   ├── error.rs         # Error types
│   ├── engine/          # Core orchestrator
│   ├── detection/       # Batch pipeline + EWMA scoring
│   ├── storage/         # DashMap + TTL wheel + WAL
│   ├── metrics/         # Atomic counters + snapshot
│   ├── forecasting/     # Holt-Winters + Shannon entropy
│   ├── dashboard/       # Axum HTTP + SSE server
│   ├── ipc/             # TCP JSON protocol
│   └── util/            # BoundedVecDeque, helpers
├── scripts/             # Python attack simulators
├── config.toml          # Default config
├── config.stress.toml   # Production config
└── Cargo.toml
```

---

## Requirements

- Rust 1.70+ (2021 edition)
- Python 3 (for attack simulators only)

---

## License

MIT