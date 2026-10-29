# RamShield — Self-Hosted Linux Ingress Defense

Detects and blocks abusive traffic at the kernel (XDP/eBPF) level.
For self-hosted/sovereign infra operators who can't afford enterprise DDoS mitigation.
Run it locally or behind your reverse proxy. No cloud dependency. No subscription. Just Rust.

[![GitHub release](https://img.shields.io/github/v/release/grep999/ramshield?label=release)](https://github.com/grep999/ramshield/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

## One-Command Install

**Requires:** root (XDP/eBPF maps, systemd service, /usr/local, /etc/ramshield, /var/lib/ramshield)

```bash
curl -fsSL https://raw.githubusercontent.com/grep999/ramshield/master/scripts/install.sh | sudo bash -s -- --version 0.6.0
```

### Userspace (non-root)

Downloads binary to `$HOME/.local/bin`, stores config/state in
`$HOME/.config/ramshield` / `$HOME/.local/share/ramshield`.  No kernel
XDP/eBPF blocking — only a userspace proxy.  Run as a normal user.

```bash
curl -fsSL https://raw.githubusercontent.com/grep999/ramshield/master/scripts/install-user.sh | bash -s -- --version 0.6.0
```

## Quick Start

Full step-by-step instructions (including IPC auth) live in **[docs/QUICKSTART.md](docs/QUICKSTART.md)**. Short version:

### Build

```bash
git clone https://github.com/grep999/ramshield.git
cd ramshield
cargo build --release --locked --features full
```

### Run — No XDP (userspace, dev / CI)

```bash
cp config.baseline.toml config.toml
./target/release/ramshield --config config.toml --no-xdp
```

Verify (baseline enables IPC auth — the public dev key must be supplied):

```bash
curl -fsS http://127.0.0.1:9999/healthz

export RAMSHIELD_IPC_KEY=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
./target/release/ramshield-cli --key "$RAMSHIELD_IPC_KEY" status
```

### Run — With XDP (kernel dataplane)

```bash
cp config.baseline.toml config.toml
# REQUIRED: set xdp.enabled = true, xdp.interface = "<your-nic>", replace the public IPC key,
# and point wal.dir at persistent storage.
sudo setcap 'cap_net_admin,cap_perfmon,cap_bpf=eip' ./target/release/ramshield
sudo ./target/release/ramshield --config config.toml
```

See [Quickstart](docs/QUICKSTART.md) for the complete checklist, [Configuration](docs/CONFIGURATION.md) for every setting, and [Dashboard access](docs/DASHBOARD_AUTH.md) for the operator password.

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
export KEY=<your-hex-key>
./target/release/ramshield-cli --key "$KEY" status
./target/release/ramshield-cli --key "$KEY" stats
./target/release/ramshield-cli --key "$KEY" check <ip>
./target/release/ramshield-cli --key "$KEY" info <ip>
```

Manual blocks:

```bash
./target/release/ramshield-cli --key "$KEY" block <ip> --reason manual --ttl 300
./target/release/ramshield-cli --key "$KEY" unblock <ip>
./target/release/ramshield-cli --key "$KEY" unblock-cidr <cidr>
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

## Documentation

- [Quickstart](docs/QUICKSTART.md)
- [Configuration](docs/CONFIGURATION.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Dashboard Auth](docs/DASHBOARD_AUTH.md)
- [Security](SECURITY.md)
- [Operations](docs/OPERATIONS.md)

## License

MIT
