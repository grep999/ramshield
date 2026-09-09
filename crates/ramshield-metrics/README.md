# ramshield-metrics

## Problem

Operators need to know what RamShield is doing: How many events per second? How many IPs are blocked? Is the system healthy? Without centralized metrics, each module would need its own logging and the dashboard would need to query every module separately. The metrics module provides a single place where all modules write counters and the dashboard reads from.

The second problem is performance. If every event incremented a mutex-protected counter, the metrics themselves would become a bottleneck under attack traffic.

## How it works

### Atomic counters (hot path)

All counters are `Arc<AtomicU64>` — lock-free, ~9 ns per increment. Every module writes directly:

```rust
pub struct Metrics {
    pub requests_total: AtomicU64,       // IPC frames received
    pub events_ingested: AtomicU64,      // connection events processed
    pub events_rejected: AtomicU64,      // events dropped (channel full)
    pub blocked_total: AtomicU64,        // total blocks applied
    pub evictions_total: AtomicU64,      // RAM-pressure evictions
    pub channel_depth: AtomicU64,        // IPC channel queue depth
    // ... 20+ counters
}
```

Write methods (called from hot path):
- `inc_requests()` — per IPC frame, 9 ns
- `inc_ingested(n)` — per batch flush, 9 ns
- `record_block(ip, reason, module)` — per enforcement action
- `record_batch(BatchRecord)` — per detection tick (50ms)

### Batch history (ring buffer)

```rust
pub struct BatchRecord {
    pub ts_ms: u64,
    pub events: u32,
    pub unique_ips: u32,
    pub promoted: u32,
    pub cold_skipped: u32,
    pub blocks: u32,
    pub hot_subnets: u32,
}
```

Bounded `VecDeque` (80 entries). One record per 50ms detection tick. The dashboard's throughput chart reads from this.

### Block log (ring buffer)

```rust
pub struct BlockRecord {
    pub ts_ms: u64,
    pub ip: String,
    pub reason: String,
    pub module: String,  // "detection", "forecasting", "manual"
}
```

Bounded `VecDeque` (configurable, default 1000). One record per enforcement action. The dashboard's block history reads from this.

### Prometheus rendering

`render_prometheus()` formats all counters in standard Prometheus exposition text:

```
# HELP ramshield_requests_total Total IPC requests received
# TYPE ramshield_requests_total counter
ramshield_requests_total 123456
```

`render_prometheus_cached()` wraps in `Arc<str>` with 1s TTL — avoids reformatting on every scrape.

### Dashboard snapshot

`DashboardSnapshot` assembles all live data into a single JSON blob for the `/api/snapshot` endpoint:

```rust
pub struct DashboardSnapshot {
    pub ips_tracked: usize,
    pub blocked_total: u64,
    pub ram_bytes: usize,
    pub ram_pct: f64,
    pub cpu_usage: f32,
    pub is_healthy: bool,      // healthy = !shutting_down && ram_pct < 95%
    pub pipeline: PipelineFlow,
    // ... 20 fields
}
```

### System usage

```rust
pub fn get_system_usage() -> (f32, usize, usize)  // (cpu%, total_ram_mb, rss_mb)
```

Uses `sysinfo` with a 1s TTL cache. Called on every dashboard snapshot.

## Dependencies

```
ramshield-metrics
  ← serde, serde_json, sysinfo, arc-swap

Used by:
  → ramshield-detection (record_batch, inc_ingested)
  → ramshield-enforcement (record_block)
  → ramshield-forecasting (set_forecast_hw, set_entropy)
  → ramshield-storage (read store stats for snapshot)
  → src/dashboard (render_prometheus, get_batch_history, get_block_log)
  → src/engine (dashboard_snapshot, get_module_stats)
```

This crate has zero RamShield-internal dependencies — it's a leaf crate. Any module can import it without circular dependencies.

## What to read next

- `src/dashboard/` — serves metrics via HTTP (Prometheus, JSON API)
- `src/engine/` — assembles DashboardSnapshot from Metrics + Store
