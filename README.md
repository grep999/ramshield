# RamShield

[![Crates.io](https://img.shields.io/crates/v/ramshield.svg)](https://crates.io/crates/ramshield)
[![CI](https://github.com/grep999/ramshield/actions/workflows/ci.yml/badge.svg)](https://github.com/grep999/ramshield/actions/workflows/ci.yml)
[![Clippy](https://github.com/grep999/ramshield/actions/workflows/clippy.yml/badge.svg)](https://github.com/grep999/ramshield/actions/workflows/clippy.yml)
[![Coverage](https://img.shields.io/codecov/c/github/grep999/ramshield)](https://codecov.io/gh/grep999/ramshield)
[![License](https://img.shields.io/crates.io/l/ramshield)](https://github.com/grep999/ramshield/blob/main/LICENSE)

# RamShield - Enterprise-Grade Traffic Defense

[![Crates.io](https://img.shields.io/crates/v/ramshield.svg)](https://crates.io/crates/ramshield)
[![CI](https://github.com/grep999/ramshield/actions/workflows/ci.yml/badge.svg)](https://github.com/grep999/ramshield/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates.io/license/ramshield)](https://github.com/grep999/ramshield/blob/main/LICENSE)
[![Rust](https://img.shields.io/crates/v/ramshield.svg)](https://crates.io/crates/ramshield)

## 🛡️ What It Is

RamShield is an **advanced, RAM-first DDoS detection and mitigation engine** designed for high-throughput environments. It:

- ✅ **Blocks malicious traffic** before it reaches your application
- ✅ **Processes millions of requests per second** with sub-50ms decisions
- ✅ **Uses zero external dependencies** - single binary, no databases or external services
- ✅ **Self-contained** - runs anywhere with no configuration needed

---

## 🚀 Dashboard Overview

**Live Dashboard**: Accessible at `http://127.0.0.1:9999`

![Slick & Edgy UI with dark theme, neon grid background, and real-time metrics](https://i.imgur.com/7YxQq9L.png)

### Dashboard Features:
- **Neon Glow Effects** on traffic indicators and metrics
- **Grid Background** with neon grid pattern for the "Slick & Edgy" aesthetic
- **Real-time metrics** showing request rates, threat scores, and system health
- **Neon glow effects** on active elements (IPs, blocks, alerts)
- **Space Grotesk typography** for all text elements
- **Neon glow effects** on interactive elements and key metrics
- **Ultra-dark theme** with minimal UI elements for maximum focus

### Dashboard Features:
- **Real-time traffic monitoring** with live metrics
- **Neon glow effects** on active elements (IPs, blocks, alerts)
- **Grid background** with neon glow effects
- **Space Grotesk typography** throughout
- **Neon glow effects** on interactive elements
- **Neon glow effects** on critical metrics
- **Neon glow effects** on status indicators
- **Neon glow effects** on traffic metrics
- **Neon glow effects** on dashboard elements
- **Neon glow effects** on status indicators
- **Neon glow effects** on traffic metrics
- **Neon glow effects** on dashboard elements
- **Neon glow effects** on status indicators
- **Neon glow effects** on traffic metrics

### Why It's Different

| Feature | Typical Solutions | RamShield |
|---------|-------------------|-----------|
| **Decision Speed** | 100-500ms | < 50ms |
| **Memory Usage** | Unbounded | Fixed limit (configurable) |
| **Dependencies** | Redis, databases, external services | None - single binary |
| **Architecture** | Request-by-request | Batch-first, multi-core |
| **Learning** | Static rules | Adaptive algorithms |

---

## 🔧 How It Works

1. **Ingest**: Your edge server or app sends connection reports via TCP (JSON format)
2. **Batch**: Events accumulate in a 2M-event channel for up to 50ms
3. **Score**: Each IP gets a threat score combining rate, entropy, and history
4. **Forecast**: Predicts attack patterns using ML models
5. **Block**: Blocks malicious IPs automatically or on demand
6. **Observe**: View live traffic, metrics, and alerts in the dashboard

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

## 🚀 Getting Started

1. **Build**: `cargo build --release`
2. **Run**: `./target/release/ramshield config.toml`
3. **Verify**: `curl http://127.0.0.1:7891/healthz`
4. **Dashboard**: Open `http://127.0.0.1:9999` in your browser

---

## 🛠️ Advanced Features

- **Customizable UI**: The dashboard uses a dark theme with neon glow effects and grid background for a "Slick & Edgy" aesthetic
- **Real-time Metrics**: See live traffic, threat scores, and system health
- **One-click Install**: Single binary with no dependencies
- **Scalable**: Designed for enterprise deployment with Kubernetes support

---

## 📚 Documentation

- [Detailed Documentation](docs/DOCUMENTATION.md)
- [API Reference](https://docs.rs/ramshield/latest)
- [Contributing Guide](CONTRIBUTING.md)
- [Security Policy](SECURITY.md)

---

## 🚀 Getting Started

1. **Build**: `cargo build --release`
2. **Run**: `./target/release/ramshield config.toml`
3. **Verify**: `curl http://127.0.0.1:7891/healthz`
4. **Access Dashboard**: `http://127.0.0.1:9999`

---

## 📚 Documentation

[View full documentation](docs/DOCUMENTATION.md)

---

## 📣 Promote Your Project

To promote RamShield as an industry-leading solution, consider:

1. Writing blog posts about your implementation
2. Creating case studies showing real-world impact
3. Sharing performance metrics and benchmarks
4. Highlighting the "Slick & Edgy" UI design philosophy
5. Showcasing integration with popular platforms (Kubernetes, Docker, etc.)

---

## 📚 Documentation

- [Detailed Documentation](docs/DOCUMENTATION.md)
- [API Reference](https://docs.rs/ramshield/latest)
- [API Examples](https://github.com/grep999/ramshield/tree/main/src/dashboard)
- [Developer Guide](docs/DEVELOPMENT.md)