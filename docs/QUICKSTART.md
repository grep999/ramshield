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

## Production Notes

- Do not expose IPC or dashboard publicly without authentication and TLS boundary
- Keep control plane on loopback or behind authenticated TLS boundary
- XDP qualification requires actual kernel, driver, and capability verification
- Enable WAL (`wal.enabled=true`) for production enforcement if blocks must survive restart; WAL is optional and off by default in code defaults.
- See [TUNING.md] for detection threshold tuning
- See [ARCHITECTURE.md] for runtime topology and data flow