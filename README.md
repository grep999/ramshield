# RamShield

**DDoS mitigation that decides in milliseconds and drops at the kernel.**

Rust · IPv4+IPv6 XDP/BPF line-rate blocking · EWMA + CUSUM + pulse-wave detection · HMAC-signed IPC · WAL-durable blocklist · live operations console

[![CI](https://img.shields.io/github/actions/workflow/status/grep999/ramshield/ci.yml?style=flat-square&label=ci)](https://github.com/grep999/ramshield/actions)
[![license](https://img.shields.io/github/license/grep999/ramshield?style=flat-square)](LICENSE)
[![rust](https://img.shields.io/badge/rust-1.85%2B%20(ed.2024)-orange?style=flat-square&logo=rust)](https://doc.rust-lang.org/edition-guide/rust-2024/)
[![tests](https://img.shields.io/badge/tests-190%20passing-00e589?style=flat-square)](#benchmarks)

---

Your proxy sees the attack *after* the kernel has already spent on it. RamShield
spends nothing: verdicts are computed from authenticated connection events, and
confirmed attackers are blackholed in a **BPF hash map** — packets die at L3,
before socket, before your app, before memory.

```text
 attacker ──▶ [ XDP: BLOCKLIST hit? DROP ]───miss───▶ your NIC ──▶ your app
                        ▲                                     │
                        │ one atomic map insert               │ connection events
                        │                                     ▼
              ┌─────────┴──────────┐   HMAC-SHA256 IPC   ┌──────────────┐
              │ enforcement actor  │◀────────────────────│  EWMA/CUSUM  │
              │ WAL + TTL expiry   │   64k-event channel │ + pulse-wave │
              └────────────────────┘                     │ + swarm gate │
                                                         └──────────────┘
```

## Why it exists

| | |
|---|---|
| **Drops at line rate** | XDP/BPF `XDP_DROP` — no userspace copy, no connection state, no syscall for the victim path |
| **Three detection brains** | per-IP EWMA (rate), CUSUM (sustained sub-threshold creep), pulse-wave (2s-on/3s-off evaders) + subnet swarm gate (50 unique IPs + 100 events per 2s window; /24 on v4, /64 on v6) |
| **No false-positive theatre** | 0.0000% FPR across a 21M-event, 21-phase benchmark; cold one-shot traffic is *skipped*, not scored |
| **Survives crashes** | WAL with CRC32 + lz4, quarantine of corrupt tails, blocklist replayed AND TTL schedules re-armed on boot — a restart does not un-ban an attacker, and does not ban one forever |
| **Hardened control plane** | HMAC-SHA256 frame auth with nonce replay store, Argon2id console login, per-IP lockout, fail-closed config validation |
| **Boring to run** | one static binary, 335 KB resident after 21M events (0.004% of RAM budget), systemd-friendly |

## Quick start

```bash
git clone https://github.com/grep999/ramshield && cd ramshield/beta/rs
cargo build --release --locked --features full
```

Grant the binary the minimum kernel privileges (one-time, root):

```bash
sudo setcap 'cap_net_admin,cap_bpf,cap_perfmon+eip' ./target/release/ramshield
```

`config.toml`:

```toml
[xdp]
enabled = true
interface = "eth0"     # your NIC
mode = "skb"           # "drv"/"hw" for native once validated

[ipc]
tcp_addr = "127.0.0.1:7890"                     # your apps talk here
auth_keys = ["k1:PASTE-32-BYTES-HEX"]           # openssl rand -hex 32

[dashboard]
http_addr = "127.0.0.1:9999"                    # console + /metrics
admin_password_hash = "argon2id-hash"           # echo -n 'pw' | argon2 "$(head -c16 /dev/urandom | xxd -p)" -id -e

[detection]
rps_threshold = 5000
rate_window_secs = 10
subnet_batch_threshold = 50                     # unique IPs per 2s window (/24 v4, /64 v6)
```

Run it:

```bash
mkdir -p /var/lib/ramshield/wal
./target/release/ramshield --config config.toml
```

Console at `http://127.0.0.1:9999`, Prometheus at `/metrics`, health at `/healthz`.

### Feeding events

Any signed newline-delimited JSON frame over TCP. Reference client:

```python
import hmac, hashlib, json, socket, time

KEY = bytes.fromhex("PASTE-32-BYTES-HEX")
def sign(ts, p):
    m = hmac.new(KEY, digestmod=hashlib.sha256)
    m.update(str(ts).encode()); m.update(b"."); m.update(p)
    return m.hexdigest()

def send(req):
    ts = int(time.time() * 1000)
    p = json.dumps(req, separators=(",", ":"), sort_keys=True).encode()
    env = {**req, "auth": {"key_id": "k1", "ts_ms": ts, "sig": sign(ts, p)}}
    with socket.create_connection(("127.0.0.1", 7890)) as s:
        s.sendall(json.dumps(env, separators=(",", ":"), sort_keys=True).encode() + b"\n")
        return json.loads(s.recv(8192).decode())

send({"type": "report_connections", "events": [
    {"ip": "203.0.113.9", "bytes": 2048, "status_code": 200, "proto_fp": 0}
    for _ in range(100)
]})
```

Events flow: **HMAC verify → nonce check → 64k bounded channel → sharded
pre-aggregator (cold IPs skipped) → batch merge into per-IP records → detection
verdict → enforcement actor (WAL → XDP map insert, TTL expiry)**.

## Benchmarks

Laptop-class single host (Linux 6.8, i7, 1×GbE loopback path), Rust edition 2024.
Full methodology: [`docs/DDOS_BENCHMARK_REPORT.md`](docs/DDOS_BENCHMARK_REPORT.md).

| | RamShield |
|---|---|
| IPC throughput, HMAC-signed | **135,602 events/s** |
| Sustained flood (single attacker) | **154,731 events/s** |
| Distributed flood (50 attackers) | 115,278 events/s |
| Detection latency (warm) | **108 ms** |
| Block → recovery visible in snapshot | **52 ms** |
| False positives, 21M events / 21 phases | **0.0000%** |
| Resident memory after 21M events | 335 KB (0.004% of budget) |
| Concurrent control connections | 1,019 (ulimit-bound) |
| RFC 9411 probe oracle under attack | 100% / 100% |

Versus the open-source field: MHDDoS / GoldenEye / slowhttptest are attack
simulators — they transmit, they do not defend. The defensive comparables
(`xdp-ddos-protect`, `holon-rs`) ship a single heuristic and no published
benchmarks. RamShield's differentiators: three stacked statistical detectors +
subnet-swarm gate, durable (WAL-replayed) blocking, authenticated event
ingestion, and a built-in console — at laptop-class zero-fuss footprint.

## Console

Live throughput chart (canvas, 4-minute window, block-event markers), pipeline
funnel with sparkline proportions, hot-subnet intensity bars, block feed with
fresh-row highlighting, store-RAM/CPU/RSS gauges. Pauses polling when the tab
is hidden; respects `prefers-reduced-motion`. No JS frameworks — one file,
zero build step, ~30 KB.

`/metrics` is Prometheus text format, scrape-public; everything else is behind
Argon2id session auth when a password hash is configured.

## Architecture

Workspace: a thin root binary crate + 9 focused crates.

| crate | job |
|---|---|
| `ramshield-types` | shared primitives, error taxonomy |
| `ramshield-config` | parse + **fail-closed validation** (file *and* env-merged final) |
| `ramshield-protocol` | HMAC frame auth, nonce replay store, sessions |
| `ramshield-detection` | ingest channel, sharded pre-aggs, EWMA/CUSUM/pulse, /24 (v4) + /64 (v6) swarm loop |
| `ramshield-storage` | DashMap store (RAM budget, blocked indexes), WAL, subnets |
| `ramshield-enforcement` | block/unblock actor, XDP map sync, TTL expiry, reconcile |
| `ramshield-forecasting` | Holt-Winters entropy baseline, preemptive threat sampling |
| `ramshield-metrics` | counters, snapshot, Prometheus exposition |
| `ramshield-xdp` | BPF object build + load (aya), clang fallback |

Hard-won invariants (see [`SECURITY.md`](SECURITY.md), [`docs/audits/`](docs/audits/)):
`block_state` is written **only** by the enforcement actor; detection mutates
stats via a single-shard-lock `Store::update_ip` RMW. The XDP key contract is
raw-wire-octet layout — byte order is load-bearing; v6 keys live in a dedicated
`BLOCKLIST6` map (family-isolated, 16-byte keys) and the kernel drop path is
runtime-verified (`scripts/verify_v6_drop.sh`). WAL replay refuses any
compressed record claiming more than the append cap (decompression-bomb gate).
Channel capacity is a `const` with exactly one definition site.

## Requirements

- Linux **5.8+** (`xdpgeneric`; 5.12+ for `xdpdrv`) — in-band mode works without any of this
- `cap_net_admin`, `cap_bpf`, `cap_perfmon` on the binary, or run with systemd `AmbientCapabilities=`
- Rust **1.85+** to build (edition 2024; nightly pinned for XDP build-std path)

## Development

```bash
cargo test --workspace --locked --features full          # 190 tests
cargo clippy --workspace --locked --features full --all-targets -- -D warnings
scripts/prod_smoke.sh                                    # boots + exercises the real binary
```

Adversarial audit history (security / correctness / dead-code / perf) is
carried in the commit messages — every P0 found since June 2026 shipped with
a regression test that fails without the fix — with working notes in
`docs/audits/`. Contributions welcome; read `CONTRIBUTING.md` first.

## License

MIT or Apache-2.0, at your option.
