# RamShield

[![Crates.io](https://img.shields.io/crates/v/ramshield.svg)](https://crates.io/crates/ramshield)
[![CI](https://github.com/grep999/ramshield/actions/workflows/ci.yml/badge.svg)](https://github.com/grep999/ramshield/actions/workflows/ci.yml)
[![Clippy](https://github.com/grep999/ramshield/actions/workflows/clippy.yml/badge.svg)](https://github.com/grep999/ramshield/actions/workflows/clippy.yml)
[![Coverage](https://img.shields.io/codecov/c/github/grep999/ramshield)](https://codecov.io/gh/grep999/ramshield)
[![License](https://img.shields.io/crates.io/l/ramshield)](https://github.com/grep999/ramshield/blob/main/LICENSE)

# RamShield - Enterprise-Grade Traffic Defense

## 🛡️ What It Is

RamShield is an **advanced, RAM-first DDoS detection and mitigation engine** designed for high-throughput environments. It:

- ✅ **Blocks malicious traffic** before it reaches your application
- ✅ **Processes millions of requests per second** with sub-50ms decisions
- ✅ **Uses zero external dependencies** - single binary, no databases or external services
- ✅ **Self-contained** - runs anywhere with no configuration needed
- ✅ **XDP-accelerated** kernel-level packet drop for blocked IPs (Linux)

---

## 🚀 Dashboard Overview

**Live Dashboard**: Accessible at `http://127.0.0.1:9999`

### Dashboard Features:
- **Neon Glow Effects** on traffic indicators and metrics
- **Grid Background** with neon grid pattern for the "Slick & Edgy" aesthetic
- **Real-time metrics** showing request rates, threat scores, and system health
- **Space Grotesk typography** for all text elements
- **Ultra-dark theme** with minimal UI elements for maximum focus

### Why It's Different

| Feature | Typical Solutions | RamShield |
|---------|-------------------|-----------|
| **Decision Speed** | 100-500ms | < 50ms |
| **Memory Usage** | Unbounded | Fixed limit (configurable) |
| **Dependencies** | Redis, databases, external services | None - single binary |
| **Architecture** | Request-by-request | Batch-first, multi-core |
| **Learning** | Static rules | Adaptive algorithms |
| **Kernel Offload** | N/A | XDP eBPF (Linux) |

---

## 🔧 How It Works

1. **Ingest**: Your edge server or app sends connection reports via TCP (JSON format)
2. **Batch**: Events accumulate in a 2M-event channel for up to 50ms
3. **Score**: Each IP gets a threat score combining rate, entropy, and history
4. **Forecast**: Predicts attack patterns using ML models
5. **Block**: Blocks malicious IPs automatically or on demand
6. **Accelerate**: XDP eBPF program drops packets at kernel level
7. **Observe**: View live traffic, metrics, and alerts in the dashboard

---

## 🚀 Quick Start

```bash
# Build
cargo build --release

# Run (default config)
./target/release/ramshield config.toml

# Or production config
./target/release/ramshield config.stress.toml

# Verify
curl http://127.0.0.1:7891/healthz

# Open dashboard
http://127.0.0.1:9999
```

### Configuration

- **Production**: `config.stress.toml` (8GB RAM, 1024 shards)
- **Development**: `config.toml` (512 MB RAM, 256 shards)
- **Custom**: Set via environment variables or config files

---

## 🛠️ Advanced Features

### XDP eBPF Acceleration (Linux)
- Kernel-level packet filtering via XDP (eXpress Data Path)
- Compiles to `bpfel-unknown-none` using `aya-ebpf`
- Falls back to clang C compilation if Rust BPF toolchain unavailable
- Requires `CAP_SYS_ADMIN` and kernel ≥ 5.10

### Enforcement Pipeline
- Single-writer architecture for all block/unblock operations
- In-memory deduplication prevents redundant XDP operations
- WAL-ready design for durability
- Automatic TTL expiry with timing wheel

### IPC Aggregation
- Aggregates connection counts per IP over configurable windows
- Threshold-based enforcement reduces channel traffic
- Batch `report_connections` endpoint for high throughput

---

## 📚 Documentation

- [Detailed Documentation](docs/DOCUMENTATION.md)
- [API Reference](https://docs.rs/ramshield/latest)
- [Contributing Guide](CONTRIBUTING.md)
- [Security Policy](SECURITY.md)

---

## 📚 Documentation

- [Detailed Documentation](docs/DOCUMENTATION.md)
- [API Reference](https://docs.rs/ramshield/latest)
- [API Examples](https://github.com/grep999/ramshield/tree/main/src/dashboard)
- [Developer Guide](docs/DEVELOPMENT.md)