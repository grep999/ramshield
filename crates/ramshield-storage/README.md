# ramshield-storage

The per-IP and per-subnet state of the running daemon, in one place: threat
scores, block state, detection counters, subnet membership - plus the
write-ahead log that makes the block decisions survive a crash.

```text
                    ┌───────────────────────────────┐
                    │          Store                  │
                    │  ┌─────────────┐ ┌───────────┐ │
Detection ─→ update_ip   │  │ IpEntryMap  │ │ SubnetMap │ │
                    │  │ DashMap<IP> │ │ DashMap<SN>│ │
Forecaster ─→ read counters  └─────────────┘ └───────────┘ │
Enforcement ←── get  │  ┌─────────────┐ ┌───────────┐ │
IPC handlers ←── query  │  │ BlockedSet  │ │ SubnetIdx │ │
                    │  │ DashSet<IP> │ │ DashMap<SN>│ │
                    │  └─────────────┘ └───────────┘ │
                    │  TrafficCounters (atomics)      │
                    └───────────────────────────────┘
                          ↕
                    Wal (crash recovery)
```

## Why it exists

Several subsystems need per-IP state, and none of them should own it. This
crate holds it in one sharded, RAM-budgeted map, and writes the block
decisions to a write-ahead log so a crash does not lose them.

Detection writes the counters, enforcement reads and writes the block
state, forecasting reads the traffic counters, and the IPC layer queries
the store. None of them keeps its own copy.

## How it works

### The sharded IP map

`Store` wraps a `DashMap<IpAddr, Entry>`:

```rust
pub struct Store {
    inner: Arc<IpEntryMap>,          // DashMap<IpAddr, Entry>
    subnet_table: Arc<SubnetTable>,  // DashMap<SubnetKey, SubnetRecord>
    subnet_index: Arc<SubnetIndex>,  // DashMap<SubnetKey, HashSet<IpAddr>>
    blocked_set: Arc<DashSet<IpAddr>>,
    pub active_cidrs: Arc<DashSet<IpNetwork>>,
    pub traffic: Arc<TrafficCounters>,
    pub total_inserts: Arc<AtomicU64>,
    pub total_evictions: Arc<AtomicU64>,
    // ...ram_bytes, blocked_count, ttl_entries
}
```

The shard count is a parameter of `Store::new`, rounded up to a power of
two; the configuration default is `engine.shard_count`, 256. Each shard has
its own lock, so operations on keys that land in *different* shards never
contend - two different keys can still land in the same shard, because the
shard is chosen by hash.

`active_cidrs` is the set of enforced network prefixes. It is written only
by the enforcement actor, because a member of a blocked /24 has no
`IpRecord.block_state` of its own - the CIDR is the decision and the
members are views of it, so per-IP state alone would report `blocked:
false` for an IP the dataplane is dropping.

### IpRecord - the per-IP state

```rust
pub struct IpRecord {
    pub ip: IpAddr,
    pub request_count: u64,
    pub ewma_rps: f64,
    pub cusum_s: f64,
    pub baseline_rps: f64,
    pub prev_sample_hot: bool,
    pub sample_count: u8,
    pub relative_breach_streak: u8,
    pub pulse_samples_in_window: u8,
    pub pulse_window_start_ns: u64,
    pub adaptive_baseline_rps: f64,
    pub adaptive_cusum_s: f64,
    pub adaptive_model_sample_count: u64,
    pub adaptive_sample_count: u64,
    pub variance: f64,
    pub last_bucket_ns: u64,
    pub recovery_count: u64,
    pub detector_mode: u8,
    pub adaptive_bucket_id: u64,
    pub adaptive_bucket_packets: u64,
    pub adaptive_bucket_initialized: bool,
    pub first_seen_ns: u64,
    pub last_seen_ns: u64,
    pub bytes_in: u64,
    pub status_dist: [u32; 5],
    pub proto_fingerprint: u32,
    pub threat_score: f32,
    pub block_state: BlockState,
}
```

`threat_score` is written by detection's flush, as a blend of the RPS
score and the error rate. The CUSUM accumulator is a different field,
`cusum_s`, and the slow EWMA it measures deviation from is `baseline_rps`.
`status_dist` is a per-status-code counter array, not a history ring.

`Entry` wraps a `Value` - which may be an `IpRecord` - plus an optional
expiry:

```rust
pub struct Entry {
    pub value: Value,
    pub expires_at: Option<Instant>,
}
```

`Value::from_bytes` stores a payload of 64 bytes or fewer inline and a
larger one as a blob. The threshold is bytes, not events.

### Who writes what

Three mutators, and each one owns one part of the record:

- `update_ip` mutates the stat fields in place, under one shard lock. It
  is the detection path, and by contract it never touches `block_state`.
- `block_ip` and `unblock_ip` change the block state and nothing else,
  each under one shard lock. They are the enforcement path, and they keep
  the blocked index in step with the record under that same lock.

That split is what stops a detection flush and a block from rolling each
other's fields back.

### Subnet tracking

Two parallel maps support subnet-level detection:

- `SubnetTable`: maps a `/24` (IPv4) or `/64` (IPv6) key to a
  `SubnetRecord` - the network, the total request rate, a 256-bit bitmap
  of the host octets seen in the window, and when it was last updated.
  The distinct-source count is derived from that bitmap
  (`unique_ips()`), and the CIDR string is rendered from the network on
  demand.
- `SubnetIndex`: the reverse index, mapping a subnet key to the set of IPs
  seen in that subnet. The set is a plain `HashSet`, not a `DashSet` -
  the index is written by the flush and read for counts.

The subnet key is a `u128`: IPv4 networks occupy the low 32 bits and IPv6
`/64` networks the upper 64 bits. The prefix length is **not** packed into
it - it is carried separately in the `IpNetwork`.
`subnet_key_v4(octets)` masks the `/24` and returns a `u32`;
`subnet_key_v6(octets)` zeroes the lower 64 bits and returns the full
128-bit value.

### The RAM budget

The store tracks its own usage and refuses to grow past it. When a net-new
key would push the store past `ram_limit_mb` (configuration
`engine.ram_limit_mb`, default 512 MB), the insert is **refused** with
`RsError::CapacityExceeded` - the store does not evict live entries to
make room. The only removals are a TTL that has passed (`evict_expired`)
and an explicit `evict_batch` or `remove`.

The accounting is byte-precise rather than an estimate: an entry costs
`size_of::<IpAddr>() + size_of::<Entry>() + value.heap_bytes()`. An
`IpRecord` adds no heap bytes - it lives inside the `Value` enum.

An IP the store refused is dropped from detection too, so its counters
restart; the refusal is observable through
`capacity_exceeded_count` and `capacity_exceeded_ips`.

### The write-ahead log

`Wal` is a handle over the `ramwal` crate's log, and the knobs - the
directory, compression, durability, segment size and the retention cap -
live in `ramwal::Config`:

```rust
pub struct Wal {
    guard: Guard,
}
```

The log holds `WalEntry` values, not enforcement commands:

```rust
pub enum WalEntry {
    BlockIp { ip, reason, ttl_secs, ts_ns },
    UnblockIp { ip, ts_ns },
    BlockCidr { cidr, reason, ttl_secs, ts_ns },
    UnblockCidr { cidr, ts_ns },
    Insert { key, value_json, ttl_secs, ts_ns },
    Delete { key, ts_ns },
    MeshMutation { .. },
    Checkpoint { snapshot_path, ts_ns },
}
```

An enforcement decision is translated into one of those and appended
**before** the store mutation and the XDP projection, in that order.

The four durability modes are `None` (buffered - a block can be lost on
power failure), `Flush` (an explicit `sync()`), `Fsync` (a barrier on
every append) and `GroupCommit` (batched barriers, the default). The
configuration default is `Flush`.

`Wal::replay()` and `Wal::replay_from(min_lsn)` are read-only.
`Wal::replay_dir(dir)` is the boot-time **repairing** entry point: it
truncates a torn tail and quarantines an unusable segment, so it must not
be pointed at a directory that is being written. The engine calls
`ramshield_enforcement::replay_wal_into_store_seeded(...)` at boot, which
folds the WAL tail into the store's blocked set, and the enforcement run
loop then reconciles the XDP map from `get_all_blocked_ips()`.

### TrafficCounters

```rust
pub struct TrafficCounters {
    pub events_last_second: AtomicU64,
    pub unique_ips_window: AtomicU64,
    pub promoted_ips: AtomicU64,
    pub subnet_window: [AtomicU64; 256],
    pub threat_sample: SegQueue<(IpAddr, f32)>,
    pub ram_limit_mb: AtomicUsize,
    pub used_bytes: AtomicU64,
    pub uptime_secs: AtomicU64,
}
```

No locks in the hot path - the counters are atomics and the threat-sample
queue is a lock-free `SegQueue`, not a mutex. Detection pushes a sample
on each flush and the forecaster drains the queue. The Prometheus surface
lives in `ramshield-metrics`, which does not depend on this crate.

## Dependencies

**Dependencies:** `ramshield-types`, `ahash`, `dashmap`, `serde`, `serde_json`, `tracing`, `crossbeam-queue`, `ramwal`. Dev-dependencies: `tempfile`, `proptest`.

**Reads from:** `ramshield-types` (`IpNetwork`, `BlockReason`,
`Durability`, `Result`, `RsError`).

**Written by:** `ramshield-detection` (`update_ip`, `merge_subnet_window`,
`update_subnet_index`, `evict_expired`), `ramshield-enforcement`
(`block_ip`, `unblock_ip`, and the WAL appends).

**Read by:** `ramshield`, `ramshield-detection`, `ramshield-enforcement`, `ramshield-forecasting`. The others are `ramshield-enforcement` (`get`, `get_all_blocked_ips`, `is_blocked_by_cidr`, `active_cidrs`), `ramshield-forecasting` (the traffic counters) and the root package's IPC handlers (`get`, `get_stats`).

## Benchmarks

The workspace has one benchmark binary, and it takes no name filter:

```bash
cargo bench --bench hot_paths --features full
```

The storage benchmarks in it are `subnet_key_v4`, `subnet_key_v6`,
`Store::get (1K entries)` and `Store::update_ip (1K entries)`.

## Testing

52 tests: shard-level concurrency (8 threads inserting at once), the RAM
limit (an insert past the limit is refused, and no phantom entry or
blocked-index entry is left behind), the blocked index staying in step
with the records (a blocked record published through `update_ip` is
counted and indexed, and a removal concurrent with a re-block does not
lose the index entry), WAL append and replay round-trips, subnet index
consistency (insert an IP, verify the index, remove the IP, verify the
index is cleaned up), and TTL expiry timing. `tests/fuzz.rs` is a
proptest over the WAL segment, and `tests/wal_concurrency.rs` races
appends.

`crates/ramshield-storage/tests/readme_claims.rs` checks this file
against the crate: the dependency list, the list of crates that read it,
the types documented here, and the test count above.


### Adaptive detector state

The adaptive CUSUM detector runs alongside the existing EWMA, debounce, pulse, and
relative-baseline gates. Its state is stored directly in each `IpRecord` so updates
survive successive flushes without a separate unbounded map. The legacy CUSUM fields
remain independent to preserve existing detection semantics during this prototype.

`detector_mode` is persisted as `0 = WarmingUp`, `1 = Monitoring`, and
`2 = Breached`. Adaptive updates are accepted only when the sample rate and
monotonic timestamp are valid and all derived arithmetic remains finite. Rejected
updates leave the adaptive fields unchanged. Forecast-context modulation is exposed
by the detector API, but the current `merge_record` call site does not yet provide
a forecast context, so the runtime path uses the conservative default profile.


The adaptive variance field has a positive Serde migration default (`0.5`, matching
the default detector profile's variance floor). Older serialized records also start
with an uninitialized adaptive bucket. The first observed second is accumulated and
submitted only when the next bucket begins; partial flushes are not fed directly to
adaptive CUSUM.
