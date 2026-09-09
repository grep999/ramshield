# ramshield-detection

```text
IPC TCP ──→ ConnectionEvent ──→ PreAggregator ──→ Batch Workers (N threads)
                                                       │
                    ┌──────────────────────────────────┘
                    ↓
        ┌───────────────────────────────────────────┐
        │ IpAgg: per-IP counters                    │
        │ BloomFilter: dedup during batch           │
        │ EWMA tracker: exponential moving average  │
        │ CUSUM: cumulative sum change detector     │
        │ Pulse tracker: burst-spacing correlation  │
        │ Subnet aggregator: /24 swarm detection    │
        └──────────┬────────────────────────────────┘
                   ↓
              EnforceCommand ──→ EnforcementService
```

## Why it exists

Connection events arrive at 10K–150K per second during an attack. Processing each event individually would burn through memory and CPU before any analysis finished. This crate batches events, deduplicates IPs within each window, and runs four independent detection algorithms against the aggregated data — each catching a different class of attack.

## How it works

### PreAggregation

Events arrive through a crossbeam channel (`CHANNEL_CAPACITY = 64_000`). Multiple batch worker threads compete to drain it. Each worker accumulates events into a `HashMap<IpAddr, IpAgg>` — an in-memory per-IP accumulator that counts events, status code buckets, error ratios, and unique upstream IPs without touching the main `Store`:

```rust
pub struct IpAgg {
    pub count: u64,
    pub unique_upstreams: usize,
    pub status_buckets: [u16; 6],   // 1xx..5xx buckets
    pub error_ratio: f64,
    pub first_seen: Instant,
    pub last_seen: Instant,
}
```

When the batch window expires (default 500ms) or the event count hits `batch_max_events` (default 5000), the worker calls `flush_batch()`.

### Bloom Filter

A 100K-bit Bloom filter tracks IPs already seen in the current window. Insert+check is O(1) per event. This prevents double-counting a single IP sending thousands of identical packets. The filter is cleared at each batch window boundary.

### Four Detection Algorithms

**1. EWMA (Exponential Weighted Moving Average) — rate spike detection**
Tracks RPS per IP. If current RPS exceeds `ewma(prev, sample)` by more than `rps_threshold`, the IP is flagged. Catches sudden rate spikes — a single IP jumping from 10 to 5000 RPS.

**2. CUSUM (Cumulative Sum) — sustained drift detection**
If an IP's rate stays marginally above baseline for multiple consecutive samples, the CUSUM accumulator grows: `cusum_step_capped(prev_s, inst, baseline, cap)`. Once it exceeds `cusum_allowance(threshold)`, the IP is flagged even though each individual sample was below the EWMA threshold. Catches slow-ramp attacks that stay just under any single-sample alarm.

**3. Pulse tracker — burst spacing correlation**
Counts samples that exceed the rate threshold within a sliding `pulse_window_secs` window (default 5s). If the count hits `pulse_threshold_samples` (default 2), the IP is flagged. Catches intermittent attacks that fire 2-second bursts spaced 3 seconds apart — below EWMA, below CUSUM, but detectable by burst frequency.

**4. Subnet swarm — /24 block detection**
Tracks unique IPs per /24 subnet within each batch window. If a subnet has `>= subnet_batch_threshold` unique IPs (default 50) AND `>= subnet_batch_min_events` total events, the entire /24 is flagged. This catches distributed attacks where 50+ IPs each send a few events — no single IP triggers individual thresholds, but the swarm does.

### Data Structures

- **Sharded maps**: `DashMap` with configurable shard count for the IP records and subnet index. Contention scales with `sqrt(N)` shards.
- **Bounded deque**: `BoundedVecDeque` for status code history and event ring buffers — O(1) push/pop with automatic eviction.
- **SubnetKey**: `u128` that packs IPv4/IPv6 + CIDR prefix into a single comparable integer. IPv4 uses the lower 32 bits shifted to align with IPv6 prefix positions.

### Configuration

```toml
[detection]
rps_threshold = 5000           # events/sec to trigger EWMA
rate_window_secs = 10          # EWMA window
subnet_batch_threshold = 50    # unique IPs in /24 for swarm block
pulse_window_secs = 5          # burst correlation window
batch_max_events = 5000        # max events per batch flush
batch_window_ms = 500          # max wait before flush
promote_min_events = 3         # hits before full IpRecord tracking
```

## Uniqueness

**Four-algorithm detection stack.** Most DDoS detectors use a single rate threshold. RamShield runs four independent detectors in parallel — each catching what the others miss. The CUSUM accumulator is particularly effective against slow-ramp attacks that evade rate-based detection by staying below any single threshold.

**Pre-aggregation avoids per-event lock contention.** Events are accumulated in thread-local `HashMap`s, then merged into the shared `Store` only at flush boundaries. This keeps the hot path lock-free and scales linearly with CPU cores.

## Dependencies

**Reads from:** `ramshield-types` (`ConnectionEvent`, `BlockReason`, `IpNetwork`, `BoundedVecDeque`), `ramshield-storage` (`Store`), `ramshield-config` (`DetectionConfig`).

**Writes to:** `Store` (via `flush_batch`), `EnforceCommand` channel (to `ramshield-enforcement`), `ramshield-metrics` (batch statistics).

## Benchmarks

```bash
cargo bench --bench hot_paths --features full -- bloom_filter
cargo bench --bench hot_paths --features full -- pre_aggregate
cargo bench --bench hot_paths --features full -- subnet_swarm
```

## Testing

45 tests across `lib.rs` and `batch.rs`: EWMA threshold triggering, CUSUM warmup period, pulse tracker sliding window, subnet swarm unique-IP counting, Bloom filter false positive rate, batch flush timing, and worker thread shutdown. Integration tests feed synthetic traffic through `flush_events()` and verify correct block/unblock decisions.
