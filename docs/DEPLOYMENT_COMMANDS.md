# Deployment Commands Reference

Core operational commands for installing and running RamShield as a security appliance.

## Build

```bash
cargo build --release --locked --features full
```

For development builds faster:
```bash
cargo build --locked --features full
```

## Configuration

### Production config
```bash
# Copy baseline and secure
cp config.baseline.toml config.toml

# Set secrets via environment (recommended)
export RAMSHIELD_DASHBOARD__ADMIN_PASSWORD_HASH='<argon2-hash>'
export RAMSHIELD_IPC__AUTH_KEYS='key1,key2'

# Or edit config.toml directly (never commit secrets)
vim config.toml
```

### Environment override examples
```bash
# Detection tuning
export RAMSHIELD_DETECTION__RPS_THRESHOLD=1000
export RAMSHIELD_DETECTION__BATCH_WINDOW_MS=25

# Engine limits
export RAMSHIELD_ENGINE__RAM_LIMIT_MB=512

# IPC bind address (non-loopback)
export RAMSHIELD_IPC__TCP_ADDR='0.0.0.0:7890'
export RAMSHIELD_TRUSTED_PROXIES='10.0.0.0/8'

# Dashboard
export RAMSHIELD_DASHBOARD__HTTP_ADDR='0.0.0.0:9999'
```

## Initial Setup

### No XDP (development / container)
```bash
RAMSHIELD_AUTONOMOUS__ENABLED=false \
RAMSHIELD_NATIVE_INGEST__ENABLED=false \
RAMSHIELD_SYNPROXY__ENABLED=false \
./target/release/ramshield --config config.toml --no-xdp
```

### Production with XDP
```bash
./target/release/ramshield --config config.toml
```

XDP attach failure blocks startup by default. For degraded mode:
```toml
[xdp]
allow_inband_fallback = true
```

## Health and Verification

```bash
# Health check
curl -fsS http://127.0.0.1:9999/healthz

# Prometheus metrics
curl -fsS http://127.0.0.1:9999/metrics

# Live status
./target/release/ramshield-cli stats

# Per-IP info
./target/release/ramshield-cli info 203.0.113.7
```

## CLI Command Reference

### Query
```bash
# Check if IP is blocked
ramshield-cli check 203.0.113.7
```

### Enforcement
```bash
# Block IP for 5 minutes
ramshield-cli block 203.0.113.7 --reason manual --ttl 300

# Unblock
ramshield-cli unblock 203.0.113.7

# Block CIDR prefix
ramshield-cli block-cidr 10.0.0.0/24 --reason attack --ttl 3600
```

### Status
```bash
# System status
ramshield-cli status

# Stats overview
ramshield-cli stats

# Block log
ramshield-cli blocks
```

## WAL and Recovery

```bash
# WAL directory location
WAL_DIR=/var/lib/ramshield/wal

# Verify WAL integrity manually (advanced)
ls -la $WAL_DIR/
```

For production: WAL **must** be enabled with durable sync. The baseline uses `GroupCommit` durability.

## Systemd Service Template

```ini
[Unit]
Description=RamShield Autonomous Ingress Defense Daemon
Documentation=https://github.com/grep999/ramshield
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/ramshield --config /etc/ramshield/config.toml
Restart=on-failure
RestartSec=5s
LimitNOFILE=65535

User=ramshield
Group=ramshield

AmbientCapabilities=CAP_NET_ADMIN CAP_BPF CAP_PERFMON CAP_NET_RAW
CapabilityBoundingSet=CAP_NET_ADMIN CAP_BPF CAP_PERFMON CAP_NET_RAW

NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectControlGroups=true
ProtectKernelTunables=true
ProtectKernelModules=true
ReadWritePaths=/var/lib/ramshield /dev/shm /sys/fs/bpf

[Install]
WantedBy=multi-user.target
```

### Install (production)

```bash
# 1. Build
cargo build --release --locked --features full

# 2. Install binary
sudo install -m 0755 target/release/ramshield /usr/local/bin/ramshield

# 3. Apply file capabilities (required for XDP attach)
sudo setcap cap_net_admin,cap_perfmon,cap_bpf=eip /usr/local/bin/ramshield

# 4. Install sysctl tunables (default path: /etc/sysctl.d/99-ramshield.conf)
sudo cp deploy/sysctl.d/99-ramshield.conf /etc/sysctl.d/99-ramshield.conf
sudo sysctl --system

# 5. Install systemd unit
sudo cp deploy/systemd/ramshield.service /etc/systemd/system/ramshield.service

# 6. Install config
sudo mkdir -p /etc/ramshield /var/lib/ramshield/wal
sudo cp config.baseline.toml /etc/ramshield/config.toml
sudo chown -R ramshield:ramshield /var/lib/ramshield /etc/ramshield

# 7. Enable and start
sudo systemctl daemon-reload
sudo systemctl enable --now ramshield
```

### Verify

```bash
# Health check
curl -fsS http://127.0.0.1:9999/healthz

# Systemd status
systemctl status ramshield

# Capabilities verified
getcap /usr/local/bin/ramshield

# Sysctl applied
sysctl net.ipv4.tcp_syncookies net.core.bpf_jit_enable
```

## Log Rotation

Standard systemd journal. View:
```bash
journalctl -u ramshield -f
```

## TLS Termination (Dashboard/IPC)

RamShield itself does not speak TLS. Place behind:
- Cloud Load Balancer with HTTPS termination
- Terminator-proxy (Envoy, Caddy, nginx)
- Kubernetes Ingress with TLS cert

Configure `trusted_proxies` and `tls_enabled=true` in config.toml for proper header forwarding and cookie security.

## Uninstall / Stop

```bash
systemctl stop ramshield
systemctl disable ramshield
rm -rf /opt/ramshield  # or wherever installed
```