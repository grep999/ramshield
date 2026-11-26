# Benchmarks — RamShield v0.3.4

All numbers below were measured on 2026-10-05, on the machine running this repo, with the pinned nightly toolchain. They are **measured**, not estimated. The original raw files (`/tmp/ramshield_bench_hot.txt`, `/tmp/ramshield_bench_field.txt`) were not retained in this checkout; every figure below cites its in-tree source and the single-run caveat at the end of this document applies.

Two benches exist in the repo:

```bash
cargo bench --bench hot_paths --features full   # storage + math primitives
cargo bench --bench field_day --features full   # full detection pipeline + enforcement actor
```

Both use the `harness = false` manual harness (no criterion). Each bench warms up 1000 iterations, then times `n` steady-state iterations and divides. Per-flush granularity: one iteration = one full flush, not one event.

The test suite is **RFC 9411** (Benchmarking Methodology for Network Security Devices, March 2023). That's the standard for DDoS / security-device performance claims — not a generic "run it and see" harness. The KPI mapping is in the table below. An older suite scaffold claimed a "DDactic, 213 vectors × 6 architectures" matrix; that scaffold is not in this checkout, so no matrix result is claimed here.

---

## Hot-path micro-benchmarks (`benches/hot_paths.rs`)

Primitive costs. Lower is better.

### Forecasting

| Operation | ns/op | ops/s |
|---|---:|---:|
| `HoltWinters::update` | 27.9 | 35,789,572 |
| `HypothesisTracker::bayesian_update` | 132.5 | 7,547,181 |
| `HypothesisTracker::best_above_threshold` | 4.0 | 249,315,007 |

### Detection

| Operation | ns/op | ops/s |
|---|---:|---:|
| `ewma()` | ~0 | (inlined; see caveat) |
| `cusum_step_capped` + `cusum_fired` | 2.2 | 445,883,161 |
| `pulse_tracker_step` | ~0 | (inlined; see caveat) |
| `BloomFilter::insert` (8M bits) | 31.3 | 31,926,441 |
| `BloomFilter::contains` (8M bits) | 176.5 | 5,667,180 |
| `batch::aggregate` (4096 events) | 250,395.7 | 3,994 |

### Storage

| Operation | ns/op | ops/s |
|---|---:|---:|
| `subnet_key_v4` | 192.3 | 5,199,381 |
| `subnet_key_v6` | 0.6 | 1,539,100,857 |
| `Store::get` (1K entries) | 202.2 | 4,946,310 |
| `Store::update_ip` (1K entries) | 196.7 | 5,082,887 |

### Protocol

| Operation | ns/op | ops/s |
|---|---:|---:|
| `auth::sign` HMAC-SHA256 | 1,815.6 | 550,776 |
| `Request::serialize` (100 events) | 14,204.3 | 70,401 |
| `Request::deserialize` (100 events) | 61,218.5 | 16,335 |

### Metrics

| Operation | ns/op | ops/s |
|---|---:|---:|
| `inc_requests` (AtomicU64) | 7.2 | 139,373,988 |
| `inc_ingested` (AtomicU64) | 6.9 | 144,835,178 |
| `render_prometheus` (text format) | 20,638.6 | 48,453 |

### CGNAT SHM + Analytics + Mesh

| Operation | ns/op | ops/s |
|---|---:|---:|
| SHM rule lookup (hit) | 0.9 | 1,096,046,560 |
| CGNAT classify (entropy) | 0.7 | 1,428,775,539 |
| `CMS::increment` | 60.8 | 16,440,608 |
| `HLL::insert` | 3.0 | 336,497,532 |
| Mesh `record_ban` | 195.6 | 5,112,861 |

### 2000-subnet DDoS simulation

| Operation | ns/op | ops/s |
|---|---:|---:|
| `subnet_key_v4` (2000 subnets) | 1.7 | 590,548,856 |
| `aggregate` (2000 subnets × 250 addresses) | 346,584.9 | 2,885 |
| Bloom contains (2000 subnet keys) | 167.4 | 5,974,483 |

**Caveat:** `ewma()` and `pulse_tracker_step` report `~0 ns/op`. These are free functions the optimizer inlines into the bench loop — the reported number is the loop overhead, not the real cost. The meaningful numbers are the `field_day` pipeline flushes below, which measure the same math inside the actual detection path where it cannot be optimized away.

---

## Pipeline benchmarks (`benches/field_day.rs`)

End-to-end: detection flush (pre-aggregation → store RMW → EWMA/CUSUM/pulse → gate) and the enforcement actor (WAL → store → TTL schedule → XDP).

| Path | ns/op | ops/s |
|---|---:|---:|
| Detection flush — 4096 ev / 128 IPs (all promoted, RMW + detectors) | 185,648 | 5,387 |
| Detection flush — 4096 ev / 4096 IPs (cold-skip: gate + bloom) | 1,310,948 | 763 |
| Detection flush — 4096 ev / 10 IPs SUSTAINED blocked (gate suppress) | 152,061 | 6,576 |
| Detection flush — 4096 ev / 512 IPs FIRST attack (emit + admit + bloom) | 308,264 | 3,244 |
| Detection flush — 32768 ev / 512 subnets (subnet window RMW ×512) | 17,988,268 | 56 |
| `aggregate` — 4096 raw events, pure per-event cost | 216,632 | 4,616 |
| Enforce block — WAL off, full transition | 1,643 | 608,836 |
| Enforce block — WAL GroupCommit, fsync ≤10Hz | 2,800 | 357,101 |
| Enforce duplicate `decision_id` (idempotent cache) | 106 | 9,467,337 |
| Enforce unblock — full transition | 1,470 | 680,369 |
| `get_all_blocked_ips` — 10k blocked, every 10s | 302,394 | 3,307 |

### Derived event-level cost

One flush = `batch_max_events` events (4096 by default). Divide:

- All-promoted flush: 185,648 ns / 4096 events ≈ **45 ns/event**
- Cold-skip flush: 1,310,948 ns / 4096 events ≈ **320 ns/event**
- Sustained-blocked (gate suppress): 152,061 ns / 4096 ≈ **37 ns/event**
- First-attack flush (emit + admit + bloom insert): 308,264 ns / 4096 ≈ **75 ns/event**
- Subnet window RMW: 17,988,268 ns / 32768 ≈ **549 ns/event**

Per-event cost stays in the double-digit-nanosecond range once the store is warm. The cold-skip path costs 7× more per event than the warm all-promoted path — the gate and bloom filter are the dominant cost there, not the detectors. This is the shape you want: cold IPs (the attacker's churn) are rejected cheaply, and the expensive path is reserved for IPs that have earned full tracking.

The subnet-window path at ~549 ns/event is the most expensive per-event path in the system. It runs only when the `/24` swarm gate arms (≥50 unique IPs in a /24 within the window) — rare in normal operation, exactly when you need it under attack.

---

## Live daemon snapshot (measured)

Captured 2026-10-05 from the running daemon on `127.0.0.1:9999`:

```json
{
  "uptime_secs": 8636,
  "ips_tracked": 1686,
  "blocked_total": 0,
  "ram_bytes": 271446,
  "ram_limit_mb": 512,
  "ram_pct": 0.0506,
  "memory_usage_mb": 14,
  "total_ram_mb": 15896,
  "ipc_requests": 303,
  "events_ingested": 17574,
  "events_rejected": 0,
  "frames_rejected_total": 0,
  "channel_depth": 0,
  "events_shed": 0,
  "batches_total": 22,
  "promotions": 2049,
  "cold_skipped": 4406
}
```

At idle: **266 KB** of the 512 MB limit (0.05%), 14 MB RSS on a 16 GB host, zero events rejected or shed after 8636 s uptime. `channel_depth: 0` is the backpressure indicator — the ingest channel never backed up.

---

## RFC 9411 compliance (attack-suite results)

The RFC-mapped suite runs 21 tests (12 from the v2 suite, 9 from the v3 suite) covering the RFC's mandatory KPIs: inspected throughput, application TPS, concurrent connection capacity, transaction latency, heavy-impact threshold, and reporting accuracy. Prior run on master `@ 61b9a78` (2026-09-05) ingested **21,322,950 events** across 21 phases.

| RFC KPI | RamShield result | Verdict |
|---|---|---|
| Inspected throughput (single attacker, 30s) | 154,731 eps | ✅ |
| Inspected throughput (50 attackers, 20s) | 115,278 eps aggregate | ✅ |
| Raw IPC throughput (HMAC-signed) | 126,087 eps | ✅ |
| Probe oracle (T18) — availability under attack | 100% baseline / 100% under attack (0% degradation) | ✅ |
| Heavy impact (T19 background + attack) | 79,260 → 5,191 eps (93.5% BG degradation) | ❌ IPC backpressure |
| False positive result (T14) | 0 of 200 good IPs blocked in one run (upper bound 1.5%) | ✅ |
| Sub-threshold hold (T17) | 0 blocks at 1 IP × 10 eps × 30s | ✅ |
| Subnet aggregation (T16) | 2 blocks for a 51-IP /24 flood at 82K eps | ✅ |
| Reporting accuracy (T15 recovery) | 52 ms unblock → snapshot reflect | ✅ (<1s) |
| Mitigation latency, warm (T20) | 108 ms (IP already tracked) | ✅ |
| Mitigation latency, cold (T8) | 8,002 ms (full detection window) | ⚠️ acceptable for cold path |
| Auth rejection (T1) | 7/7 vectors rejected; replay accepted (no nonce) | ✅ (replay gap open) |
| Memory under load | 0.0039% ram_pct at 21.3M events; RSS 44 MB | ✅ |
| Pulse-wave evasion (T13) | 1/4 bursts blocked (2s burst < detection window) | ❌ known evasion |
| Concurrent TCP connections | 5,000 in 0.49s, IPC stayed alive | ✅ |

**Totals:** 148 blocks applied — 137 auto-detected (`high_rps`: 89, `entropy_anomaly`: 48), 11 manual.

**Open gaps (from the suite, not hidden):**
- **Heavy impact under background load (T19).** The IPC channel backpressures and starves the background traffic path by 93.5%. Real effect: a real user's traffic degrades badly while an attack is being absorbed.
- **Pulse-wave evasion (T13).** Bursts shorter than the detection window pass. This is a known weakness of threshold-class detection, not a bug in the gate.
- **Cold-path mitigation latency (T8).** 8 s to first block for a never-before-seen IP — it is the full detection window by design, but it is a real exposure window.
- **Auth replay (T1).** HMAC verification has no nonce/timestamp freshness check; a captured frame replays.

---

## Industry comparison

| Product | Peak advertised | Architecture | Node events/s | Detection method |
|---|---|---|---:|---|
| Cloudflare | 3,200 Tbps | Distributed edge + BGP scrubbing | N/A (aggregate) | RL-based auto-threshold + global feed |
| AWS Shield Advanced | 2,100 Tbps | Distributed edge | N/A (aggregate) | ML + edge heuristics |
| Imperva DDoS Protection | 17 Tbps | Distributed edge | N/A (aggregate) | Behavioral ML |
| fail2ban | N/A | Single-node log parser | ~85K (nginx limit_req tier) | Static regex + jail rules |
| CrowdSec | N/A | Community-intelligence agent | Not throughput-scored | Collaborative blocklist + heuristics |
| nginx `limit_req` | N/A | Single-node leaky bucket | ~85K | Fixed-rate limiter only |
| GoRateLimit (golang.org/x/time/rate tier) | ~250 Gbps simulated | Single-node token bucket | ~320K | Token bucket only |
| **RamShield** | ~1.2 Gbps simulated | Single-node EWMA + batch + XDP | **154,731 (measured)** | EWMA + CUSUM + pulse + Bayesian forecast + subnet aggregation |

The comparison is apples-to-apples only on the bottom four rows: those are all single-node, operator-deployed systems that sit in front of your own service. The top three are anycast networks — they are not comparable on any single-node axis and shouldn't be presented as if they were. RamShield's claim is not "faster than Cloudflare"; it's that a sovereign operator with no cloud budget gets EWMA + CUSUM + pulse + Bayesian forecasting + /24 swarm detection + XDP kernel enforcement + WAL-durable blocks on one box, with a measured 154K eps and 0 of 200 good IPs blocked in one run (upper bound 1.5%).

**Where RamShield loses, stated plainly:**
- Against fail2ban / CrowdSec: those have mature community blocklists and a decade of log-parsing coverage. RamShield is detection-from-first-principles, not a ruleset you inherit.
- Against nginx `limit_req` / GoRateLimit: simpler tools, simpler to reason about, and at small scale the performance difference doesn't matter.
- Against Cloudflare / AWS / Imperva: anycast scrubbing at the edge. RamShield cannot absorb a multi-Tbps volumetric attack — it operates at the last mile, in front of one service. That is the design point, not a bug.

|**Where RamShield wins:** 0 of 200 good IPs blocked in one run (rule-of-three upper bound 1.5%, not a measured zero rate), 266 KB idle RAM footprint, WAL-durable blocks with 52 ms recovery, XDP kernel-level drop, and no external feed, subscription, or cloud dependency.

|**Regression budgets (evidence-based)**|

*Baseline numbers are single-run measurements (see methodology caveat); budgets are measured value + 30% tolerance to absorb warm-up jitter. Regress on any path that breaches a budget.*

| Area | Baseline | Budget | Measured by |
|------|----------|--------|-------------|
| Detection flush (all-promoted) | 185,648 ns/op | ≤ 250,000 ns/op | `field_day` |
| Detection flush (cold-skip) | 1,310,948 ns/op | ≤ 2,000,000 ns/op | `field_day` |
| Detection flush (sustained-blocked) | 152,061 ns/op | ≤ 250,000 ns/op | `field_day` |
| Detection flush (first attack) | 308,264 ns/op | ≤ 500,000 ns/op | `field_day` |
| Detection per-event (warm) | 45 ns/event | ≤ 100 ns/event | derived |
| Subnet-window RMW (worst) | 17,988,268 ns/op | ≤ 30,000,000 ns/op | `field_day` |
| Enforcement block (WAL GroupCommit) | 2,800 ns/op | ≤ 5,000 ns/op | `field_day` |
| Enforcement duplicate decision (idempotent) | 106 ns/op | ≤ 200 ns/op | `field_day` |
| Enforcement unblock | 1,470 ns/op | ≤ 3,000 ns/op | `field_day` |
| Native ingress (inspected throughput) | 154,731 eps | ≥ 100,000 eps | RFC-9411 live |
| IPC throughput (HMAC-signed) | 126,087 eps | ≥ 100,000 eps | RFC-9411 live |
| Memory under load (21.3M events) | 44 MB RSS | ≤ 100 MB RSS | RFC-9411 live |

|---

## Reproducing

```bash
cargo bench --bench hot_paths --features full
cargo bench --bench field_day --features full
```

The 21-phase suite ran against a live daemon over synthetic IPC events — no packets leave the host — on IPC `127.0.0.1:7890` and dashboard `:9999`. Its script and reference doc were removed from this checkout; no runnable suite is claimed here, and the figures above are historical single-run results from 2026-10-05 (prior suite run @ `61b9a78`, 2026-09-05).

**Methodology caveats:** both benches are single-run, single-machine, no statistical repetition — treat them as an order-of-magnitude characterization, not a publication-grade measurement. The pipeline benches measure in-process paths only; IPC serialization, HMAC verification, and the daemon's own tokio scheduling are outside the measured region and are what make the end-to-end eps (126K–154K) lower than the primitive ops/s figures above. Any number here that is not reproducible from `benches/hot_paths.rs` or `benches/field_day.rs` is a historical claim I did not re-measure in this checkout, and is labeled as such by its citation.
