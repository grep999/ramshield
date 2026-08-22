<h1 align="center">🛡️ RamShield</h1>

<p align="center">
  <strong>RAM-first DDoS detection &amp; mitigation engine with kernel-space XDP enforcement.</strong><br>
  Detect · Decide · Enforce — in under 50&nbsp;ms, from a single static binary.
</p>

<p align="center">
  <a href="https://crates.io/crates/ramshield"><img src="https://img.shields.io/crates/v/ramshield.svg" alt="Crates.io"></a>
  <a href="https://github.com/grep999/ramshield/actions/workflows/ci.yml"><img src="https://github.com/grep999/ramshield/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/grep999/ramshield/actions/workflows/clippy.yml"><img src="https://github.com/grep999/ramshield/actions/workflows/clippy.yml/badge.svg" alt="Clippy -D warnings"></a>
  <a href="https://codecov.io/gh/grep999/ramshield"><img src="https://img.shields.io/codecov/c/github/grep999/ramshield" alt="Coverage"></a>
  <a href="https://github.com/grep999/ramshield/blob/main/LICENSE"><img src="https://img.shields.io/crates.io/l/ramshield" alt="License"></a>
  <img src="https://img.shields.io/badge/rust-2024%2B-orange.svg" alt="Rust 2024">
  <a href="https://docs.rs/ramshield/latest"><img src="https://img.shields.io/docsrs/ramshield/latest" alt="docs.rs"></a>
</p>
## Why RamShield

Edge proxies see attacks. They don't know what to *do* about them.
RamShield is the decision point between your reverse proxy and your origin:

- **Detects** volumetric, protocol and application-layer abuse in real time — EWMA rate tracking, /24 subnet correlation, Holt-Winters forecasting, payload-entropy analysis
- **Decides** in &lt;50 ms using batched evaluation over sharded in-memory state
- **Enforces** via IPC answers to your proxy — or drops packets **in the kernel** through an eBPF/XDP program before they ever reach userspace
- **Survives** crashes: WAL-first commit log replays live blocks back into store + XDP on restart

One binary. No external services. No GC pauses. Hard memory ceiling enforced by design.

```
nginx / HAProxy / Envoy ──batch──► RamShield ──check──► allow / deny
                                      │
                                      ▼ (optional, CAP_NET_ADMIN)
                                XDP eBPF drop @ ~100 ns/packet
```

---

## Highlights

| | |
|---|---|
| **:zap: Sub-millisecond queries** | `check_ip` p50 **0.29 ms**, p99 **1.05 ms** under sustained flood |
| **:chart_with_upwards_trend: High-throughput ingest** | 30–40k events/s sustained per driver set on a 4-core box; 1M+/s batch path ceiling; **0 rejections** across an 11.4M-event soak |
| **:brain: Four detection engines** | EWMA rate · subnet batch · Holt-Winters anomaly · Shannon entropy — composite threat score |
| **:nut_and_bolt: Kernel enforcement** | Optional aya-based XDP program drops blocked IPs at ~100 ns in kernel space |
| **:floppy_disk: Crash-durable state** | WAL-first commit (append → mutate → XDP). Restart replays live blocks automatically; TTL-aware; segment retention with pruning |
| **:package: Single binary** | Static build, zero runtime dependencies, JSON-over-TCP integration from any language |
| **:bar_chart: Live observability** | Dark ops dashboard + Prometheus-compatible metrics export |

<details>
<summary><b>Full feature list</b></summary>

### Detection & Mitigation
- **EWMA rate tracking** per IP with configurable thresholds
- **Subnet batch blocking** — automatic /24 blocks on coordinated abuse
- **Holt-Winters forecasting** — preemptive blocks on anomaly z-score
- **Entropy analysis** — detects botnet uniformity across subnets
- **Threat scoring** — composite of RPS, error rate, and history

### Enforcement Pipeline
- **Idempotent operations** — UUID `decision_id` prevents replay
- **WAL crash durability** — WAL-first commit, startup replay restores live blocks into store + XDP, TTL-expired entries skipped, segment retention with oldest-first pruning
- **TTL wheel** — automatic expiry without background scans
- **XDP integration** — targeted insert/remove on BPF hash map, full sync at boot

### Integration & Observability
- **JSON over TCP** line protocol — integrate from nginx/Lua, Go, Python, anything
- **Batch endpoint** — `report_connections` for high-throughput edge proxies
- **HTTP dashboard** — real-time metrics, block feed, subnet heat
- **Structured logging** — `tracing` with `RUST_LOG` filtering
- **Prometheus-compatible** — `Metrics::emit_prometheus()`

</details>

---

## Quick Start

```bash
git clone https://github.com/grep999/ramshield
cd ramshield/beta/rs
cargo build --release -F full

# run (dashboard :9999, IPC :7890)
./target/release/ramshield config.toml
```

```bash
# report traffic from your edge proxy
curl -s http://127.0.0.1:9999/healthz          # {"status":"ok"}

# ask about an IP
echo '{"type":"check_ip","ip":"203.0.113.7"}' | nc -q1 127.0.0.1 7890
# → {"type":"ip_status","ip":"203.0.113.7","blocked":false,"threat":0.0,...}
```

### Drive it with the attack simulator

```bash
python3 scripts/attack_nexus.py profiles list          # 6 attack classes + chain
python3 scripts/attack_nexus.py run --profile l7_http_flood --duration 30
python3 scripts/attack_nexus.py run --profile red_team_full   # 6-phase chain
```

Watch blocks appear live at `http://localhost:9999`.

### CLI

```bash
./target/release/ramshield-cli stats
./target/release/ramshield-cli check 1.2.3.4
./target/release/ramshield-cli block 1.2.3.4 --reason manual --ttl 3600
```

---

## Configuration

```toml
[engine]
shard_count = 256          # DashMap shards (power of 2)
ram_limit_mb = 512         # hard memory ceiling — CapacityExceeded beyond it

[detection]
rps_threshold = 1000       # EWMA RPS block trigger
promote_min_events = 8     # hits before full IpRecord promotion
subnet_window_threshold = 500  # /24 volume for subnet block
batch_max_events = 4096    # batch flush size
batch_window_ms = 50       # max batch wait

[forecasting]
enabled = true
anomaly_zscore = 2.5       # Holt-Winters z-score threshold
min_entropy = 2.0          # Shannon entropy floor (bits)

[ipc]
tcp_addr = "127.0.0.1:7890"
max_connections = 256

[wal]                      # crash-durable block state (off by default)
enabled = true
dir = "/var/lib/ramshield/wal"
durability = "Flush"       # None | Flush | Fsync | GroupCommit (= Fsync today)
compress = true            # LZ4 records > 64 B
seg_max_bytes = 67108864   # 64 MB segment rotation
retention_max_bytes = 536870912  # 512 MB total cap; oldest pruned (0 = ∞)

[xdp]                      # optional kernel acceleration
enabled = false
interface = "eth0"
build_mode = "auto"        # auto | rust | clang | stub
```

Every field is also env-overridable: `RAMSHIELD_ENGINE__RAM_LIMIT_MB=2048`.

**WAL recovery semantics:** every block/unblock is journaled *before* state mutation.
On restart the log is folded in LSN order — live blocks are restored into the store
and re-armed in XDP by reconciliation, unblocks cancel earlier blocks, expired TTLs
are skipped. Verified end-to-end: block → `kill -9` → restart → `restored N live blocks`.

---

## Performance

Measured on a 4-core laptop under `scripts/attack_nexus.py` mixed-profile load:

| Metric | Measured | Notes |
|--------|----------|-------|
| IPC `check_ip` latency | p50 **0.29 ms** · p99 **1.05 ms** | 200-probe sample during sustained flood |
| Sustained ingest | **30–40k events/s** | per driver worker set; 11.4M-event soak, 0 rejections |
| Throughput ceiling | 1M+ events/s | batch path, 8-core |
| Decision latency | < 50 ms | P99 under load |
| Memory | hard-limited | `CapacityExceeded` at `ram_limit_mb`; RSS stable ~315 MB after 11.4M events |
| XDP drop latency | ~100 ns | kernel fast path |
| False positives | < 0.1% | bloom filter + promotion gate |

Attack profiles used: `l7_http_flood`, `volumetric_syn`, `slowloris`,
`dns_amplification`, `botnet_entropy`, `api_abuse` + `red_team_full` chain.

<details>
<summary><b>Robustness verification</b></summary>

- Malformed-input fuzz (empty lines, garbage bytes, unknown types, missing fields,
  invalid IPs): clean typed JSON errors, server never exits
- Oversized lines (> `max_connection_bytes`): connection reset — limit enforced
- 1062 concurrent worker sockets held open for 20+ minutes without fd exhaustion
- Kill -9 mid-write + WAL replay: zero block-state loss
- 91 tests incl. property-based (`proptest`) and WAL recovery roundtrips

</details>

---

## Architecture

```
┌─────────────┐     ┌─────────────────────────────────────────────────┐
│  Edge /     │     │              ramshield daemon                   │
│  Proxies    │────►│                                                 │
│  (nginx,    │     │  ┌──────────┐    ┌──────────────────────┐     │
│   HAProxy,  │     │  │  Engine  │───►│  DetectionEngine     │     │
│   Envoy)    │     │  └────┬─────┘    │  (batch processor)   │     │
└─────────────┘     │       │         └──────────┬───────────┘     │
                    │       ▼                    ▼                 │
                    │  ┌──────────┐    ┌──────────────────────┐     │
                    │  │  Store   │◄───│  Forecaster          │     │
                    │  │ (DashMap)│    │  (Holt-Winters +     │     │
                    │  └──────────┘    │   Entropy)           │     │
                    │       ▲          └──────────┬───────────┘     │
                    │       │                     │                 │
                    │       │    BlockDecision    │                 │
                    │  ┌────┴───────┐             │                 │
                    │  │ Enforcement│             │                 │
                    │  │ (sole      │──► WAL      │                 │
                    │  │  writer)   │    (durable)│                 │
                    │  └────┬───────┘             │                 │
                    │       ▼                     │                 │
                    │  ┌──────────┐    ┌──────────────────────┐     │
                    │  │ XDP Mgr  │───►│  eBPF/XDP Program    │     │
                    │  └──────────┘    │  (kernel space)      │     │
                    └─────────────────────────────────────────────────┘
```

**Design principles:** RAM-first hot path (sharded `DashMap`) · batch-first
evaluation (50 ms / 4096-event windows amortize lock costs) · cold-event bloom
filter before promotion · subnet-scale counters catch distributed floods early ·
single-writer enforcement (all mutations through one actor, WAL-first) · hard
memory ceiling as a first-class constraint.

Workspace layout: `binary glue in src/` + nine domain crates
(`config`, `detection`, `enforcement`, `forecasting`, `metrics`, `protocol`,
`storage`, `types`, `xdp`). Edition 2024, `cargo audit` clean.

<details>
<summary><b>IPC protocol reference</b></summary>

Transport: TCP, one JSON object per `\n`-terminated line.

| Request | Purpose |
|---------|---------|
| `report_connection` | single event |
| `report_connections` | batch events (high throughput) |
| `check_ip` | query block status + threat score |
| `block_ip` / `unblock_ip` | manual control |
| `get_stats` / `get_ip_stats` | engine statistics |

```lua
-- nginx/Lua: accumulate and flush every 50ms
table.insert(batch, {ip=ngx.var.remote_addr, bytes=ngx.var.body_bytes_sent, status_code=ngx.status})
if #batch >= 100 then
    tcp_send("127.0.0.1", 7890, cjson.encode({type="report_connections", events=batch}) .. "\n")
    batch = {}
end
```

</details>

<details>
<summary><b>XDP acceleration details</b></summary>

When `[xdp].enabled = true`, an eBPF XDP program drops packets from blocked IPs
in kernel space — before userspace is ever woken.

Requirements: Linux ≥ 5.10, XDP-capable NIC driver, `CAP_SYS_ADMIN`.
Build pipeline tries three paths in order: aya/Rust (`bpf-linker`) →
clang `-target bpf` → stub ELF (guarantees builds never fail).

```rust
// startup: full blocklist sync from store
xdp.sync_blocklist(&mut blocklist)?;
// runtime: targeted updates only
xdp.apply_block_decision(ip)?;   // BPF map insert
xdp.remove_block(ip)?;           // BPF map remove
```

</details>

---

## Security

- **No TLS / auth on IPC** — localhost or isolated network only, by design
- **Input validation** — every request parsed+validated; invalid IP → typed error
- **Resource limits** — RAM ceiling, connection caps, bounded channel backpressure
- **Audit trail** — every decision carries actor, source, timestamp, policy version

| Threat | Mitigation |
|--------|------------|
| Memory exhaustion | hard RAM limit + promotion filter + capacity Result (no panics) |
| IPC channel flood | bounded queue + backpressure, oversized-line reset |
| Blocklist replay | UUID `decision_id` idempotency |
| Crash state loss | WAL-first journaling + replay |
| Kernel exploit surface | minimal eBPF program, verifier-enforced safety |

---

## Documentation

| Document | Description |
|----------|-------------|
| [Technical Documentation](docs/DOCUMENTATION.md) | architecture, modules, config, integration |
| [API Reference](https://docs.rs/ramshield/latest) | generated Rust docs |
| [Contributing](CONTRIBUTING.md) | workflow, style, testing |
| [Security Policy](SECURITY.md) | vulnerability reporting |
| [Changelog](CHANGELOG.md) | release history |

## Contributing

```bash
cargo fmt -- --check && cargo clippy --all-targets -- -D warnings && cargo test
```

1. Fork → feature branch → keep gates green
2. Conventional commits (`feat:`, `fix:`, `perf:`, `docs:`)
3. Pull request

## License

Dual-licensed **MIT** or **Apache-2.0**, at your option.

---

<p align="center">
  <a href="https://github.com/grep999/ramshield/issues">Issues</a> ·
  <a href="https://github.com/grep999/ramshield/discussions">Discussions</a> ·
  <a href="https://crates.io/crates/ramshield">crates.io</a> ·
  <a href="https://docs.rs/ramshield/latest">docs.rs</a>
</p>
