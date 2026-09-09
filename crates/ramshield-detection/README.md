# ramshield-detection

Per-IP event aggregation, subnet batching, rate tracking, Bloom filtering, and threat scoring. This is the analytical core of RamShield — every connection event passes through here before a block decision is made.

## Architecture

```
ConnectionEvents (from IPC)
        │
        ▼
batch::PreAggregator (50ms window, DashMap<IpAddr, IpAgg>)
        │  flush()
        ▼
detection::Engine
        ├── IpAgg → per-IP counters (count, bytes, status_dist, proto_fp)
        ├── promote_to_store() — hot IP → Store
        ├── BloomFilter — IP membership (insert + contains)
        ├── rate_tracker::ewma() — exponential moving average
        ├── rate_tracker::cusum_step_capped() — cumulative sum drift detector
        ├── rate_tracker::pulse_tracker_step() — burst pattern detector
        ├── subnet detection — /24 (v4) or /64 (v6) swarm gate
        └── threat scoring — multi-signal per-IP score
```

## batch::PreAggregator

The entry point for all connection events. Implements a 50ms batch window using a `DashMap<IpAddr, IpAgg>` for lock-free concurrent aggregation.

```rust
pub fn aggregate(events: &[ConnectionEvent]) -> HashMap<IpAddr, IpAgg>
```

`aggregate()` is the hot path. For each event, it upserts an `IpAgg` entry:
- Increments `count` and `bytes`
- Buckets `status_code` into 5 categories (2xx, 3xx, 4xx, 5xx, other)
- XOR-merges `proto_fingerprint` (protocol anomaly detection)

After 50ms (or when the channel fills), `flush()` drains all entries to the detection engine and spawns a subnet window update.

### IpAgg

```rust
pub struct IpAgg {
    pub count: u32,
    pub bytes: u64,
    pub status_dist: [u32; 5],
    pub proto_fp: u32,
    pub first_seen_ns: u64,
    pub last_seen_ns: u64,
}
```

Lightweight — 40 bytes per IP. At 100K unique IPs, this is 4 MB in the DashMap. The 50ms window prevents unbounded growth: after flush, entries are drained and the map resets.

## detection::Engine

The main detection pipeline. Called by the engine's batch flush loop after `PreAggregator::flush()`.

### Rate tracking

**EWMA (Exponential Weighted Moving Average):**
```rust
pub fn ewma(prev: f64, sample: f64) -> f64  // alpha = 0.3
```
Single-line function. Computes the new moving average with alpha=0.3 (fast adaptation). Used for per-IP RPS estimation. Inlined by the compiler — benchmarked at <1 ns/op.

**CUSUM (Cumulative Sum drift detector):**
```rust
pub fn cusum_step_capped(s: f64, z: f64, drift: f64, cap: f64) -> f64
pub fn cusum_fired(s: f64, threshold: i32) -> bool
```
Detects slow-ramp attacks invisible to z-score thresholds. The accumulator `s` grows when the rate exceeds the baseline by more than `drift`. `cusum_fired()` checks if `s` has exceeded the decision boundary (threshold consecutive samples). Capped at `cap` to prevent runaway accumulation from a single spike. Benchmark: 3 ns/op.

**Pulse-wave tracker:**
```rust
pub fn pulse_tracker_step(
    prev_count: u8,
    prev_window_start_ns: u64,
    now_ns: u64,
    over_threshold: bool,
    window_secs: u64,
    threshold: u8,
) -> (u8, u64, bool)
```
Sliding-window burst detector. Counts how many consecutive samples exceed the threshold within a time window. When the count reaches the threshold, it fires — detecting short bursts spaced just below the EWMA detection limit. Benchmark: <1 ns/op (inlined).

### Bloom filter

```rust
pub struct BloomFilter { ... }
impl BloomFilter {
    pub fn new(bits: usize) -> Self       // default: 8M bits = 1 MB
    pub fn insert(&mut self, ip: IpAddr)
    pub fn contains(&self, ip: IpAddr) -> bool
}
```

Probabilistic set membership for IP tracking. Uses `ahash` for fast hashing and bit-packed storage. False positive rate is configurable via the `bits` parameter (8M bits at 100K IPs ≈ 0.1% FPR). Insert is 38 ns/op; contains is 258 ns/op.

The Bloom filter serves two purposes:
1. Fast "have we seen this IP before?" check to skip cold IPs.
2. Deduplication in the batch window — only the first occurrence of an IP per window triggers the detection path.

### Subnet detection

Tracks /24 (IPv4) or /64 (IPv6) subnets in rolling 2-second windows:

```
for each subnet:
    if unique_ips >= 50 AND events >= 100 in 2s window:
        → BLOCK entire subnet (10s TTL)
    if window expires:
        → UNBLOCK
```

The dual gate (50 IPs + 100 events) prevents false positives on CGNAT and shared hosting. A single abusive client at 500 events is one offender; 50 IPs at 12 events each is a swarm. TTL is intentionally short (10s) — subnet blocks are re-evaluated every window.

### Threat scoring

Per-IP threat score is a weighted combination of:
- Rate deviation (EWMA vs baseline)
- Status code distribution (high 5xx ratio = suspicious)
- Protocol fingerprint anomalies
- Request volume (absolute and relative)

Scores are clamped to [0.0, 1.0]. The `threat_score` field feeds the forecasting module's Bayesian hypothesis tracker.

## Dependencies

`ramshield-types`, `ahash`, `serde`. No async runtime required. The detection engine is synchronous — it's called from the engine's batch flush loop on a dedicated thread.

## Tests (30)

- Batch: aggregate counts, byte totals, status bucket correctness, subnet key roundtrip.
- Detection: IP tracking, IPv6 swarm detection, pulse detection, rate convergence.
- CUSUM: quiet stays zero, sustained drift fires, noise rejection, reset after alarm.
- Subnet: dual-gate enforcement (50+100), cold IP not stored, hot IP promotion.
- Bloom: false positive rate within bounds at expected IP count.
- Storage integration: block/unblock lifecycle, TTL expiry, concurrent transitions.
