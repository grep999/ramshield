# RamShield — Self-Hosted Linux Ingress Defense

Detects and blocks abusive traffic at the kernel (XDP/eBPF) level.
For self-hosted/sovereign infra operators who can't afford enterprise DDoS mitigation.
Run it locally or behind your reverse proxy. No cloud dependency. No subscription. Just Rust.

[![GitHub release](https://img.shields.io/github/v/release/grep999/ramshield?label=release)](https://github.com/grep999/ramshield/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

## One-Command Install

```bash
curl -fsSL https://ramshield.io/install.sh | bash
```

## Quick Start

### Build

```bash
git clone https://github.com/grep999/ramshield.git
cd ramshield
cargo build --release --locked --features full
```

### Run — No XDP (userspace proxy, dev / CI)

```bash
cp config.baseline.toml config.toml
RAMSHIELD_AUTONOMOUS__ENABLED=false \
RAMSHIELD_NATIVE_INGEST__ENABLED=false \
RAMSHIELD_SYNPROXY__ENABLED=false \
./target/release/ramshield --config config.toml --no-xdp
```

Verify:
```bash
curl -fsS http://127.0.0.1:9999/healthz
./target/release/ramshield-cli status
```

### Run — With XDP (kernel eBPF dataplane, production)

```bash
cp config.baseline.toml config.toml
# edit config.toml: xdp.enabled = true, xdp.interface = "eth0", xdp.mode = "drv"
sudo setcap cap_net_admin,cap_perfmon,cap_bpf=eip ./target/release/ramshield
./target/release/ramshield --config config.toml
```

Verify:
```bash
curl -fsS http://127.0.0.1:9999/healthz
./target/release/ramshield-cli status
```

See [Quickstart](docs/QUICKSTART.md) for full setup and [Configuration](docs/CONFIGURATION.md) for XDP tuning.

## What happens when traffic turns bad

RamShield collects telemetry, keeps a bounded view of recent activity, and looks for traffic that crosses the configured detection rules.

A block can then move through:

```text
detection
  ↓
decision
  ↓
WAL
  ↓
enforcement
  ↓
XDP
```

Blocks have TTLs, can be inspected from the CLI, and are removed when they expire.

```bash
ramshield-cli status
ramshield-cli stats
ramshield-cli check <ip>
ramshield-cli info <ip>
```

Manual blocks are available too:

```bash
ramshield-cli block <ip> --reason manual --ttl 300
ramshield-cli unblock <ip>
ramshield-cli unblock-cidr <cidr>
```

## XDP

When XDP is enabled, enforcement happens close to the network interface:

```text
packet
  ↓
NIC
  ↓
XDP
  ├── drop
  └── pass
```

RamShield exposes the XDP state so you can see whether the kernel dataplane is actually active.

## Modules Overview

| Module | Responsibility |
|--------|---------------|
| `ramshield-config` | TOML config, validation, env overrides |
| `ramshield-types` | IP networks, ConnectionEvent, IpRecord, CGNAT tiers |
| `ramshield-detection` | EWMA + CUSUM + Pulse + Subnet/swarm detection |
| `ramshield-enforcement` | XDP projection, reconcile, block-state transition |
| `ramshield-storage` | WAL, checkpoint, snapshots, recovery |
| `ramshield-mesh` | CRDT Aworset sync (disabled in production) |
| `ramshield-xdp` | eBPF XDP engine (kernel dataplane) |
| `ramshield-protocol` | IPC JSON framing, HMAC auth, replay store |
| `ramshield-cgnat` | CGNAT deployment with shm tables and classification |
| `ramwal` | WAL persistence: append-only log, checkpoints, snapshots |

## Configuration

RamShield uses TOML.

A starting point is included in the repository:

```text
config.baseline.toml
```

The configuration covers detection, batching, IPC, dashboard, WAL, XDP, forecasting and memory limits.

See [Configuration](docs/CONFIGURATION.md).

## Observe it

Health:

```bash
curl http://127.0.0.1:9999/healthz
```

Metrics:

```bash
curl http://127.0.0.1:9999/metrics
```

Dashboard and API are available from the same local service.

## Performance & Benchmarks

> Measured on **Intel Core i7-3687U CPU @ 2.10GHz**, **4 cores / 8 threads**, **15 GB RAM**, **Linux 6.8.0-147-generic**, **Rust 1.100.0-nightly (17fd5b8a3 2026-08-28)**. Methodology follows **RFC 9411** (Benchmarking Methodology for Network Security Devices).

### Measured Pipeline Latency & Throughput
| Pipeline Stage | Measured Latency | Throughput | Workload Context |
| :--- | :---: | :---: | :--- |
| **Detection Flush (Warm)** | **222400 ns/op** | **4496 ops/s** | 4096 events / 128 IPs (all promoted) |
| **Detection Flush (Cold Skip)** | **2606343 ns/op** | **384 ops/s** | 4096 events / 4096 IPs (Bloom rejection) |
| **Enforcement Commit (WAL)** | **3955 ns/op** | **252873 ops/s** | `GroupCommit` durability |
| **Idempotent Decision Cache** | **130 ns/op** | **7702531 ops/s** | Re-block deduplication |
| **Reverse-Proxy SHM Lookup** | **1.3 ns** | **761 M ops/s** | C11 Seqlock table (probe dist 0–3) |
| **Ingested Event Throughput** | — | **95653 eps** | Sustained multi-subnet flood (5 s, 1000 IPs) |

### Resource Footprint
- **Idle Memory:** `~266 KB` heap footprint (`14 MB` total process RSS)
- **Memory Under Attack:** Bounded by `engine.ram_limit_mb` (default `1024 MB`)
- **WAL Recovery Time:** `<60 ms` to restore state across restarts

---

## Architectural Comparison

| Capability / Metric | RamShield | fail2ban | CrowdSec | Nginx `limit_req` | Cloud Edge (e.g., Cloudflare) |
| :--- | :---: | :---: | :---: | :---: | :---: |
| **Enforcement Layer** | **XDP/eBPF (Kernel)** | Netfilter / iptables | Bouncer / iptables | Userspace (HTTP) | Anycast Edge Scrubbing |
| **Detection Method** | **EWMA + CUSUM + Pulse + Subnet** | Regex Log Parsing | Log / Scenarios | Leaky Bucket Token | Global ML / Auto-Thresholds |
| **Throughput (Events/sec)** | **95 653 eps (Measured)** | ~85k eps (via Nginx) | Untracked (Batch) | ~85k eps | Multi-Tbps (Cloud aggregate) |
| **Crash Durability** | **WAL + Checkpoint (CRC32-C)** | ❌ (State lost) | ⚠️ (DB sync) | ❌ (State lost) | Cloud redundant |
| **Cold-Start Defense** | **Stateless XDP PPS Budgets** | ❌ (Minutes to fire) | ❌ (Log-dependent) | ⚠️ (Immediate cap) | Global Edge Anycast |
| **Subnet / Swarm Defense** | **256-bit HostBitmap (/24, /64)** | ❌ | ⚠️ | ❌ | BGP Scrubbing |
| **Cloud / Feed Dependency**| **Zero (100% Sovereign)** | Zero | Community Feed | Zero | Proprietary Cloud |
| **Deployment Model** | **Single Binary / Node-Guard** | Daemon | Daemon + Bouncers | Webserver Module | DNS / BGP Proxy |

### Where RamShield Wins & Where It Loses
- **Where It Wins:** Sovereign, zero-subscription infrastructure; kernel-level packet drops before socket allocations; crash-durable WAL persistence; deterministic microsecond CUSUM drift detection; zero external feed dependencies.
- **Where It Loses (Stated Plainly):** Cannot defeat attacks that physically saturate the ISP uplink (requires BGP FlowSpec/RTBH upstream escalation); does not provide a community threat-intelligence database; not an HTTP/2 or WAF application-logic rules engine.

## Documentation

[Quickstart](docs/QUICKSTART.md) · [Operations](docs/OPERATIONS.md) · [Configuration](docs/CONFIGURATION.md) · [Architecture](docs/ARCHITECTURE.md) · [Troubleshooting](docs/TROUBLESHOOTING.md) · [Development](docs/DEVELOPMENT.md)

## Development

```bash
cargo test --workspace --locked --features full
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --features full -- -D warnings
```

See [Development](docs/DEVELOPMENT.md).

## License

MIT
## Threat-intelligence independence

RamShield does not depend on community IP reputation or a shared public ban feed. Enforcement decisions are derived from first-party local telemetry, trusted RamShield fleet signals, or explicit operator action. Community reputation is intentionally excluded from the authoritative enforcement vocabulary, so a feed outage cannot disable detection and an external reputation score cannot silently become a block decision.

This is deliberate: RamShield learns from the protected system and its trusted fleet rather than outsourcing the security boundary to a global reputation service.


## Security Gateway Architecture

The 0.6 security gateway closes the proxy-only observation gap with bounded Linux AF_PACKET telemetry, kernel-first XDP/SYNPROXY protection, deterministic bounded HTTP inspection, and optional ExaBGP FlowSpec/RTBH escalation. See `docs/SECURITY_GATEWAY.md`.
