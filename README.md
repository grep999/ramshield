# RamShield — Enterprise-Grade Traffic Defense

[![Crates.io](https://img.shields.io/crates/v/ramshield.svg)](https://crates.io/crates/ramshield)
[![CI](https://github.com/grep999/ramshield/actions/workflows/ci.yml/badge.svg)](https://github.com/grep999/ramshield/actions/workflows/ci.yml)
[![Clippy](https://github.com/grep999/ramshield/actions/workflows/clippy.yml/badge.svg)](https://github.com/grep999/ramshield/actions/workflows/clippy.yml)
[![Coverage](https://img.shields.io/codecov/c/github/grep999/ramshield)](https://codecov.io/gh/grep999/ramshield)
[![License](https://img.shields.io/crates.io/l/ramshield)](https://github.com/grep999/ramshield/blob/main/LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024%2B-orange.svg)](https://www.rust-lang.org/)
[![docs.rs](https://img.shields.io/docsrs/ramshield/latest)](https://docs.rs/ramshield/latest)

> **RAM-first DDoS detection and mitigation engine** with optional XDP eBPF kernel acceleration.

RamShield is a specialized in-memory engine that sits at your network edge and:
- **Detects** volumetric, protocol, and application-layer attacks in real time
- **Decides** on mitigation actions in < 50ms using adaptive algorithms
- **Enforces** blocks via userspace IPC or kernel-level XDP packet drop
- **Operates** with zero external dependencies — single binary deployment

---

## 🏗️ Architecture

```
┌─────────────┐     ┌─────────────────────────────────────────────────┐
│  Edge /     │     │              ramshield daemon                   │
│  Proxies    │────►│                                                 │
│  (nginx,    │     │  ┌──────────┐    ┌──────────────────────┐     │
│   HAProxy,  │     │  │  Engine  │───►│  DetectionEngine     │     │
│   Envoy)    │     │  └────┬─────┘    │  (batch processor)   │     │
└─────────────┘     │       │         └──────────┬───────────┘     │
                    │       │                    │                 │
                    │       ▼                    ▼                 │
                    │  ┌──────────┐    ┌──────────────────────┐     │
                    │  │  Store   │◄───│  Forecaster          │     │
                    │  │ (DashMap)│    │  (Holt-Winters +     │     │
                    │  └──────────┘    │   Entropy)           │     │
                    │       ▲          └──────────────────────┘     │
                    │       │                    │                 │
                    │       │    BlockDecision   │                 │
                    │  ┌────┴────┐                │                 │
                    │  │Enforce  │                │                 │
                    │  │Service  │                │                 │
                    │  └────┬────┘                │                 │
                    │       │                     │                 │
                    │       ▼                     │                 │
                    │  ┌──────────┐    ┌──────────────────────┐     │
                    │  │ XDP Mgr  │───►│  eBPF/XDP Program    │     │
                    │  └──────────┘    │  (kernel space)      │     │
                    └─────────────────────────────────────────────────┘
```

### Key Design Principles

| Principle | Implementation |
|-----------|----------------|
| **RAM-first** | All hot-path data in sharded `DashMap`; configurable hard limit |
| **Batch-first** | 50ms / 4096-event windows; amortizes hash+lock costs |
| **Promotion filter** | Only IPs with ≥8 hits/window get full `IpRecord` tracking |
| **Subnet-scale first** | /24 counters updated per batch; catches distributed floods early |
| **Single-writer enforcement** | `EnforcementService` is sole mutator of block state |
| **XDP offload (optional)** | Kernel-level drop via eBPF hash map; zero userspace copy |

---

## ✨ Features

### Detection & Mitigation
- **EWMA rate tracking** per IP with configurable thresholds
- **Subnet batch blocking** — automatic /24 blocks on coordinated abuse
- **Holt-Winters forecasting** — preemptive blocks on anomaly z-score
- **Entropy analysis** — detects botnet uniformity across subnets
- **Threat scoring** — composite of RPS, error rate, and history

### Enforcement Pipeline
- **Idempotent operations** — UUID `decision_id` prevents replay
- **In-memory deduplication** — `HashSet<IpAddr>` skips redundant XDP ops
- **WAL crash durability** — WAL-first commit (append→mutate→XDP), automatic startup replay restores live blocks, TTL-aware, segment retention with pruning (`[wal]` config, off by default)
- **TTL wheel** — automatic expiry without background scans
- **XDP integration** — targeted `insert`/`remove` on BPF hash map

### IPC & Integration
- **JSON over TCP** — simple integration from any language
- **Batch endpoint** — `report_connections` for high-throughput edge proxies
- **Aggregation layer** — server-side per-IP count thresholding
- **CLI tool** — `ramshield-cli` for operator actions

### Observability
- **HTTP dashboard** — real-time metrics at `:9999`
- **Structured logging** — `RUST_LOG=ramshield=debug`
- **Atomic counters** — requests, blocks, events, promotions
- **Prometheus-compatible** — `Metrics::emit_prometheus()`

---

## 🚀 Quick Start

### Prerequisites
- Rust 1.80+ (2024 edition)
- Linux kernel ≥ 5.10 for XDP (optional)

### Build & Run

```bash
# Clone and build
git clone https://github.com/grep999/ramshield
cd ramshield/beta/rs
cargo build --release

# Run with default config (512 MB RAM, 256 shards)
./target/release/ramshield config.toml

# Run with production config (8 GB RAM, 1024 shards)
./target/release/ramshield config.stress.toml
```

### Verify

```bash
# Health check
curl http://127.0.0.1:7891/healthz

# Dashboard
open http://127.0.0.1:9999

# CLI
./target/release/ramshield-cli stats
./target/release/ramshield-cli check 1.2.3.4
./target/release/ramshield-cli block 1.2.3.4 --reason manual --ttl 3600
```

---

## 📦 Configuration

Configuration via `config.toml` (CLI arg) or environment variables.

```toml
[engine]
shard_count = 256          # DashMap shards (power of 2)
ram_limit_mb = 512         # Hard memory ceiling

[detection]
rps_threshold = 1000       # EWMA RPS block trigger
promote_min_events = 8     # Hits before full IpRecord
subnet_window_threshold = 500  # /24 volume for subnet block
batch_max_events = 4096    # Batch flush size
batch_window_ms = 50       # Max batch wait time

[forecasting]
enabled = true
anomaly_zscore = 2.5       # Holt-Winters z-score threshold
min_entropy = 2.0          # Shannon entropy floor (bits)

[ipc]
tcp_addr = "127.0.0.1:7890"
max_connections = 256

[wal]                      # Crash-durable block state (off by default)
enabled = true
dir = "/var/lib/ramshield/wal"
durability = "Flush"       # None | Flush | Fsync | GroupCommit (currently = Fsync; single-writer actor has nothing to batch)
compress = true            # LZ4 records > 64B
seg_max_bytes = 67108864   # 64 MB segment rotation
retention_max_bytes = 536870912  # 512 MB total cap; oldest pruned (0 = unlimited)

[xdp]                      # Optional kernel acceleration
enabled = false
interface = "eth0"
build_mode = "auto"        # auto | rust | clang | stub
```

**WAL crash recovery:** with `[wal].enabled = true`, every block/unblock is
journaled before state mutation (WAL-first). On restart the WAL is replayed:
live blocks are restored into the store and re-armed in XDP by reconciliation;
TTL-expired blocks are skipped; unblocks cancel earlier blocks. Verified
end-to-end: block → kill → restart → `restored N live blocks` in the log.

**Environment overrides** (prefix `RAMSHIELD_`):
```bash
RAMSHIELD_ENGINE__RAM_LIMIT_MB=2048 \
RAMSHIELD_DETECTION__RPS_THRESHOLD=2000 \
RAMSHIELD_XDP__ENABLED=true \
RAMSHIELD_XDP__INTERFACE=eth0 \
./target/release/ramshield config.toml
```

---

## 🔌 IPC Protocol

**Transport:** TCP (default `127.0.0.1:7890`)  
**Framing:** One JSON object per line (`\n` terminated)

### Core Endpoints

| Request | Purpose | Example |
|---------|---------|---------|
| `report_connection` | Single event (backward compat) | `{"type":"report_connection","ip":"1.2.3.4","bytes":512,"status_code":200}` |
| `report_connections` | Batch events (high throughput) | `{"type":"report_connections","events":[...]}` |
| `check_ip` | Query block status & threat | `{"type":"check_ip","ip":"1.2.3.4"}` |
| `block_ip` | Manual block | `{"type":"block_ip","ip":"1.2.3.4","reason":"manual","ttl_secs":3600}` |
| `unblock_ip` | Clear block | `{"type":"unblock_ip","ip":"1.2.3.4"}` |
| `get_stats` | Engine statistics | `{"type":"get_stats"}` |
| `get_ip_stats` | Detailed IP record | `{"type":"get_ip_stats","ip":"1.2.3.4"}` |

### Batch Integration (nginx/lua example)

```lua
-- Accumulate in shared dict, flush every 50ms
local batch = ngx.shared.ramshield_batch
table.insert(batch, {ip=ngx.var.remote_addr, bytes=ngx.var.body_bytes_sent, status_code=ngx.status})
if #batch >= 100 then
    local json = cjson.encode({type="report_connections", events=batch})
    tcp_send("127.0.0.1", 7890, json .. "\n")
    batch = {}
end
```

---

## ⚡ XDP eBPF Acceleration (Linux)

When enabled, RamShield loads an eBPF XDP program that drops packets for blocked IPs **in kernel space** — before they reach userspace.

### Requirements
| Component | Version | Notes |
|-----------|---------|-------|
| Kernel | ≥ 5.10 | XDP hook support |
| NIC driver | `ixgbe`, `i40e`, `mlx5`, etc. | XDP-capable |
| Privileges | `CAP_SYS_ADMIN` | Required for `bpf_prog_load` |
| Toolchain (Rust) | LLVM 21 + `bpf-linker` | Preferred path |
| Toolchain (C) | `clang` + `linux-libc-dev` | Fallback path |

### Build Pipeline

```
cargo build --release
    │
    ├─► Try 1: aya-build (Rust) ──► bpfel-unknown-none
    │       Requires: nightly, bpf-linker, LLVM 21
    │
    ├─► Try 2: clang -target bpf ──► C source
    │       Requires: clang, kernel headers
    │
    └─► Try 3: Stub ELF (4 bytes) ──► Always succeeds
            Guarantees `cargo check` never fails
```

### Runtime Behavior
```rust
// Startup: full blocklist sync from Store
xdp.sync_blocklist(&mut blocklist)?;

// Runtime: targeted updates only
xdp.apply_block_decision(ip)?;   // BPF map insert
xdp.remove_block(ip)?;           // BPF map remove
```

---

## 📊 Performance Characteristics

Measured on 4-core dev box, `scripts/attack_nexus.py` mixed-profile load:

| Metric | Measured | Notes |
|--------|----------|-------|
| IPC `check_ip` latency | p50 0.29 ms / p99 1.05 ms | 200-probe under sustained flood |
| Sustained ingest | 30–40k events/s per driver worker set | 0 rejections across 10M+ event soak |
| Decision latency | < 50 ms | P99 under load |
| Throughput ceiling | 1M+ events/s | Batch path, 8-core |
| Memory | Hard-limited | `CapacityExceeded` enforced at `ram_limit_mb`; RSS stable ~230 MB under 10M-event soak |
| XDP drop latency | ~100 ns | Kernel fast path |
| False positive rate | < 0.1% | Bloom filter + promotion |

*Attack profiles: l7_http_flood, volumetric_syn, slowloris, dns_amplification,
botnet_entropy, api_abuse + red_team_full chain (`scripts/profiles.json`).*
*Benchmarks run with `scripts/attack_nexus.py` against local instance.*

---

## 🛡️ Security

- **No TLS on IPC** — intended for localhost/trusted network only
- **No authentication** — deploy behind firewall or in isolated network
- **Input validation** — all IPC requests validated; invalid IP → 400
- **Resource limits** — RAM ceiling, connection caps, channel backpressure
- **Audit trail** — `EnforceCommand` includes actor, source, timestamp, policy version

### Threat Model
| Threat | Mitigation |
|--------|------------|
| Memory exhaustion | Hard RAM limit + promotion filter |
| IPC channel flood | 2M event capacity + 503 backpressure |
| Blocklist replay | UUID `decision_id` idempotency |
| Kernel exploit | Minimal eBPF surface; verifier enforces safety |

---

## 📚 Documentation

| Document | Description |
|----------|-------------|
| [Technical Documentation](docs/DOCUMENTATION.md) | Full architecture, modules, config, integration |
| [API Reference](https://docs.rs/ramshield/latest) | Generated Rust docs |
| [Contributing](CONTRIBUTING.md) | Development workflow, style, testing |
| [Security Policy](SECURITY.md) | Vulnerability reporting |
| [Changelog](CHANGELOG.md) | Release history |

---

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Run `cargo test && cargo clippy --all-targets -- -D warnings`
4. Commit with conventional messages (`feat:`, `fix:`, `docs:`, etc.)
5. Open a Pull Request

See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

---

## 📄 License

Dual-licensed under **MIT** or **Apache-2.0** at your option.

---

## 🔗 Links

- **Crates.io**: https://crates.io/crates/ramshield
- **Documentation**: https://docs.rs/ramshield/latest
- **Repository**: https://github.com/grep999/ramshield
- **Issues**: https://github.com/grep999/ramshield/issues
- **Discussions**: https://github.com/grep999/ramshield/discussions
