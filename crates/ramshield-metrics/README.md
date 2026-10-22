# ramshield-metrics

```text
Detection   ──→ record_batch(BatchRecord) ─────┐
Detection   ──→ record_block_ip(ip, reason, …) ─┤
Forecasting ──→ record_block_ip(ip, reason, …) ─┤
IPC handler ──→ record_block_ip(ip, reason, …) ─┤
Forecasting ──→ set_forecast_hw(rps, z, f) ─────┤──→ Metrics
Forecasting ──→ set_entropy(h) ────────────────┤      │
Enforcement ──→ inc_blocks(), set_wal_lsn(…) ──┤      │
IPC server  ──→ inc_requests(), inc_rejected(…)┘      │
                                                      ↓
                             ┌────────────────────────┴────────────┐
                             ↓                                     ↓
                    DashboardSnapshot                       Prometheus text
                    (JSON, /api/snapshot)                   (text, /metrics)
                    assembled by the root crate
```

## Why it exists

Operators need to know what RamShield is doing right now: how many events
per second, how many IPs are blocked, which subsystem triggered the blocks,
and whether the forecast models are tracking normal traffic. Without a
centralized metrics store, each subsystem would maintain its own counters.

## How it works

### Atomic counters

Every hot-path counter is an `Arc<AtomicU64>`; the increment is a
`fetch_add`, so the event path never takes a lock. `Metrics` has 85 public
fields — 80 counters plus the two ring buffers and their cap. One flag
(`xdp_projection_active`) is an `AtomicBool`, and three caches are behind
`Mutex`.

```rust
pub struct Metrics {
    pub requests_total: Arc<AtomicU64>,   // total IPC requests received
    pub blocks_total: Arc<AtomicU64>,     // total block decisions applied
    pub events_ingested: Arc<AtomicU64>,  // total events ingested
    pub events_rejected: Arc<AtomicU64>,  // events rejected
    // … 72 more counters …
    pub batch_history: Arc<Mutex<VecDeque<Arc<BatchRecord>>>>, // last 80 batch stats
    pub block_log: Arc<Mutex<VecDeque<BlockRecord>>>,          // last N block events
    pub block_log_cap: usize,
}
```

`batch_history` keeps the last **80** records (`HISTORY` in `src/lib.rs`);
`block_log` keeps the last `block_log_size` records, which comes from
`[dashboard] block_log_size` and defaults to 1000. The `batch_history` and
`block_log` rings are written at batch flush boundaries — every
`detection.batch_window_ms` (default 50 ms, maximum 500 ms) for the batch
history, and on a block decision for the block log. The hot path — counting
events — never touches a lock.

### BatchRecord

Recorded at every detection flush. The forecaster does **not** read this
ring: it reads the store's live traffic counters
(`store.traffic.events_last_second` and `store.traffic.unique_ips_window`).

```rust
pub struct BatchRecord {
    pub ts_ms: u64,
    pub events: u32,
    pub unique_ips: u32,
    pub promoted: u32,             // unique IPs promoted to full tracking
    pub cold_skipped: u32,         // unique IPs skipped (below the promotion threshold)
    pub promoted_events: u32,      // connection events in promoted IPs
    pub cold_skipped_events: u32,  // connection events in cold-skipped IPs
    pub blocks: u32,
    pub hot_subnets: u32,
}
```

### BlockRecord

Recorded by the detection, forecasting and IPC paths when they issue a
block. The enforcement service does **not** write the block log — it counts
with `inc_blocks()` and `inc_xdp_apply_failures()`.

```rust
pub struct BlockRecord {
    pub ts_ms: u64,
    pub ip: String,
    pub reason: String,    // a BlockReason::as_str() token
    pub module: String,    // "detection" | "forecasting" | "ipc"
}
```

The `reason` is normally one of the stable wire tokens from
`ramshield-types::BlockReason::as_str()` — except that the subnet-scan path
writes `subnet_batch`, which is not one of those tokens:

`high_rps`, `subnet_burst`, `forecast_anomaly`, `entropy_anomaly`, `manual`,
`l7_high_rps`, `l7_cost`, `http2_stream_abuse`.

The `module` is the subsystem that issued the block: `detection` (the flush,
mitigation and subnet-scan paths), `forecasting` or `ipc`.

### Dashboard snapshot

`DashboardSnapshot` is a plain data type in this crate. The snapshot is
assembled by the **root** crate — `Engine::dashboard_snapshot()` in
`src/engine/mod.rs` — from the store and these counters; there is no
`get_dashboard_snapshot()` here.

```rust
pub struct DashboardSnapshot {
    pub ts_ms: u64,
    pub uptime_secs: u64,
    pub ips_tracked: usize,
    pub blocked_total: u64,
    pub ram_bytes: usize,
    pub ram_limit_mb: usize,
    pub ram_pct: f64,
    pub cpu_usage: f32,
    pub memory_usage_mb: usize,
    pub total_ram_mb: usize,
    pub ipc_requests: u64,
    pub events_ingested: u64,
    pub events_rejected: u64,
    pub frames_rejected_total: u64,
    pub channel_depth: usize,
    pub events_shed: u64,
    pub batches_total: u64,
    pub promotions: u64,
    pub cold_skipped: u64,
    pub blocks_applied: u64,
    pub pipeline: PipelineFlow,   // ingest / queued / batched / promoted / merged / blocked
    pub is_healthy: bool,
    pub health_reason: String,
    pub xdp_active: bool,          // the kernel dataplane is loaded and attached
    pub wal_lsn: u64,
    pub pending_expirations: u64,
    pub xdp_apply_failures: u64,
    pub xdp_projection_stale: bool,
    pub xdp_configured: bool,
    pub protection_state: ProtectionState,
    pub native_capture_workers_running: u64,
    pub native_capture_worker_failures: u64,
}
```

The dashboard serves it as JSON at `/api/snapshot`. The IPC `Stats`
response is a **separate, smaller** view built from `store.get_stats()`
(`ips_tracked`, `blocked`, `ram_bytes`, `ram_limit_mb`, `uptime_secs`,
`evictions`) — it does not read this crate.

### Prometheus export

`render_prometheus()` produces standard Prometheus text format:

```
# HELP ramshield_uptime_seconds Process uptime in seconds.
# TYPE ramshield_uptime_seconds gauge
ramshield_uptime_seconds 342

# HELP ramshield_events_ingested_total Total events ingested.
# TYPE ramshield_events_ingested_total counter
ramshield_events_ingested_total 5490000

# HELP ramshield_blocks_total Total blocks issued.
# TYPE ramshield_blocks_total counter
ramshield_blocks_total 47
```

The counter for ingested events is `ramshield_events_ingested_total`.

### Native ingest drops

The native capture path (Linux, `native_ingest.enabled`) sends observation
events downstream **without blocking**, so a saturated queue drops the event
rather than stalling the capture thread. That is the intended overload
policy, but only if the loss is visible:

| Metric | Meaning |
|---|---|
| `ramshield_native_ingest_drops_total` | Every observation event the native producer built but could not hand to the downstream channel. |
| `ramshield_native_ingest_drops_disconnected_total` | The subset where the downstream channel was **closed** rather than full — the detection pipeline is gone, not busy. |

A full channel is backpressure and needs no log line. A closed channel is
logged once per daemon (every subsequent event is rejected too, so a log per
event would be a flood) and is broken out so an operator can tell the two
apart.

Queue pressure comes in two gauges:

| Metric | Meaning |
|---|---|
| `ramshield_ingest_channel_depth` | Current buffered events in the bounded ingest queue. |
| `ramshield_ingest_channel_depth_peak` | High-water mark since process start. |

The native capture worker counters say whether the capture path is
actually running:

| Metric | Meaning |
|---|---|
| `ramshield_native_capture_workers_expected` | Worker slots the capture path plans to run. |
| `ramshield_native_capture_workers_started` | Slots that were actually started — a spawn failure is a real loss of capture capacity. |
| `ramshield_native_capture_workers_running` | Slots alive right now. A slot between two capture attempts still counts as alive. |
| `ramshield_native_capture_worker_failures_total` | Capture attempts that failed. |

The instantaneous depth says how full the queue is right now; the peak says
how close it has ever come to saturation. Read together with the drop
counter, the peak distinguishes "the queue is genuinely too small for this
load" from "a brief spike", which the instantaneous value alone cannot.

These count **channel rejections only**. A packet that never becomes an
observation event — a non-IP frame, a trusted-overlay skip, or one refused by
the per-second admission budget — is not counted, because no event was built
to deliver. They are also Prometheus-only: the dashboard's own drop figures
do not include native ingest drops, so an operator reading only the dashboard
will not see this loss.

`ramshield_blocks_total` counts block **decisions issued** — it matches the
`HELP` text the exporter emits. A block whose kernel (XDP) projection failed
is still counted here, because the decision was issued; those failures are
counted separately by `ramshield_xdp_apply_failures_total`. The two
counters have different populations — `inc_blocks` counts enforcement
decisions, while the block log counts detection-side blocks — so their
difference is not the number of blocks in the kernel map.

`render_prometheus_cached()` caches the output for 1 second. The cache
absorbs bursts of scrapes inside the same second; a typical 15–30 second
scrape interval is longer than the TTL, so each scrape re-renders the text.

### System usage

`get_system_usage()` uses the `sysinfo` crate and returns
`(cpu_usage: f32, total_ram_mb: usize, own_process_rss_mb: usize)`. The
second element is the machine's total RAM and the third is this process's
RSS, both in megabytes. It is called from `Engine::dashboard_snapshot()`
and from `get_module_stats_data()`; an internal 1-second cache keeps it
cheap.

## Uniqueness

**Lock-free hot path.** Every counter that increments on every event uses
atomics. The `Mutex`-protected rings are only written at flush boundaries —
not per-event.

**Cached Prometheus export.** Re-rendering Prometheus text on every scrape
wastes CPU. The 1-second cache means a burst of scrapes inside the same
second renders the text once.

## Dependencies

**Depends on:** `serde`, `serde_json`, `sysinfo`. This crate has **no**
workspace-crate dependency — it never touches `Store`.

**Written by:** `ramshield-detection` (`record_batch`, `record_block_ip`,
`inc_ingested`), `ramshield-forecasting` (`set_forecast_hw`, `set_entropy`,
`record_block_ip`), `ramshield-enforcement` (`inc_blocks`,
`inc_xdp_apply_failures`, `set_wal_lsn`), the root package's IPC server
(`inc_requests`, `inc_rejected`) and the root engine's native ingest
(`inc_native_ingest_drop`).

**Read by:** `ramshield` (the root package — its `src/dashboard/` module serves `/api/snapshot`, `/metrics` and `/api/history/*` from it, and `src/ipc/` is the other consumer), `ramshield-detection`, `ramshield-enforcement` and `ramshield-forecasting`. The dashboard and the IPC server are modules of the root crate, not crates of their own.

## Benchmarks

Measured in the workspace `hot_paths` bench (see `docs/BENCHMARKS.md`):

| Operation | ns/op | ops/s |
|---|---:|---:|
| `inc_requests` (AtomicU64) | 7.2 | 139,373,988 |
| `inc_ingested` (AtomicU64) | 6.9 | 144,835,178 |
| `render_prometheus` (text format) | 20,638.6 | 48,453 |

## Testing

18 tests: JSON-cache invalidation for the block log and the batch history,
the 1-second Prometheus render cache, the ingest depth and peak gauges,
the IPC rejection breakdown series, the bloom series and the k=2
false-positive formula, the XDP staleness and apply-failure counters, the
native-ingest drop counters, the native capture worker counters, and
block-log ring eviction at the configured cap.
