> **A-021:** Nested tree is **not** maintained or shipped. See [STATUS.md](STATUS.md) and [docs/WORKSPACE_SCOPE.md](../docs/WORKSPACE_SCOPE.md).

# RamShield — self-hosted Linux ingress defense in Rust
Detects and blocks abusive traffic at the kernel (XDP/eBPF) level.
For self-hosted/sovereign infra operators who can't afford enterprise DDoS mitigation.
Run it locally or behind your reverse proxy.
No cloud dependency. No subscription. Just Rust.

[![GitHub release (latest by date)](https://img.shields.io/github/v/release/grep999/ramshield?label=release&color=0A0)](https://github.com/grep999/ramshield/releases/latest)


**Linux-native traffic protection with eBPF/XDP.**

RamShield watches traffic from your proxy, spots abusive patterns, and can block offending IPs or networks directly at the kernel level.

The basic loop is simple:

**observe → detect → decide → block → expire**

Run it:

```bash
git clone https://github.com/grep999/ramshield.git
cd ramshield
cargo build --release --locked --features full
```

Start locally:

```bash
cp config.baseline.toml config.toml
# The tracked baseline uses a public dev-only loopback HMAC key; replace it before production.
RAMSHIELD_AUTONOMOUS__ENABLED=false \
RAMSHIELD_NATIVE_INGEST__ENABLED=false \
RAMSHIELD_SYNPROXY__ENABLED=false \
./target/release/ramshield \
  --config config.toml \
  --no-xdp
```

Check that it is running:

```bash
curl http://127.0.0.1:9999/healthz
./target/release/ramshield-cli status
```

See the [Quickstart](docs/QUICKSTART.md) for the full setup.

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
