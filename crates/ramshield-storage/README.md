# ramshield-storage

Sharded in-memory key-value store with WAL durability, blob store, and subnet tracking.

## Architecture

```
Store (DashMap<IpAddr, Value>)
    ├── blocked_set: HashSet<IpAddr> — reverse index for enforcement
    ├── subnet_index: DashMap<SubnetKey, SubnetRecord> — /24 or /64 aggregation
    ├── subnet_windows: DashMap<IpAddr, AtomicSubnetWindow> — 2h rolling windows
    ├── ram_bytes: AtomicU64 — memory pressure tracking
    └── blob_store: BlobStore — compressed large payloads

WAL (BufWriter append)
    ├── WAL segment files: wal-{idx}.rshw (LZ4 compressed, CRC32 checksummed)
    ├── LSN (Log Sequence Number) monotonically increasing
    ├── Replay on startup with corruption truncation
    └── Retention: max 10 segments, oldest deleted on rotation
```

## Key Components

### Store
- DashMap with configurable shard count (power of 2, default 16)
- `insert()`: RAM-aware insertion with capacity enforcement
  - New entries: check ram_bytes + heap_bytes delta ≤ ram_limit_mb
  - Replacements: allowed without capacity check (net-zero delta)
- `remove()`: free heap, update ram_bytes, remove from blocked_set + subnet_index
- `get()`: returns Arc clone (DashMap shard lock held briefly)
- `evict_batch()`: bulk eviction of oldest entries by TTL expiry
  - Updates blocked_set index, subnet_index, ram_bytes atomically
  - Concurrent eviction races on subnet_index are safe (no-op on missing)

### blocked_set
- HashSet<IpAddr> maintained in sync with Store mutations
- 4 mutation paths: insert (clean→new), replace (clean→blocked), replace (blocked→clean), remove
- `get_all_blocked_ips()`: returns snapshot for enforcement polling

### WAL
- Append-only log with LZ4 compression + CRC32 checksums
- Segment rotation at configurable size threshold
- Replay: sequential scan, truncate on corruption (quarantine bad tail)
- Concurrent-safe: Mutex for writes, lock-free reads during replay

### BlobStore
- Compressed storage for large payloads (SubnetRecord thresholds)
- LZ4 compression, offset-based addressing
- Inline for small payloads, file-backed for large

### SubnetKey helpers
- `subnet_key_v4()`: pack IPv4 /24 into u32 (low 32 bits, host octet zeroed)
- `subnet_key_v6()`: pack IPv6 /64 into u128 (host bits zeroed)
- `subnet_key_u128()`: universal wrapper returning Option<SubnetKey>

## 17 tests
- insert/get/remove lifecycle, capacity race serialization
- blocked_set consistency across 4 mutation paths
- subnet_index cleanup on evict and remove
- WAL: roundtrip, replay, corruption quarantine, segment rotation
- BlobStore: roundtrip, large payload compression
- Concurrent transitions: DashMap entry guard path
