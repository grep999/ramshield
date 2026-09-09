# ramshield-storage

```text
                    ┌───────────────────────────────┐
                    │          Store                  │
                    │  ┌─────────────┐ ┌───────────┐ │
Detection ──→ insert│  │ IpEntryMap  │ │ SubnetMap │ │
                    │  │ DashMap<IP> │ │ DashMap<SN>│ │
Forecaster ──→ update│  └─────────────┘ └───────────┘ │
Enforcement ←── get  │  ┌─────────────┐ ┌───────────┐ │
Dashboard   ←── query│  │ BlockedSet  │ │ SubnetIdx │ │
                    │  │ DashSet<IP> │ │ DashMap<SN>│ │
                    │  └─────────────┘ └───────────┘ │
                    │  TrafficCounters (atomics)      │
                    └───────────────────────────────┘
                          ↕
                    Wal (crash recovery)
```

## Why it exists

Every other subsystem needs per-IP state — threat scores, block status, event counts, subnet membership — but no subsystem should own it. A centralized in-memory store with RAM-bounded eviction and WAL-backed crash recovery lets detection write, enforcement read, forecasting update, and dashboard query without duplicating state or fighting over locks.

## How it works

### Sharded IP Map

`Store` wraps a `DashMap<IpAddr, Entry>` with configurable shard count (default: power-of-two matching CPU cores). Each shard has its own RwLock — operations on different keys never contend:

```rust
pub struct Store {
    inner: Arc<IpEntryMap>,          // DashMap<IpAddr, Entry>
    subnet_table: Arc<SubnetTable>,  // DashMap<SubnetKey, SubnetRecord>
    blocked_set: Arc<DashSet<IpAddr>>,
    subnet_index: Arc<SubnetIndex>,  // DashMap<SubnetKey, DashSet<IpAddr>>
    pub traffic: Arc<TrafficCounters>,
    // ...eviction, RAM tracking
}
```

### IpRecord — Per-IP State

Each IP gets an `IpRecord` with fields updated by different subsystems:

```rust
pub struct IpRecord {
    pub event_count: u64,
    pub first_seen_ns: u64,
    pub last_seen_ns: u64,
    pub threat_score: f32,        // updated by Forecaster (CUSUM accumulator)
    pub block_state: BlockState,  // written by Enforcement
    pub status_history: BoundedVecDeque<u16>,  // last 20 status codes
}
```

`Entry` wraps `IpRecord` with optional inline storage — IPs with fewer than 64 events store their status history in the struct itself (no heap allocation). Above 64 events, the history spills to a `BoundedVecDeque` on the heap.

### Subnet Tracking

Two parallel maps support subnet-level detection:

- `SubnetTable`: maps `/24` (IPv4) or `/64` (IPv6) keys to `SubnetRecord` — event count, unique IP count, last-seen timestamp, CIDR string.
- `SubnetIndex`: reverse index mapping subnet keys to the set of IPs observed in that subnet.

The subnet key is a `u128` that packs the network address and prefix length into a single comparable integer: `subnet_key_v4(octets: [u8; 4])` zeros the host bits and shifts the result into the upper 32 bits, while `subnet_key_v6(octets: [u8; 16])` does the same for 128-bit addresses.

### RAM-Bounded Eviction

`TrafficCounters` tracks total heap usage via `AtomicU64`. When `ram_bytes()` exceeds `ram_limit_mb` (default 128 MB), eviction runs in two passes:

1. **Expired entries** — IPs with `last_seen_ns` older than `rate_window_secs` are removed.
2. **LRU eviction** — if still over limit, the least-recently-seen entries are removed in batches of 1024.

The RAM accounting is conservative: each `IpRecord` is estimated at `size_of::<IpRecord>() + inline data`. Evicted IPs are simply forgotten — detection will re-create their records if they reappear.

### Write-Ahead Log (WAL)

`Wal` provides crash recovery for enforcement decisions. Every `EnforceCommand` is appended to the WAL before XDP rules are applied:

```rust
pub struct Wal {
    inner: Arc<Mutex<Inner>>,
    compress: bool,
    durability: Durability,  // NoSync | Fsync | Fdatasync
    seg_max: u64,            // segment rotation threshold (bytes)
    retention_max: u64,      // total disk cap (oldest segments deleted)
    base_dir: String,
}
```

On startup, `Wal::replay(dir)` reads all WAL segments and returns the entries in LSN order. `EnforcementService` replays them to restore blocked IPs and re-apply XDP rules — the system recovers from a crash without losing block state.

The WAL supports three durability modes:
- `NoSync`: fastest, blocks may be lost on power failure.
- `Fsync`: `fsync()` after every append — guaranteed durable, ~2ms per write.
- `Fdatasync`: `fdatasync()` — durable for data, metadata may be stale.

### TrafficCounters

A set of atomic counters shared across all subsystems for dashboard and Prometheus metrics:

```rust
pub struct TrafficCounters {
    pub ram_limit_mb: AtomicUsize,
    pub ram_bytes: AtomicUsize,
    pub total_events: AtomicU64,
    pub unique_ips: AtomicU64,
    pub uptime_secs: AtomicU64,
    pub threat_samples: Mutex<Vec<(IpAddr, f32)>>,
}
```

No locks in the hot path — every counter uses `AtomicU64::fetch_add` or `AtomicUsize::store`. The `threat_samples` mutex is only touched during batch flush (every 500ms).

## Uniqueness

**RAM-bounded by design.** Most in-memory stores grow until they OOM. This one tracks its own heap usage and evicts entries when the limit is hit. The operator sets `ram_limit_mb`; the store enforces it without external pressure.

**Subnet reverse index.** The `SubnetIndex` maps subnet keys back to individual IPs — enabling O(1) lookup of "all IPs in this /24" for batch subnet blocks. Without this index, finding all IPs in a subnet would require scanning the entire map.

**WAL with configurable durability.** Three modes let operators trade crash safety for throughput: `NoSync` for benchmarking, `Fdatasync` for production, `Fsync` for financial environments where every block decision must survive power loss.

## Dependencies

**Reads from:** `ramshield-types` (`ConnectionEvent`, `IpNetwork`, `BlockReason`, `Durability`, `BoundedVecDeque`).

**Written by:** `ramshield-detection` (insert, update_ip), `ramshield-forecasting` (update_ip for threat scores), `ramshield-enforcement` (update_ip for block_state), `ramshield-enforcement::wal` (append, checkpoint).

**Read by:** `ramshield-enforcement` (get, evict_batch), `ramshield-dashboard` (query, stats), `ramshield-metrics` (traffic counters).

## Benchmarks

```bash
cargo bench --bench hot_paths --features full -- store_insert
cargo bench --bench hot_paths --features full -- store_lookup
cargo bench --bench hot_paths --features full -- subnet_key
```

## Testing

52 tests covering: shard-level concurrency (10 threads inserting simultaneously), RAM limit enforcement (insert until limit hit, verify eviction), WAL append+replay round-trip, subnet index consistency (insert IP → verify subnet index → remove IP → verify index cleaned up), and expired entry eviction timing.
