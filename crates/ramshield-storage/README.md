# ramshield-storage

## Problem

Every detection decision needs to reference per-IP history: "What's this IP's current threat score? Is it already blocked? When did we first see it?" A naive approach (reading from disk on every query) would be too slow. But storing everything in memory risks OOM under load. The storage module solves this with a RAM-bounded, sharded in-memory store backed by a WAL for crash recovery.

Without this module, blocks are lost on restart. Without RAM limits, a flood of unique IPs would exhaust memory.

## How it works

### Store (DashMap-backed)

```rust
pub struct Store {
    pub inner: DashMap<IpAddr, Entry, ahash::RandomState>,
    pub blocked_set: DashSet<IpAddr>,           // reverse index
    pub subnet_index: DashMap<SubnetKey, DashSet<IpAddr>>,
    pub ram_bytes: AtomicU64,                   // live memory tracking
    pub traffic: TrafficCounters,               // lock-free atomic counters
}
```

DashMap with configurable shard count (power of 2). Each shard is a separate RwLock — concurrent reads don't block each other. `ahash` provides collision-resistant hashing (no HashDoS).

### RAM-aware insertion

Every `insert()` checks `ram_bytes + heap_delta ≤ ram_limit_bytes`. If exceeded, the insert is rejected with `StorageFull`. Replacements are allowed (net-zero delta). Eviction is lazy: `evict_batch()` scans for expired TTLs and frees memory in bulk.

### Per-IP records

```rust
pub struct IpRecord {
    pub ip: IpAddr,
    pub request_count: u64,
    pub ewma_rps: f64,
    pub cusum_s: f64,
    pub baseline_rps: f64,
    pub prev_sample_hot: bool,
    pub sample_count: u8,
    pub pulse_samples_in_window: u8,
    pub pulse_window_start_ns: u64,
    pub first_seen_ns: u64,
    pub last_seen_ns: u64,
    pub bytes_in: u64,
    pub status_dist: [u32; 5],
    pub proto_fingerprint: u32,
    pub threat_score: f32,
    pub block_state: BlockState,
}
```

~150 bytes per IP. At 100K tracked IPs = 15 MB. The detection engine reads and writes these on every batch flush.

### WAL (Write-Ahead Log)

Append-only log for crash recovery:

```
wal-00000000.rshw  (segment file)
├── 23-byte header: magic + version + LSN + payload_len + CRC32 + flags
├── LZ4-compressed payload
└── CRC32 checksum
```

**Durability modes:**
- `None` — no sync (fastest, crash-unsafe)
- `Flush` — flush OS buffer
- `Fsync` — full disk sync per append
- `GroupCommit` — batch fsyncs (default, best throughput/safety tradeoff)

**Replay on startup:** Sequential scan, truncate on corruption (quarantine bad tail). Expired TTLs skipped. Idempotent — duplicate replay is safe.

### Subnet tracking

```rust
pub fn subnet_key_v4(octets: [u8; 4]) -> u32   // pack /24 into u32
pub fn subnet_key_v6(octets: [u8; 16]) -> u128  // pack /64 into u128
```

The `subnet_index` maps subnet keys to sets of member IPs. The detection engine uses this for swarm detection (50+ unique IPs in a /24 = block the whole subnet).

### blocked_set

A `DashSet<IpAddr>` maintained in sync with Store mutations. Makes `get_all_blocked_ips()` O(blocked) instead of O(all). The enforcement engine polls this for XDP reconciliation.

## Dependencies

```
ramshield-storage
  ← ramshield-types (BlockReason, IpNetwork, Durability, RsError)
  ← dashmap, ahash, lz4_flex, crc32fast, crossbeam-queue, serde, tokio

Used by:
  → ramshield-detection (read/write IpRecords, subnet tracking)
  → ramshield-enforcement (read blocked_set, write WAL)
  → ramshield-forecasting (read TrafficCounters)
  → ramshield-metrics (read store stats for dashboard)
  → src/engine (create Store at boot)
```

This is the shared state layer. Every other module reads from or writes to the Store.

## Key benchmarks

| Function | ns/op | ops/sec |
|----------|-------|---------|
| subnet_key_v4 | 249 | 4.0M |
| Store::get (1K entries) | 253 | 3.9M |
| Store::update_ip (1K entries) | 267 | 3.7M |

## What to read next

- `crates/ramshield-detection/` — reads and writes IpRecords through this module
- `crates/ramshield-enforcement/` — writes WAL records and updates blocked_set
- `crates/ramshield-forecasting/` — reads TrafficCounters for anomaly detection
