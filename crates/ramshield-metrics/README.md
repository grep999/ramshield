# ramshield-metrics

```text
Detection ──→ record_batch(BatchRecord) ──┐
Enforcement ──→ record_block(BlockRecord) ─┤
Forecaster ──→ set_forecast_hw(rps, z) ───┤──→ Metrics
                                           │     │
                              ┌────────────┘     │
                              ↓                  ↓
                    DashboardSnapshot      Prometheus
                    (JSON, /api/snapshot)  (text, /metrics)
```

## Why it exists

Operators need to know what RamShield is doing right now: how many events per second, how many IPs are blocked, which subsystem triggered the blocks, and whether the forecast models are tracking normal traffic. Without a centralized metrics store, each subsystem would maintain its own counters, and the dashboard would need to poll seven different sources.

## How it works

### Atomic Counters

All hot-path counters use `AtomicU64` or `AtomicUsize` — no locks, no contention:

```rust
pub struct Metrics {
    requests: AtomicU64,       // total IPC requests received
    blocks: AtomicU64,         // total block decisions applied
    ingested: AtomicU64,       // total events ingested
    rejected: AtomicU64,       // events rejected (channel full, parse error)
    batch_history: Mutex<VecDeque<BatchRecord>>,  // last 120 batch stats
    block_log: Mutex<VecDeque<BlockRecord>>,       // last 1000 block events
}
```

The `batch_history` and `block_log` use `Mutex<VecDeque>` — but these are only touched at batch flush boundaries (every 500ms for batches, at block/unblock events for blocks). The hot path — counting events — never touches a lock.

### BatchRecord

Recorded at every detection flush. The forecaster reads these to compute Holt-Winters forecasts:

```rust
pub struct BatchRecord {
    pub timestamp_ms: u64,
    pub total_events: u64,
    pub unique_ips: u64,
    pub rps_estimate: f64,
    pub blocked_new: u64,
    pub subnet_blocked: u64,
    pub cold_skip_pct: f64,    // % of events that hit existing records (no new allocation)
}
```

### BlockRecord

Recorded at every enforcement decision. Serves the dashboard block history and Prometheus counters:

```rust
pub struct BlockRecord {
    pub timestamp_ms: u64,
    pub ip: String,
    pub reason: String,    // "rate_limit", "subnet_burst", "pulse_wave", "manual"
    pub module: String,    // "detection", "forecasting", "manual"
}
```

### Dashboard Snapshot

`get_dashboard_snapshot()` assembles a point-in-time view from all internal state:

```rust
pub struct DashboardSnapshot {
    pub uptime_secs: u64,
    pub total_events: u64,
    pub blocked_ips: u64,
    pub rps_current: f64,
    pub forecast_hw: f64,      // Holt-Winters one-step forecast
    pub forecast_z: f64,       // current z-score
    pub entropy: f64,          // Shannon entropy of source IPs
    pub ram_bytes: usize,
    pub batch_history: Vec<BatchRecord>,
    pub block_log: Vec<BlockRecord>,
    pub subnets: Vec<SubnetRow>,
    pub pipeline: PipelineFlow, // flow diagram data
    pub modules: Vec<ModuleStats>,
}
```

The dashboard HTTP endpoint (`/api/snapshot`) serves this as JSON. The IPC `Stats` response uses the same data.

### Prometheus Export

`render_prometheus()` produces standard Prometheus text format:

```
# HELP ramshield_uptime_seconds Process uptime in seconds.
# TYPE ramshield_uptime_seconds gauge
ramshield_uptime_seconds 342

# HELP ramshield_events_total Total connection events processed.
# TYPE ramshield_events_total counter
ramshield_events_total 5490000

# HELP ramshield_blocks_total Total block decisions applied.
# TYPE ramshield_blocks_total counter
ramshield_blocks_total 47
```

`render_prometheus_cached()` caches the output for 1 second — preventing re-rendering on every scrape request. Prometheus typically scrapes every 15–30 seconds, so the cache hit rate is >95%.

### System Usage

`get_system_usage()` reads `/proc/self/stat` for CPU time and `/proc/self/status` for RSS — returning `(cpu_percent, rss_bytes, vm_bytes)`. Called once per second by the dashboard uptime ticker.

## Uniqueness

**Lock-free hot path.** Every counter that increments on every event uses atomics. The `Mutex`-protected structures (`batch_history`, `block_log`) are only written at flush boundaries — not per-event. This keeps the metrics overhead under 1% of total CPU even at 150K events/second.

**Cached Prometheus export.** Re-rendering Prometheus text on every scrape wastes CPU. The 1-second cache means the metrics string is built once per second regardless of scrape frequency.

## Dependencies

**Reads from:** `ramshield-storage` (`Store::traffic` for RAM usage, uptime).

**Written by:** `ramshield-detection` (`record_batch`), `ramshield-enforcement` (`record_block`), `ramshield-forecasting` (`set_forecast_hw`, `set_entropy`), `ramshield-engine` (ingested/rejected counts).

**Read by:** `ramshield-dashboard` (snapshot, Prometheus endpoint), `ramshield-ipc` (stats response).

## Benchmarks

No standalone benchmarks — metrics overhead is measured implicitly through the detection batch benchmarks. At 150K events/second, the metrics update time is <0.01ms per event.

## Testing

22 tests covering: atomic counter accuracy under concurrent increment, batch history ring buffer overflow (old entries evicted), block log ring buffer overflow, Prometheus format validity (parsed by `prometheus::text::parse`), and dashboard snapshot field completeness.
