# ramshield-metrics

Atomic counters, batch history, block logs, and Prometheus text rendering for RamShield's operational dashboard. This crate is the single source of truth for all runtime statistics — every other module writes to it; the dashboard reads from it.

## Architecture

```
Detection Engine ──── inc_requests()
                     inc_ingested(n)
                     record_batch(BatchRecord)
                     record_block(BlockRecord)
                           │
                           ▼
                    Metrics (atomics + ring buffers)
                           │
            ┌──────────────┼──────────────┐
            ▼              ▼              ▼
        /metrics      /api/snapshot   /api/history/*
       (Prometheus)   (JSON blob)    (paginated JSON)
```

All writes are lock-free atomics (Relaxed ordering). Reads are cheap snapshots. There is no mutex contention between the hot path (write) and the dashboard (read).

## Metrics struct

```rust
pub struct Metrics {
    requests_total: AtomicU64,
    ingested_total: AtomicU64,
    ingested_bytes: AtomicU64,
    blocked_total: AtomicU64,
    evictions_total: AtomicU64,
    batch_history: Mutex<VecDeque<BatchRecord>>,
    block_log: Mutex<VecDeque<BlockRecord>>,
    // ... additional counters
}
```

### Write methods (hot path)

```rust
pub fn inc_requests(&self)           // called per IPC request frame
pub fn inc_ingested(&self, n: u64)   // called per batch flush, n = event count
pub fn record_batch(&self, rec: BatchRecord)
pub fn record_block(&self, rec: BlockRecord)
```

`inc_requests()` and `inc_ingested()` are single `AtomicU64::fetch_add` operations — approximately 9 ns each on modern hardware. They are called in the IPC server's hot path and must add zero measurable overhead.

### Read methods (dashboard path)

```rust
pub fn render_prometheus(&self) -> String
pub fn render_prometheus_cached(&self) -> Arc<str>
pub fn get_batch_history(&self) -> Vec<BatchRecord>
pub fn get_block_log(&self) -> Vec<BlockRecord>
pub fn get_block_log_json(&self) -> Arc<str>
pub fn get_batch_history_json(&self) -> Arc<str>
pub fn get_module_stats_data(&self, cfg: &Config) -> ModuleStats
pub fn snapshot_json(&self, store: &Store, cfg: &Config) -> serde_json::Value
```

`render_prometheus()` formats all counters in Prometheus exposition format. `render_prometheus_cached()` wraps the result in `Arc<str>` and caches it, recomputing only when a generation counter advances (set by `record_batch`). This avoids reformatting the full metrics text on every scrape.

## BatchRecord

```rust
pub struct BatchRecord {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub event_count: u64,
    pub window_ms: u64,
    pub unique_ips: u64,
    pub blocked_ips: u64,
    pub avg_threat: f64,
    pub detection_mode: String,
}
```

One record per detection tick (50ms batch window). Stored in a bounded `VecDeque` (default capacity 10,000). Oldest entries are evicted on overflow. The batch history is the raw material for the dashboard's throughput and threat charts.

## BlockRecord

```rust
pub struct BlockRecord {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub ip: IpAddr,
    pub reason: String,
    pub ttl_secs: Option<u64>,
    pub threat_score: f32,
    pub source: String,  // "detection", "forecasting", "manual"
}
```

One record per enforcement action. Also stored in a bounded `VecDeque`. The `source` field distinguishes detection-engine blocks from forecasting blocks from manual CLI blocks — useful for debugging false positives.

## Prometheus metrics

`render_prometheus()` outputs standard Prometheus exposition text:

```
# HELP ramshield_requests_total Total IPC requests received
# TYPE ramshield_requests_total counter
ramshield_requests_total 123456
# HELP ramshield_ingested_events_total Total connection events ingested
# TYPE ramshield_ingested_events_total counter
ramshield_ingested_events_total 9876543
# HELP ramshield_blocked_ips Currently blocked IPs
# TYPE ramshield_blocked_ips gauge
ramshield_blocked_ips 42
```

The dashboard's `/metrics` endpoint serves this directly. Prometheus scrapes it; Grafana visualizes it.

## ModuleStats

```rust
pub struct ModuleStats {
    pub detection_active: bool,
    pub forecasting_enabled: bool,
    pub xdp_enabled: bool,
    pub wal_segments: u32,
    pub store_entries: usize,
    pub store_ram_mb: f64,
}
```

Derived from the live Config and Store state. The dashboard's `/api/status/modules` endpoint returns this as JSON.

## System usage

```rust
pub fn get_system_usage() -> (f32, usize, usize)
```

Returns `(cpu_percent, resident_memory_mb, virtual_memory_mb)`. Uses the `sysinfo` crate. Called on every dashboard snapshot. Note: this is the one place where `sysinfo` adds a dependency — it's a read-only syscall, not a background polling loop.

## Dashboard integration

The dashboard (`src/dashboard/mod.rs`) holds an `Arc<Metrics>` and calls the read methods on each HTTP request. The write methods are called by the engine's batch flush loop and the IPC server's request handler. Because all writes are atomics and all reads are snapshots, there is zero contention between the hot path and the dashboard.

## Dependencies

`serde`, `serde_json`, `chrono`, `sysinfo`, `arc-swap`. No async runtime required.

## Tests

- Atomic counters: concurrent increment from multiple threads, final value matches sum.
- Batch history: insert N records, verify FIFO ordering and eviction at capacity.
- Block log: insert, verify ordering, verify JSON serialization.
- Prometheus format: valid output that Prometheus parser accepts.
- ModuleStats: derived from mock Config + Store, all fields populated.
