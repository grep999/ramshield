# ramshield-storage

Sharded in-memory key-value store with WAL durability, blob storage, subnet tracking, and RAM-aware capacity management. This is the persistence layer — every block decision, IP record, and subnet aggregation lives here.

## Architecture

```
Store (DashMap<IpAddr, Value>)
    ├── blocked_set: Mutex<HashSet<IpAddr>>  — reverse index for enforcement
    ├── subnet_index: DashMap<SubnetKey, SubnetRecord>  — /24 or /64 aggregation
    ├── subnet_windows: DashMap<IpAddr, AtomicSubnetWindow>  — 2h rolling windows
    ├── ram_bytes: AtomicU64  — live memory pressure tracking
    └── traffic: TrafficCounters  — global atomic counters

WAL (append-only log)
    ├── Segments: wal-{idx}.rshw (LZ4 compressed, CRC32 checksummed)
    ├── LSN (Log Sequence Number) — monotonically increasing
    ├── Replay on startup with corruption truncation
    └── Retention: max 10 segments, oldest deleted on rotation

BlobStore
    └── Compressed storage for large payloads (SubnetRecord thresholds)
```

## Store

```rust
pub struct Store {
    pub inner: DashMap<IpAddr, Entry>,
    pub blocked_set: Mutex<HashSet<IpAddr>>,
    pub subnet_index: DashMap<SubnetKey, SubnetRecord>,
    pub subnet_windows: DashMap<IpAddr, AtomicSubnetWindow>,
    pub ram_bytes: AtomicU64,
    pub traffic: TrafficCounters,
    pub blob_store: BlobStore,
}
```

### RAM-aware insertion

```rust
pub fn insert(
    &self,
    key: IpAddr,
    value: Value,
    ttl_secs: Option<u64>,
    ram_limit_bytes: usize,
) -> Result<()>
```

Every insert checks whether `ram_bytes + heap_delta ≤ ram_limit_bytes`. If the limit is exceeded, the insert is rejected with `RamshieldError::StorageFull`. Replacements (updating an existing entry) are allowed without capacity checks since the net-zero delta doesn't increase memory pressure.

Eviction is lazy: `evict_batch()` scans for expired entries and removes them in bulk, freeing RAM and updating the blocked_set and subnet_index.

### Per-IP records (IpRecord)

```rust
pub struct IpRecord {
    pub ip: IpAddr,
    pub request_count: u64,
    pub ewma_rps: f64,
    pub cusum_s: f64,           // CUSUM accumulator
    pub baseline_rps: f64,      // slow-EWMA baseline
    pub prev_sample_hot: bool,  // debounce latch
    pub sample_count: u8,       // gates CUSUM warm-up (6 samples to arm)
    pub pulse_samples_in_window: u8,
    pub pulse_window_start_ns: u64,
    pub first_seen_ns: u64,
    pub last_seen_ns: u64,
    pub bytes_in: u64,
    pub status_dist: [u32; 5],  // 2xx, 3xx, 4xx, 5xx, other
    pub proto_fingerprint: u32,
    pub threat_score: f32,
    pub block_state: BlockState,
}
```

~150 bytes per IP. At 100K tracked IPs, this is 15 MB — well within the default 512 MB RAM limit.

### BlockState

```rust
pub enum BlockState {
    Clean,
    Suspicious,
    Blocked { reason: String, since_ns: u64 },
}
```

State machine: Clean → Suspicious (threat rising) → Blocked (enforcement applied). Blocked stores the reason and timestamp for audit logging.

### update_ip

```rust
pub fn update_ip<R>(
    &self,
    key: IpAddr,
    default: IpRecord,
    ram_limit_bytes: usize,
    f: impl FnOnce(&mut IpRecord) -> R,
) -> (R, bool)
```

The workhorse mutation. Upserts an IpRecord and applies a closure to mutate it. Returns the closure's result and whether the entry was newly created. Used by the detection engine's batch flush path — one call per IP per batch window.

### Store::get

```rust
pub fn get(&self, key: &IpAddr) -> Option<Value>
```

Returns an `Arc` clone of the value. DashMap shard lock is held only for the lookup duration — the clone is lock-free. Benchmark: 253 ns/op at 1K entries.

## blocked_set

A `Mutex<HashSet<IpAddr>>` maintained in sync with Store mutations. Four mutation paths:

1. **Insert (Clean → new):** Add to blocked_set if value is blocked.
2. **Replace (Clean → Blocked):** Add to blocked_set.
3. **Replace (Blocked → Clean):** Remove from blocked_set.
4. **Remove:** Remove from blocked_set.

`get_all_blocked_ips()` returns a snapshot clone for the enforcement engine's polling loop.

## WAL (Write-Ahead Log)

```rust
pub struct Wal { ... }
impl Wal {
    pub fn open(path: &Path, max_segments: usize, max_segment_bytes: usize) -> Result<Self>
    pub fn append(&self, record: &WalRecord) -> Result<()>
    pub fn replay(path: &Path) -> Result<Vec<WalRecord>>
}
```

Append-only log with LZ4 compression and CRC32 checksums per record. Each segment is a file (`wal-{idx}.rshw`). When a segment exceeds `max_segment_bytes`, a new one is created and the oldest is deleted (FIFO retention).

**Replay on startup:** Sequential scan of all segments. CRC32 failures cause truncation of the corrupted tail (quarantine). Expired TTLs are skipped during replay. Idempotent: duplicate replay is safe.

**Concurrent safety:** Writes are serialized via `Mutex`. Reads during replay are lock-free (single-threaded startup).

## Subnet tracking

### SubnetKey

```rust
pub type SubnetKey = u128;

pub fn subnet_key_v4(octets: [u8; 4]) -> u32     // pack /24 into u32
pub fn subnet_key_v6(octets: [u8; 16]) -> u128   // pack /64 into u128
pub fn subnet_key_u128(ip: IpAddr) -> Option<SubnetKey>
```

`subnet_key_v4` zeros the host octet and packs the network prefix into a u32. `subnet_key_v6` zeros the host bits and returns a u128. The `subnet_index` DashMap uses these as keys for /24 (v4) or /64 (v6) aggregation.

### SubnetRecord

```rust
pub struct SubnetRecord {
    pub subnet: SubnetKey,
    pub family: u8,  // AF_INET=4, AF_INET6=6
    pub unique_ips: u32,
    pub total_events: u64,
    pub window_start_ns: u64,
    pub blocked: bool,
}
```

Tracks the dual-gate subnet swarm detection: `unique_ips ≥ 50 AND total_events ≥ 100` within a 2-second window triggers a subnet block.

## TrafficCounters

```rust
pub struct TrafficCounters {
    pub ram_limit_mb: AtomicUsize,
    pub uptime_secs: AtomicU64,
    // ... additional counters
}
```

Global atomic counters for the dashboard's health metrics. No locking — all `Relaxed` ordering.

## Dependencies

`dashmap`, `ahash`, `lz4_flex`, `crc32fast`, `serde`, `crossbeam-queue`. The WAL uses `std::fs::File` for I/O — no async runtime required.

## Tests (38)

- Store: insert/get/remove lifecycle, capacity race serialization, TTL expiry.
- blocked_set: consistency across all 4 mutation paths.
- subnet_index: cleanup on evict and remove, dual-gate enforcement.
- WAL: roundtrip, replay, corruption quarantine, segment rotation.
- BlobStore: roundtrip, large payload compression.
- Concurrent transitions: DashMap entry guard path.
