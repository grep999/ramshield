# ramshield-detection

## Problem

When a DDoS attack hits, thousands of IPs flood your server simultaneously. Processing each connection event individually is too slow — you need to batch them, aggregate per-IP statistics, and decide within milliseconds which IPs are attackers. The detection module is the analytical engine that answers: "Given the last 50ms of traffic, which IPs should be blocked?"

The core challenge is doing this fast enough to keep up with millions of events per second while avoiding false positives on legitimate traffic spikes (like a product launch or news event).

## How it works

### Ingestion pipeline

```
ConnectionEvents (from IPC)
        │
        ▼
PreAggregator (DashMap<IpAddr, IpAgg>, 50ms window)
        │  flush()
        ▼
DetectionEngine
        ├── BloomFilter — fast "have we seen this IP?" check
        ├── EWMA — exponential moving average for rate tracking
        ├── CUSUM — cumulative sum for slow-ramp detection
        ├── Pulse tracker — burst pattern detection
        ├── Subnet swarm gate — /24 or /64 group blocking
        └── Threat scoring — per-IP risk score
        │
        ▼
EnforceCommand → EnforcementEngine
```

### PreAggregator

The entry point. A `DashMap<IpAddr, IpAgg>` collects events for 50ms. Each event is absorbed into an `IpAgg`:

```rust
pub struct IpAgg {
    pub count: u32,
    pub bytes: u64,
    pub status_dist: [u32; 5],  // 2xx, 3xx, 4xx, 5xx, other
    pub proto_fp: u32,
    pub first_ts_ns: u64,
    pub last_ts_ns: u64,
}
```

After 50ms, `flush()` drains all entries to the detection engine and resets the map. At 100K unique IPs, the DashMap uses ~4MB.

### Rate tracking (pure math, zero dependencies)

**EWMA** — `ewma(prev, sample)` with α=0.3. One line of code. Inlined by the compiler to <1 ns/op. Tracks per-IP request rate.

**CUSUM** — `cusum_step_capped(prev_s, inst, baseline, cap)` detects slow-ramp attacks invisible to z-score thresholds. The accumulator grows when the rate exceeds the baseline. `cusum_fired(s, threshold)` checks if it crossed the decision boundary. 3 ns/op.

**Pulse tracker** — `pulse_tracker_step(...)` counts consecutive over-threshold samples in a sliding window. Catches short bursts spaced just below the detection limit (e.g., 2s-on/3-off T13 patterns). <1 ns/op (inlined).

### Bloom filter

```rust
pub struct BloomFilter { ... }
impl BloomFilter {
    pub fn new(bits: usize) -> Self   // default: 8M bits = 1 MB
    pub fn insert(&mut self, ip: IpAddr)
    pub fn contains(&self, ip: IpAddr) -> bool
}
```

Probabilistic set membership. At 8M bits and 100K IPs, false positive rate is ~0.1%. Insert: 38 ns/op. Contains: 258 ns/op. Used for cold-skip optimization: 93% of traffic never touches the expensive detection path.

### Subnet swarm detection

Tracks /24 (IPv4) or /64 (IPv6) subnets in 2-second rolling windows:

```
if unique_ips ≥ 50 AND total_events ≥ 100:
    → BLOCK entire subnet (10s TTL)
```

The dual gate prevents false positives on CGNAT and shared hosting. A single abusive client at 500 events is one offender; 50 IPs at 12 events each is a swarm.

### Threat scoring

Per-IP score combines rate deviation, status code distribution, and protocol anomalies into a [0.0, 1.0] value. Feeds the forecasting module's Bayesian hypothesis tracker.

## Dependencies

```
ramshield-detection
  ← ramshield-types    (ConnectionEvent, EnforceCommand, BlockReason)
  ← ramshield-storage  (Store, IpRecord, subnet_key)
  ← ramshield-config   (ConfigHandle, DetectionConfig)
  ← ramshield-metrics  (Metrics, BatchRecord)
  ← ahash, dashmap, crossbeam-channel, tokio::sync::mpsc

Sends to: ramshield-enforcement (via EnforceCommand channel)
Reads from: ramshield-storage (Store lookups during merge)
```

The detection engine is the bridge between raw events and blocking decisions. It reads events from the IPC layer, processes them through the math pipeline, and sends `EnforceCommand` values to the enforcement module.

## Key benchmarks

| Function | ns/op | ops/sec |
|----------|-------|---------|
| ewma() | <1 | ~1.8B |
| cusum_step + fired | 3.2 | 317M |
| BloomFilter::insert | 38 | 26M |
| BloomFilter::contains | 258 | 3.9M |
| batch::aggregate (4096 events) | 309μs | 3.2K |

At 19K events/s, the detection pipeline uses ~1% CPU. The remaining 99% is headroom for traffic spikes.

## What to read next

- `crates/ramshield-forecasting/` — reads threat scores from this module, makes block/no-block decisions
- `crates/ramshield-storage/` — stores per-IP records updated by this module
- `crates/ramshield-enforcement/` — receives block commands from this module
