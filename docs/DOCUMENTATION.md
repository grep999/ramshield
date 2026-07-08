# RamShield — Technical & Functional Documentation

**Version:** 0.2.0 (feature & dashboard release)  
**Language:** Rust 2021  
**Location:** `ramshield/beta/rs`

---

## 1. What RamShield Is

RamShield is an **in-memory DDoS detection and mitigation engine**. It sits beside your network edge (proxy, load balancer, firewall tap) and:

- Receives reports about incoming connections
- Tracks traffic at **IP** and **/24 subnet** scale
- Scores threat levels in real time
- Blocks abusive sources automatically or on operator command

It is **not** a general database, not a full firewall, and not a packet capture tool. It is a **specialized RAM-first engine** built for high connection rates.

### What problems it solves


| Problem                         | RamShield approach                          |
| ------------------------------- | ------------------------------------------- |
| Single IP flooding              | EWMA rate tracking + automatic block        |
| Distributed attack (many IPs)   | Subnet counters + entropy analysis          |
| Attack ramp-up before threshold | Holt-Winters forecasting + preemptive block |
| Memory exhaustion under attack  | Promotion filter + RAM budget               |
| IPC overhead at high volume     | Batch ingest path                           |


---

## 2. Binaries


| Binary          | Role                                                     |
| --------------- | -------------------------------------------------------- |
| `ramshield`     | Long-running daemon — detection, forecasting, IPC server |
| `ramshield-cli` | Operator tool — check, block, unblock, stats, info       |


### Build & run

```bash
cargo build --release
./target/release/ramshield config.toml
./target/release/ramshield-cli stats
```

If `CARGO_TARGET_DIR` is set in your shell, unset it or build explicitly into `./target`:

```bash
unset CARGO_TARGET_DIR && cargo build --release
```

---

## 3. Architecture Overview

```
                    ┌─────────────────────────────────────────┐
  Edge / scripts    │           ramshield daemon              │
  ───────────────►  │                                         │
  TCP JSON IPC      │  ┌─────────┐    ┌──────────────────┐   │
  (port 7890)       │  │ Engine  │───►│ DetectionEngine  │   │
                    │  └────┬────┘    │  (batch thread)  │   │
                    │       │         └────────┬─────────┘   │
                    │       │                  │             │
                    │       ▼                  ▼             │
                    │  ┌─────────┐    ┌──────────────────┐   │
                    │  │ Store   │◄───│ Forecaster       │   │
                    │  │ DashMap │    │ (Tokio timers)   │   │
                    │  └─────────┘    └──────────────────┘   │
                    │       ▲                  ▲             │
                    │       │ BlockDecision    │             │
                    │  ┌────┴────┐      ┌──────┴─────┐       │
                    │  │ Block   │      │ Alerting   │       │
                    │  │ applier │      │ Engine     │       │
                    │  └─────────┘      └──────┬─────┘       │
                    │                          │             │
                    │                  ┌───────▼───────┐     │
                    │                  │ Dashboard HTTP│     │
                    │                  │ (port 9999)   │     │
                    │                  └───────────────┘     │
                    └─────────────────────────────────────────┘
```

### Runtime components

1. **IPC server** (Tokio) — accepts TCP connections, one JSON request per line
2. **Event channel** (crossbeam, capacity 2M) — decouples ingest from detection
3. **Batch processor** (dedicated OS thread) — aggregates events before analysis
4. **Subnet batch loop** (Tokio, every 500 ms) — blocks hot /24 prefixes
5. **Forecaster** (Tokio, 1 s / 5 s timers) — anomaly and entropy checks
6. **Block applier** (Tokio) — writes block state into the store

---

## 4. End-to-End Data Flow

### Ingest path

```
Client sends JSON line
    → Engine parses Request
    → report_connection / report_connections
    → ConnectionEvent pushed to channel (try_send)
    → Batch thread collects events (up to 4096 or 50 ms)
    → aggregate() in memory (HashMap by IpAddr, subnet counts)
    → Promotion filter decides which IPs get full tracking
    → merge_record() loads existing IpRecord, merges batch stats, writes once
    → BlockDecision broadcast if threshold exceeded
    → Block applier updates IpRecord.block_state
```

### Why batching is the default path

Under attack, millions of packets arrive per second. Processing each packet with separate map lookups and string allocations does not scale.

RamShield instead:

1. **Buffers** events for a short window (50 ms default)
2. **Counts** per IP in memory (cheap)
3. **Promotes** only IPs that matter (≥ 8 hits, hot subnet, or bloom hit)
4. **Merges** into existing records instead of rebuilding from scratch

This is the same idea used in high-throughput log pipelines (Kafka batch consumers, LMAX Disruptor): **batch first, store second**.

---

## 5. Module Reference

### 5.1 `engine/`

**Role:** Orchestrator. Wires all subsystems at startup and dispatches IPC requests.

**Key responsibilities:**
- Create `Store`, `DetectionEngine`, `Metrics`, and `Cache`
- Spawn block applier, forecaster, IPC listener, and cache manager
- Handle admin requests: check, block, unblock, stats, config reload

**Design choice:** Engine stays thin. Business logic lives in detection, storage, and forecasting.
### 5.2 `detection/`

**Role:** Core traffic analysis and blocking decisions.

#### `batch.rs`

- `aggregate()` — one pass over events → `HashMap<IpAddr, IpAgg>` + subnet counts
- `subnet_key_v4()` — packs /24 into `u32` (no string keys for subnets)
- `ip_in_subnet()` — correct octet comparison for batch blocking

#### `mod.rs`

- **Bloom filter** — fast “probably seen before” check keyed by `IpAddr` hash
- **Batch processor loop** — blocking `recv_timeout`, not Tokio spin
- **flush_batch()** — promotion + merge + block emission
- **subnet_batch_loop()** — reads `subnet_table`, blocks matching IPs

#### `rate_tracker.rs`

- EWMA helper (`ALPHA = 0.3`) shared by detection

#### Promotion rules (who gets a full `IpRecord`)

An IP is **promoted** (stored and analyzed) if **any** of:


| Condition                         | Default             | Purpose                                |
| --------------------------------- | ------------------- | -------------------------------------- |
| `agg.count >= promote_min_events` | 8                   | Ignore one-off scans                   |
| Subnet hot in window              | ≥ 500 events on /24 | Drill down when subnet is under attack |
| Bloom filter hit                  | —                   | Keep tracking known bad IPs            |


Cold IPs are counted in the batch only and **never touch the store**.

---

### 5.3 `storage/`

**Role:** In-memory typed key-value store with RAM accounting.

#### Value types


| Type                                 | Use                                             |
| ------------------------------------ | ----------------------------------------------- |
| `IpRecord`                           | Full per-IP metadata (primary detection object) |
| `SubnetRecord`                       | /24 aggregate counters                          |
| `Counter`, `Float`, `Inline`, `Blob` | Generic / future use                            |


#### `IpRecord` fields


| Field            | Meaning                                         |
| ---------------- | ----------------------------------------------- |
| `request_count`  | Total requests seen                             |
| `ewma_rps`       | Smoothed requests-per-second                    |
| `bytes_in`       | Total bytes reported                            |
| `status_dist[5]` | HTTP status buckets (1xx–5xx)                   |
| `threat_score`   | 0.0–1.0 composite score                         |
| `block_state`    | Clean, Suspicious, or Blocked { reason, since } |


#### `TrafficCounters`

Updated once per batch flush. Used by forecasting **without scanning the entire store**:

- `events_last_second` — global volume for Holt-Winters
- `unique_ips_window` — distinct IPs in last flush
- `subnet_window` — per-/24 counts for entropy
- `threat_sample` — top 128 high-threat IPs for preemptive block

#### RAM limit

Every `insert()` checks `ram_limit_bytes`. Detection uses the configured limit (not unlimited). When full, insert returns `CapacityExceeded`.

#### Modules present but not wired at startup


| Module          | Status                                                                        |
| --------------- | ----------------------------------------------------------------------------- |
| `ttl_wheel.rs`  | Implemented, **not started** by Engine — TTL expiry is lazy (on `get()` only) |
| `wal.rs`        | Implemented, **not used** unless manually integrated                          |
| `blob_store.rs` | Implemented for large payloads, **not on hot path**                           |


Config keys exist for TTL/WAL for forward compatibility.

---

### 5.4 `forecasting/`

**Role:** Detect attack patterns before or alongside rate thresholds.


| Timer          | Method               | What it does                                                       |
| -------------- | -------------------- | ------------------------------------------------------------------ |
| 1 s            | `tick_hw()`          | Holt-Winters on global event rate; z-score anomaly                 |
| 5 s            | `tick_entropy()`     | Shannon entropy on subnet distribution — low entropy = botnet-like |
| On anomaly     | `preemptive_block()` | Blocks IPs from `threat_sample` with score > 0.7                   |
| On low entropy | `entropy_block()`    | Blocks top 10% by request count (max 50)                           |


**Design choice:** Forecasting reads **incremental counters**, not the full map. This keeps cost stable as IP count grows.

**Block reasons from forecasting:**

- `ForecastAnomaly`
- `EntropyAnomaly`

---

### 5.5 `ipc/`

**Role:** JSON protocol over TCP. One request per line, one response per line.

**Design choice:** JSON over TCP is simple to integrate from any language (Python scripts, nginx lua, Go sidecars). A binary protocol could be added later; the batch struct maps cleanly.

See [Section 7 — IPC Protocol](#7-ipc-protocol).

---

### 5.6 `config/`

**Role:** TOML configuration with sensible defaults. Loaded at startup from file path argument or defaults.

Uses `arc_swap::ArcSwap` for `ConfigHandle` — structure supports hot reload in future; not actively reloaded today.

---

### 5.7 `metrics/`

**Role:** System-wide counters, HTTP dashboard snapshots, and system resource monitoring.

**Key types:**
- `Metrics` — Atomically safe counters for events, blocks, batches, promotions, cold skips, forecasting ticks, entropy values. All load/store via `Ordering::Relaxed`.
- `DashboardSnapshot` — Composite snapshot of health, KPI, pipeline, and system resource usage aggregated for the `/api/stats` endpoint and SSE stream.
- `BatchRecord` — Event/block count for one batch flush; retained in a rolling ring of 80 entries.
- `BlockRecord` — Timestamp, IP, reason, and triggering module; retained for 40 entries.
- `SubnetRow` — Prefix and event count for display in the hot-subnets table.
- `PipelineFlow` — End-to-end counts: ingest → queued → batched → promoted → merged → blocked.

**System metrics integration:**
`get_system_usage()` uses `sysinfo` to query real CPU percentage and total system memory (MB) on every snapshot build. These populate `cpu_usage` and `memory_usage_mb` in `DashboardSnapshot`.

**Key storage:** Floating-point values (HW RPS, z-score, forecast, entropy) are serialised into `AtomicU64` via `to_bits()/from_bits()` to avoid mutex contention on the snapshot path.

---

### 5.8 `dashboard/` (New)

**Role:** Real-time web UI for monitoring RamShield health, metrics, and events. Runs an Axum HTTP server.

**Key responsibilities:**
- Serves static HTML/JS/CSS assets (embedded via `include_str!`)
- Provides JSON API for stats, metrics, event history
- Streams real-time `DashboardSnapshot` data via Server-Sent Events (SSE)
- Enables hot-reloading of configuration via HTTP POST

### 5.9 `alerting/` (New)

**Role:** Enterprise alerting and audit trail module. Monitors system metrics against configurable thresholds and generates alerts.

**Key responsibilities:**
- Detects high RPS, low entropy, and high block rates
- Manages alert cooldowns to prevent spam
- Logs all alerts to an audit file for compliance (SOC2/GDPR)
- Provides an in-memory alert history for dashboard display

## 5.10 `learning/` (New)

**Role:** Builds behavioral baselines from historical traffic.

**Key responsibilities:**
- Analyze traffic patterns to identify normal behavior
- Store learned patterns for future use by prediction engine

## 5.11 `prediction/` (New)

**Role:** Anticipates attack vectors using learned patterns.

**Key responsibilities:**
- Forecast future traffic based on baselines
- Proactively identify potential threats before thresholds are met

## 5.12 `cache.rs` (New)

**Role:** In-memory caching layer with TTL support.

**Key responsibilities:**
- Store frequently accessed data for fast retrieval
- Reduce load on the main store by serving cached entries

---

## 6. Blocking Logic

### Automatic blocks


| Trigger                    | Reason enum       | Typical TTL                     |
| -------------------------- | ----------------- | ------------------------------- |
| EWMA RPS > threshold       | `HighRps(n)`      | `block_ttl_secs` (default 3600) |
| Hot /24 subnet             | `SubnetBatch`     | 3600                            |
| Holt-Winters z-score > 3.5 | `ForecastAnomaly` | 300 s                           |
| Low Shannon entropy        | `EntropyAnomaly`  | 600 s                           |


### Manual blocks

Via IPC or CLI → `ManualBlock`

### Threat score formula

```
rps_score    = min(ewma_rps / rps_threshold, 1.0)
err_frac     = status_5xx_count / total_requests
threat_score = rps_score * 0.7 + err_frac * 0.3
```

**Why EWMA?** Raw instant RPS spikes on every burst. EWMA smooths noise while still reacting to sustained abuse — standard practice in rate limiters and CDN edge logic.

**Why 70/30 RPS vs errors?** Volume alone can be legitimate (API clients). High 5xx rates suggest scanning or exploit attempts. The blend catches both floods and crawlers.

---

## 7. IPC Protocol

**Transport:** TCP  
**Default address:** `127.0.0.1:7890`  
**Framing:** One JSON object per line (`\n` terminated)

All request `type` values use **snake_case**.

### Requests

#### `report_connection` (single event — backward compatible)

```json
{"type":"report_connection","ip":"1.2.3.4","bytes":512,"status_code":200,"proto_fp":0}
```

Response: `{"type":"ok","message":"accepted"}` or error.

#### `report_connections` (batch — high throughput)

```json
{
  "type": "report_connections",
  "events": [
    {"ip": "1.2.3.4", "bytes": 512, "status_code": 200, "proto_fp": 0},
    {"ip": "1.2.3.5", "bytes": 256, "status_code": 404, "proto_fp": 1}
  ]
}
```

Response: `{"type":"batch_ok","accepted":2,"rejected":0}`

#### `check_ip`

```json
{"type":"check_ip","ip":"1.2.3.4"}
```

Response: `{"type":"ip_status","ip":"...","blocked":true,"threat":0.9,"ewma_rps":1200.0,"reason":"high_rps:1200"}`

#### `block_ip`

```json
{"type":"block_ip","ip":"1.2.3.4","reason":"manual","ttl_secs":3600}
```

#### `unblock_ip`

```json
{"type":"unblock_ip","ip":"1.2.3.4"}
```

#### `get_stats`

```json
{"type":"get_stats"}
```

#### `get_ip_stats`

```json
{"type":"get_ip_stats","ip":"1.2.3.4"}
```

#### `flush`

```json
{"type":"flush"}
```

### Error responses

```json
{"type":"error","code":503,"message":"channel full"}
```


| Code | Meaning                           |
| ---- | --------------------------------- |
| 400  | Bad request / invalid IP          |
| 404  | IP not found                      |
| 503  | Event channel full — backpressure |


---

## 8. CLI Reference

```bash
ramshield-cli [--addr 127.0.0.1:7890] <command>
```


| Command                         | Action                       |
| ------------------------------- | ---------------------------- |
| `check <ip>`                    | Is IP blocked? Threat score? |
| `block <ip> [--reason] [--ttl]` | Manual block                 |
| `unblock <ip>`                  | Clear block                  |
| `stats`                         | Engine statistics            |
| `info <ip>`                     | Detailed IP record           |


---

## 9. Configuration Reference

File: `config.toml`. Missing sections use code defaults.

### `[engine]`


| Key              | Default         | Description                             |
| ---------------- | --------------- | --------------------------------------- |
| `shard_count`    | 256             | DashMap shard count (power of 2)        |
| `worker_threads` | 0 (= CPU count) | Logged; batch uses one dedicated thread |
| `ram_limit_mb`   | 512             | Max in-memory store size                |


### `[detection]`


| Key                       | Default | Description                                   |
| ------------------------- | ------- | --------------------------------------------- |
| `rps_threshold`           | 1000    | EWMA RPS block threshold                      |
| `rate_window_secs`        | 10      | Sliding window for count decay                |
| `subnet_batch_threshold`  | 5       | /24 events per window to trigger subnet block |
| `batch_block_enabled`     | true    | Enable subnet batch blocking                  |
| `block_ttl_secs`          | 3600    | Auto-block TTL                                |
| `bloom_bits`              | 8000000 | Bloom filter size in bits                     |
| `batch_max_events`        | 4096    | Max events per flush batch                    |
| `batch_window_ms`         | 50      | Max wait before partial flush                 |
| `promote_min_events`      | 8       | Hits before per-IP store entry                |
| `subnet_window_threshold` | 500     | /24 volume to treat subnet as hot             |


### `[dashboard]`

| Key | Default | Description |
|| --- | ------- | ----------- |
| `enabled` | true | Enable HTTP dashboard server |
| `http_addr` | 127.0.0.1:9999 | Listen address |

### `[alerting]`

| Key | Default | Description |
|| --- | ------- | ----------- |
| `enabled` | true | Enable alerting engine |
| `rps_alert_threshold` | 5000 | EWMA RPS threshold for High alert |
| `entropy_alert_threshold` | 0.8 | Entropy threshold for Warning alert |
| `alert_cooldown_secs` | 60 | Min seconds between same-source alerts |
| `audit_log_enabled` | true | Write alerts to audit log |
| `audit_log_path` | ./audit.log | Path for audit log file |

### `[forecasting]`


| Key                  | Default | Description                    |
| -------------------- | ------- | ------------------------------ |
| `enabled`            | true    | Run forecaster                 |
| `ewma_alpha`         | 0.3     | Holt-Winters level smoothing   |
| `hw_beta`            | 0.1     | Trend smoothing                |
| `hw_gamma`           | 0.1     | Seasonality smoothing          |
| `seasonality_period` | 3600    | Seasonality cycle (seconds)    |
| `anomaly_zscore`     | 2.5     | Z-score alert threshold        |
| `min_entropy`        | 2.0     | Minimum Shannon entropy (bits) |


### `[storage]`


| Key                 | Default | Description             |
| ------------------- | ------- | ----------------------- |
| `wal_enabled`       | false   | WAL not wired in Engine |
| `wal_path`          | ./wal   | WAL directory           |
| `wal_sync`          | none    | sync mode if WAL used   |
| `wal_segment_bytes` | 64 MiB  | Segment size            |
| `wal_compress`      | true    | LZ4 compression         |


---

## 10. Design Decisions & Advantages

### 10.1 RAM-first, not disk-first

**Choice:** All hot path data lives in a sharded in-memory map.

**Why:** DDoS decisions must be sub-millisecond. Disk I/O on every connection report would cap throughput far below attack volumes.

**Advantage:** Predictable latency at high QPS. RAM budget gives a hard ceiling on memory use.

---

### 10.2 Batch detection instead of per-event analysis

**Choice:** Dedicated thread accumulates 50 ms / 4096 events, then flushes.

**Why:** Each store operation involves hashing, locking a shard, and possibly allocating. Batching amortizes this over hundreds or thousands of events.

**Advantage:** Throughput scales with batch size. IPC batch API (`report_connections`) matches this on the wire.

---

### 10.3 Promotion filter (cold IP skipping)

**Choice:** IPs with fewer than 8 hits per window are not stored.

**Why:** Random internet background noise creates millions of unique IPs. Storing each one would waste RAM and CPU on harmless traffic.

**Advantage:** Memory and CPU proportional to **active threats**, not **total unique sources**.

---

### 10.4 Subnet-scale diagnosis before IP-scale

**Choice:** /24 counters updated on every batch; subnet batch block runs on `subnet_table`.

**Why:** Many attacks distribute across a subnet (compromised hosting, botnet on same provider). Detecting at /24 catches coordinated abuse with fewer data structures than per-IP analysis of every source.

**Advantage:** Faster response to distributed floods within one prefix. Entropy check on subnet distribution detects botnet uniformity.

---

### 10.5 Reuse existing IpRecord on merge

**Choice:** `merge_record()` loads the current record, adds batch aggregates, writes back once.

**Why:** Rebuilding state from scratch each event duplicates work and loses EWMA history.

**Advantage:** EWMA and threat scores stay stable and meaningful across batches. Fewer allocations.

---

### 10.6 Bloom filter pre-screen

**Choice:** In-memory probabilistic set keyed by IP hash.

**Why:** Once an IP is blocked or flagged, you want to keep evaluating it even at low volume without storing every cold IP.

**Advantage:** O(1) check, no false negatives (may have false positives — acceptable for “promote to tracking”).

---

### 10.7 Crossbeam channel + dedicated thread for ingest

**Choice:** 2M bounded channel; batch thread uses blocking recv.

**Why:** Tokio tasks spinning on `try_recv()` waste CPU. Blocking recv on a native thread matches the producer–consumer pattern used in high-performance servers.

**Advantage:** Clean backpressure (503 when full). No scheduler contention on the ingest hot path.

---

### 10.8 Incremental TrafficCounters for forecasting

**Choice:** Counters updated at flush time; forecaster reads atomics.

**Why:** Scanning millions of keys every second does not scale.

**Advantage:** Forecasting cost is O(1) per tick regardless of store size.

---

### 10.9 JSON IPC with batch extension

**Choice:** Keep `report_connection`; add `report_connections`.

**Why:** Compatibility with simple integrations; batch for load generators and edge proxies that can buffer.

**Advantage:** Existing clients keep working. New clients get 10–100× fewer syscalls.

---

### 10.10 Typed values, not string maps

**Choice:** `Value` enum with `IpRecord`, counters, blobs — not `map[string]string`.

**Why:** Parsing on every read is expensive and error-prone. DDoS metadata has a fixed shape.

**Advantage:** Zero parse overhead on read. Compiler enforces structure.

---

## 11. Integration Pattern

Typical deployment:

```
Internet → Nginx/HAProxy → (access log or module)
                              │
                              ▼ report_connections batches
                         RamShield :7890
                              │
                              ▼ check_ip before proxy_pass
                         Block or allow
```

RamShield does **not** drop packets itself. It **advises** your edge (or you block at firewall based on CLI/API output).

---

## 12. Load Testing Scripts


| Script                       | Purpose                               |
| ---------------------------- | ------------------------------------- |
| `scripts/attack_sim_100k.py` | Fixed 100k event burst                |
| `scripts/attack_extreme.py`  | Burst, flood, phase, interactive REPL |


Examples:

```bash
./target/release/ramshield config.toml

python3 scripts/attack_extreme.py burst --events 500000 --workers 256
python3 scripts/attack_extreme.py flood --duration 60 --mode volumetric
python3 scripts/attack_extreme.py interactive
```

---

## 13. Observability

### Logs

Set `RUST_LOG` for verbosity:

```bash
RUST_LOG=ramshield=debug ./target/release/ramshield config.toml
```

Default filter: `ramshield=info`

### Key log messages

| Message                                                   | Meaning                  |
| --------------------------------------------------------- | ------------------------ |
| `Detection: batch processor (max N events / M ms window)` | Batch thread started     |
| `Engine started (N workers, IPC addr)`                    | Daemon ready             |
| `Batch block /24 x.y.z`                                   | Subnet block triggered   |
| `ANOMALY z=...`                                           | Forecasting alert        |
| `LOW ENTROPY H=...`                                       | Botnet-like distribution |
| `AlertingEngine: ALERT [CRITICAL] ...`                    | Alert triggered          |
| `Dashboard: Starting server on 127.0.0.1:9999`            | HTTP dashboard ready     |

### Dashboard

The built-in HTTP dashboard provides real-time visibility at `http://127.0.0.1:9999` (configurable via `[dashboard].http_addr`).

**Features:**
- KPI cards with live RAM/CPU/throughput
- Batch history chart (last 80 flushes)
- Recent blocks table with reason/module
- Hot subnets ranking
- Pipeline flow diagram
- Module-level stats (IPC, Detection, Forecasting, Storage)
- Config editor with hot-reload via POST `/api/config`
- Server-Sent Events (SSE) stream at `/sse` (5s interval)

**SSE client example:**
```javascript
const es = new EventSource('/sse');
es.onmessage = (e) => { updateUI(JSON.parse(e.data)); };
```

### Alerting

When `[alerting].enabled = true`, the `AlertingEngine` monitors:

| Source         | Condition                              | Severity   |
| -------------- | -------------------------------------- | ---------- |
| `high_rps`     | EWMA RPS > `rps_alert_threshold`       | HIGH       |
| `high_entropy` | Shannon entropy > `entropy_alert_threshold` | WARNING  |
| `high_block_rate` | Block rate > 90% with 100+ events   | CRITICAL   |

Alerts are cooldown-limited per source (default 60s). Each alert is appended to the audit log (`./audit.log` by default) in format:
```
[timestamp_ms] SEVERITY source message metrics_json
```

Recent alerts are also exposed via the dashboard API.

### Stats API

`get_stats` returns: `ips_tracked`, `blocked`, `ram_bytes`, `ram_limit_mb`, `uptime_secs`, `evictions`

---

## 14. Known Limitations (Current Version)

1. **TTL wheel not started** — expired entries only disappear on read; no background eviction
2. **WAL not connected** — blocks are not persisted across restart
3. **IPv6 subnets** — subnet key logic is IPv4-only
4. **Single batch thread** — `worker_threads` config is informational
5. **No TLS on IPC** — intended for localhost or trusted network
6. **Subnet batch block** may affect innocent IPs in same /24 (by design for aggressive mitigation)
7. **Alerting cooldown granularity** — cooldown is per-alert-type, not per-IP
8. **Dashboard persistence** — UI state not preserved across refreshes (planned for WAL integration)

*Note: Entropy block now uses the bounded threat sample for true O(1) forecasting cost.*

---

## 15. File Map

```src/
├── main.rs              Daemon entry
├── cli.rs               CLI binary
├── lib.rs               Module exports
├── config.rs            TOML config
├── error.rs             Error types
├── engine/mod.rs        Orchestrator + IPC server
├── detection/
│   ├── mod.rs           Batch detection engine
│   ├── batch.rs         In-memory aggregation
│   └── rate_tracker.rs  EWMA helpers
├── storage/
│   ├── mod.rs           Store, IpRecord, TrafficCounters
│   ├── ttl_wheel.rs     (not wired)
│   ├── wal.rs           (not wired)
│   └── blob_store.rs    (not wired)
├── forecasting/
│   └── mod.rs           Holt-Winters + entropy
├── ipc/
│   └── mod.rs           Request/Response types
├── metrics/
│   └── mod.rs           Counters, DashboardSnapshot, sysinfo integration
├── alerting/
│   └── mod.rs           AlertingEngine, alert types, audit logging
├── learning/
│   └── mod.rs           Traffic pattern analysis
├── prediction/
│   └── mod.rs           Future threat forecasting
├── cache.rs             In-memory caching layer with TTL support
├── dashboard/
│   ├── mod.rs           Axum HTTP server, SSE, config reload
│   └── static/          Embedded UI assets (HTML/JS/CSS)
│
└── scripts/
    ├── attack_sim_100k.py
    └── attack_extreme.py
```
```

---

## 16. Summary

RamShield v0.2.0 is a **batch-oriented, RAM-bounded DDoS detection engine** that:

- Ingests connection reports over JSON/TCP
- Aggregates traffic in time windows before touching shared state
- Promotes only hot IPs and hot subnets to full tracking
- Blocks on rate, subnet volume, forecast anomaly, and entropy collapse
- Exposes a compatible single-event API and a high-throughput batch API
- Provides real-time monitoring via HTTP dashboard with SSE updates
- Generates enterprise-grade alerts with audit trail for SOC2/GDPR compliance
- Implements behavioral baselines and predictive threat modeling
- Implements caching layer to reduce load under sustained traffic

The design prioritizes **throughput, memory safety, and subnet-scale visibility** over per-packet granularity — the right tradeoff when the adversary sends millions of events and you must decide in milliseconds which sources matter.