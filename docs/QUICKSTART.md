# RamShield QUICKSTART

Build and run RamShield in 30 seconds.

## First Run (No XDP)

```bash
# 1. Clone and build
git clone https://github.com/grep999/ramshield.git
cd ramshield
cargo build --release --locked --features full

# 2. Start with baseline config, no XDP
# The baseline contains a public dev-only loopback HMAC key; replace it before production.
cp config.baseline.toml config.toml
RAMSHIELD_AUTONOMOUS__ENABLED=false RAMSHIELD_NATIVE_INGEST__ENABLED=false RAMSHIELD_SYNPROXY__ENABLED=false \
./target/release/ramshield --config config.toml --no-xdp

# 3. Verify it's running
curl -fsS http://127.0.0.1:9999/healthz
./target/release/ramshield-cli status
```

## Production Run (With XDP)

```bash
# 1. Same build as above
# 2. Start with XDP enabled (requires host kernel, interface, driver qualification)
cp config.baseline.toml config.toml
./target/release/ramshield --config config.toml

# 3. Verify health
curl -fsS http://127.0.0.1:9999/healthz
./target/release/ramshield-cli status

# 4. Dashboard available at http://localhost:9999
```

## CLI Commands

```bash
# Check a specific IP
ramshield-cli check 203.0.113.7

# Block an IP (manual, 5 min TTL)
ramshield-cli block 203.0.113.7 --reason manual --ttl 300

# Unblock an IP
ramshield-cli unblock 203.0.113.7

# View status
ramshield-cli stats
ramshield-cli info 203.0.113.7
```

## Health Check

```bash
# Orchestration health
curl http://127.0.0.1:9999/healthz

# Prometheus metrics
curl http://127.0.0.1:9999/metrics
```

## SYNPROXY host prerequisites

When enabling SYNPROXY, install the host-level kernel settings before starting
RamShield. The daemon checks these values and fails closed if they do not match;
it does not change global kernel tunables itself.

```bash
sudo install -D -m 0644 deploy/sysctl.d/ramshield-synproxy.conf \
  /etc/sysctl.d/90-ramshield-synproxy.conf
sudo modprobe nf_conntrack
sudo sysctl --system

# Confirm all required values before enabling SYNPROXY.
sysctl -n net.ipv4.tcp_syncookies
sysctl -n net.ipv4.tcp_timestamps
sysctl -n net.netfilter.nf_conntrack_tcp_loose
```

Expected values, in order: `1`, `1`, `0`. The systemd service keeps
`ProtectKernelTunables=true`; do not add write access to `/proc/sys`.

## Production Notes

- Do not expose IPC or dashboard publicly without authentication and TLS boundary
- Keep control plane on loopback or behind authenticated TLS boundary
- XDP qualification requires actual kernel, driver, and capability verification
- WAL must be enabled for production enforcement
- See [TUNING.md] for detection threshold tuning
- See [ARCHITECTURE.md] for runtime topology and data flow