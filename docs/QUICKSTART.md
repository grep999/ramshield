# RamShield QUICKSTART

Canonical guide to build and run RamShield. Follow these steps exactly — they match the current code and `config.baseline.toml` defaults.

## 1. Build

```bash
git clone https://github.com/grep999/ramshield.git
cd ramshield
cargo build --release --locked --features full
```

This produces:

- `./target/release/ramshield` — main daemon
- `./target/release/ramshield-cli` — control-plane client

## 2. First run (userspace only, no root, no XDP)

Safe starting point for development and CI. No kernel capabilities required.

```bash
cp config.baseline.toml config.toml

# Baseline already has autonomous / native_ingest / synproxy disabled
# and xdp.enabled = false. --no-xdp makes the intent explicit.
./target/release/ramshield --config config.toml --no-xdp
```

In a second terminal:

```bash
# Health endpoint (no auth)
curl -fsS http://127.0.0.1:9999/healthz

# CLI — baseline turns IPC auth ON and ships a public dev key.
# You MUST supply the key or every command fails.
export RAMSHIELD_IPC_KEY=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef

./target/release/ramshield-cli --key "$RAMSHIELD_IPC_KEY" status
./target/release/ramshield-cli --key "$RAMSHIELD_IPC_KEY" stats
```

Dashboard: http://127.0.0.1:9999 (loopback only by default).

## 3. Production-style run (with XDP)

Requires root (or the listed capabilities), a real NIC, and an eBPF-capable kernel.

```bash
cp config.baseline.toml config.toml
```

Edit `config.toml` before starting:

1. `[xdp]` → `enabled = true`
2. `[xdp]` → `interface = "eth0"` (or your real interface name)
3. `[xdp]` → `mode = "drv"` (or `"skb"` if native mode fails)
4. Replace the public IPC `auth_keys` value with a unique 256-bit key
5. Change `[wal].dir` from `/tmp/...` to a persistent path (e.g. `/var/lib/ramshield/wal`)
6. (Recommended) set `[dashboard].admin_password_hash`

Then:

```bash
# Re-apply after every rebuild — capabilities are lost when the binary is replaced
sudo setcap 'cap_net_admin,cap_perfmon,cap_bpf=eip' ./target/release/ramshield

sudo ./target/release/ramshield --config config.toml
```

Verify (use the **new** key you put in the config):

```bash
curl -fsS http://127.0.0.1:9999/healthz
export RAMSHIELD_IPC_KEY=<your-new-hex-key>
./target/release/ramshield-cli --key "$RAMSHIELD_IPC_KEY" status
```

## 4. Config validation

```bash
./target/release/ramshield --config config.toml --validate
```

Exits 0 only when the file parses and passes all validation rules.

## 5. CLI reference

All mutating / status commands need the HMAC key while `ipc.require_auth = true` (the baseline default).

```bash
export KEY=<hex-key-at-least-32-chars>

./target/release/ramshield-cli --key "$KEY" status
./target/release/ramshield-cli --key "$KEY" stats
./target/release/ramshield-cli --key "$KEY" check 203.0.113.7
./target/release/ramshield-cli --key "$KEY" info 203.0.113.7
./target/release/ramshield-cli --key "$KEY" block 203.0.113.7 --reason manual --ttl 300
./target/release/ramshield-cli --key "$KEY" unblock 203.0.113.7
./target/release/ramshield-cli --key "$KEY" unblock-cidr 203.0.113.0/24
```

You can also pass `--key-id k1` (default) when the server has multiple keys.

## 6. Health & metrics

```bash
curl -fsS http://127.0.0.1:9999/healthz
curl -fsS http://127.0.0.1:9999/metrics
```

`/healthz` returns 503 when the process is degraded (for example XDP failed to attach and `allow_inband_fallback` is false).

## Production notes

- Never leave the public baseline IPC key in place on a real host.
- Keep IPC and dashboard on loopback, or put a real TLS / mTLS proxy in front.
- XDP needs the three capabilities above; they must be re-applied after every binary update.
- For durable blocks across restarts, keep `[wal].enabled = true` and point `wal.dir` at persistent storage.
- Tuning thresholds and feature flags: see [CONFIGURATION.md](CONFIGURATION.md).
- Architecture and data flow: see [ARCHITECTURE.md](ARCHITECTURE.md).
- Dashboard password and session settings: see [DASHBOARD_AUTH.md](DASHBOARD_AUTH.md).
