# RamShield Hot-Path Efficiency Audit

Scope: detection (`ramshield-detection`), storage (`ramshield-storage`), IPC server, engine. Hot path = `event → channel → pre_aggs → flush_batch → store.merge → block`.

Findings ranked by impact. Each entry: location, current state, fix, impact.

---

## HIGH IMPACT

### H1. `flush_pre_aggs_to_store` does a full `DashMap::iter()` + `clone()` of every agg
**File:** `crates/ramshield-detection/src/lib.rs:209-213`
```rust
let aggs: Vec<(IpAddr, IpAgg)> = self.pre_aggs
    .iter()
    .map(|e| (*e.key(), e.value().clone()))
    .collect();
self.pre_aggs.clear();
```
**Complexity:** O(n_unique_ips) clones, each `IpAgg` is ~56 B → ~50 MB/s allocation churn at 1M unique IPs/s. The DashMap shard locks are held only for `iter`, but every held shard lock blocks writers.

**Fix:** use `dashmap::DashMap::iter_mut()` with `std::mem::take` to drain without cloning:
```rust
let mut aggs: Vec<(IpAddr, IpAgg)> = Vec::with_capacity(self.pre_aggs.len());
for mut e in self.pre_aggs.iter_mut() {
    aggs.push((*e.key(), std::mem::take(e.value_mut())));
}
self.pre_aggs.clear();
```
Or, since pre_aggs values are `Default`, call `e.value_mut().count = 0; ...` to reset in place. **Impact: HIGH** — eliminates 1× full-value clone per IP per flush window (1ms–1s cadence).

---

### H2. `flush_batch` recomputes subnet key for every IP via two HashMap lookups
**File:** `crates/ramshield-detection/src/lib.rs:347-350`
```rust
for &(ip, ref agg) in ip_aggs {
    let subnet_hot = subnet_key(ip)
        .and_then(|(sk, _)| subnet_counts.get(&sk))
        .is_some_and(|&(ev, _)| ev as u64 >= det.subnet_window_threshold);
```
**Complexity:** O(unique_ips) calls to `subnet_key()` (which allocates a `subnet_key_v4`/`subnet_key_v6` call and builds an `IpNetwork` each time) + O(unique_ips) `subnet_counts` HashMap lookups. `IpNetwork::ipv4_subnet`/`ipv6_subnet` are not free.

**Fix:** pre-compute `subnet_key_u128(ip)` once when building `subnet_counts_of` and carry it in `IpAgg` (or in a parallel `Vec<(u128, &IpAgg)>` for the iteration). Skip the `IpNetwork` reconstruction — only the `u128` key is needed for the lookup. The `IpNetwork` is only used later in the same function (line 316) to reconstruct it again, so the second reconstruction is also redundant (see H3). **Impact: HIGH** — halves work per IP, removes a redundant `IpNetwork` construction.

---

### H3. `IpNetwork` rebuilt for every subnet in flush_batch (already-known set)
**File:** `crates/ramshield-detection/src/lib.rs:316-332`
```rust
let net = networks.get(&sk).copied().unwrap_or_else(|| {
    if sk <= 0xFFFF_FFFF {
        let o = [(sk >> 24) as u8, (sk >> 16) as u8, (sk >> 8) as u8, sk as u8];
        IpNetwork::ipv4_subnet(std::net::Ipv4Addr::from(o))
    } else {
        IpNetwork::ipv6_subnet(std::net::Ipv6Addr::from(sk))
    }
});
```
**Complexity:** O(subnets_in_flush) `IpNetwork` constructions. `IpNetwork::ipv4_subnet` allocates a new struct (no heap, but still ~24 B stack + branch) for every subnet per flush. For 1k subnets/flush at 10Hz = 10k constructions/s.

**Fix:** the `net` from the `networks` map is already correct when present (test at line 723 confirms it lands in `subnet_table`). The fallback branch runs only on the pre-agg path where `networks` is empty — in that path, build the `IpNetwork` once and inline `merge_subnet_window`'s needs. Add `IpNetwork::from_subnet_key(sk: u128) -> Self` and call it once per subnet. **Impact: MEDIUM** — measurable at >1k subnets/flush but small at typical <100.

---

### H4. `record_flush` zeros 256 atomic slots every flush
**File:** `crates/ramshield-storage/src/lib.rs:67-75`
```rust
for slot in &self.subnet_window {
    slot.store(0, Ordering::Relaxed);
}
for (i, count) in subnet_counts.iter().enumerate() {
    if i < 256 {
        self.subnet_window[i].store(*count, Ordering::Relaxed);
    }
}
```
**Complexity:** O(256) atomic stores every flush, even when only N<<256 subnets had activity. Under high subnet cardinality, two passes over the array is wasted cache traffic.

**Fix:** single pass that overwrites only the active range. Caller knows the active count:
```rust
for (i, count) in subnet_counts.iter().take(256).enumerate() {
    self.subnet_window[i].store(*count, Ordering::Relaxed);
}
if subnet_counts.len() < 256 {
    for slot in &self.subnet_window[subnet_counts.len()..] {
        slot.store(0, Ordering::Relaxed);
    }
}
```
Better: caller passes `subnet_counts.len()`; zero the tail only. **Impact: LOW-MEDIUM** — 256 atomics/flush is cheap on x86 (a single cacheline per slot, ~512 B total) but it does run on the hot path.

---

### H5. `Store::insert` calls `tracing::debug!` with format args on EVERY insert
**File:** `crates/ramshield-storage/src/lib.rs:316-356`
```rust
tracing::debug!("Store::insert - key: {}, ram_limit_bytes: {}", key, ram_limit_bytes);
tracing::debug!("Store::insert - current ram_bytes: {}, net_growth: {}", current, net_growth);
tracing::debug!("Store::insert - Successfully inserted key: {}", key);
```
**Complexity:** even with the `tracing` crate's `debug!` level filter, argument **evaluation** happens before the level check unless wrapped in `tracing::debug!`'s lazy form (which it is by default in `tracing` ≥ 0.2 — args are captured by `format_args!`, not formatted unless the level is enabled). **However**, the `key` arg uses `Display`, which for `IpAddr` does a small inline format; and there are THREE such evaluations per insert on the detection hot path (1M+ inserts/s). The `Display` for `IpAddr` is not free.

**Fix:** drop these `debug!`s entirely or gate them behind a `tracing::enabled!(tracing::Level::DEBUG)` check:
```rust
if tracing::enabled!(tracing::Level::DEBUG) {
    tracing::debug!("Store::insert - key: {}", key);
}
```
**Impact: MEDIUM** — `IpAddr` Display does 3-15 chars of formatting per call; at 1M inserts/s that's measurable.

---

## MEDIUM IMPACT

### M1. `Store::insert` → `merge_record`: per-IP `to_string()` allocations in `record_block`
**File:** `crates/ramshield-detection/src/lib.rs:401-404`
```rust
for b in &blocks {
    self.metrics
        .record_block(&b.0.to_string(), b.1.as_str(), "detection");
}
```
**Complexity:** every blocked IP gets a `String` allocation (15-39 B per IP). During a flood, blocks/sec can hit 10k+.

**Fix:** `Metrics::record_block` already allocates `String`s internally (line 285-287 in metrics). Make it take `IpAddr` directly or pass `&IpAddr`:
```rust
pub fn record_block_ip(&self, ip: &IpAddr, reason: &str, module: &str)
```
and format once. The `to_string()` then happens once in the metric instead of once at call site + once internally. Or use `write!` into a thread-local `String` buffer.

Even better: replace the `String` fields in `BlockRecord` with `IpAddr` + `&'static str` (for `module`) and only `format!` on dashboard read. **Impact: MEDIUM** — one alloc per block.

---

### M2. `merge_record` calls `subnet_key_u128(ip)` redundantly
**File:** `crates/ramshield-detection/src/lib.rs:548`
```rust
self.store.update_subnet_index(ip, subnet_key_u128(ip), false);
```
**Complexity:** `subnet_key_u128` is `#[inline]` and cheap (4-byte or 16-byte octets), but called for every promoted IP. The `subnet_key()` (line 348) was already computed for the same IP earlier in `flush_batch`. Each call costs ~5ns; 100k IPs = 500µs wasted.

**Fix:** thread the `subnet_key` through `merge_record`'s signature, or compute it once in `flush_batch` and pass `(ip, agg, sk)`. **Impact: LOW-MEDIUM.**

---

### M3. `Store::evict_batch` does a `DashMap::get` per key followed by a `remove`
**File:** `crates/ramshield-storage/src/lib.rs:367-381`
```rust
pub fn evict_batch(&self, keys: &[IpAddr]) {
    for key in keys {
        let expired = self.inner.get(key).is_some_and(|e| e.is_expired());
        if expired && let Some((_k, e)) = self.inner.remove(key) {
```
**Complexity:** 2 shard-lock acquisitions per key (one read, one write). For a batch of N expired keys, that's 2N lock round-trips. DashMap locks are sharded so contention is mild, but the redundant `get` doubles the work.

**Fix:** use `inner.remove_if` style or single-pass with `entry().or_remove_if`:
```rust
if let Some(e) = self.inner.get(key) {
    if e.is_expired() { drop(e); self.inner.remove(key); }
}
```
Or use `dashmap::DashMap::remove_if` (if available) or the `Entry` API. **Impact: MEDIUM** — eviction is not the hottest path but runs every TTL tick.

---

### M4. `WAL::append` holds `Mutex` through `fsync`
**File:** `crates/ramshield-storage/src/wal.rs:183-202`
```rust
let mut g = self.inner.lock().unwrap();
g.writer.write_all(&rh.to_bytes())?;
g.writer.write_all(&payload)?;
g.bytes += (HEADER + payload.len()) as u64;
match self.durability {
    Durability::Fsync => {
        g.writer.flush()?;
        g.writer.get_ref().sync_data()?;
    }
```
**Complexity:** std `Mutex` (not `parking_lot`) is held through a `sync_data()` syscall. `sync_data()` is 100µs–10ms (SSD/HDD). **All other WAL appends block** during this window. This is the textbook example of why WAL writers should use lock-free queues + a single dedicated writer thread, or `parking_lot::Mutex` (which at least is fair under contention).

**Fix:** (a) switch to `parking_lot::Mutex` (cheaper under low contention, fair); (b) hand the serialized record to a dedicated `std::thread` via a bounded `crossbeam_channel`, that thread owns the `Mutex` and does all I/O — callers return immediately; (c) batch fsync (group commit) with a 1–5ms timer as the comment at line 198 hints. **Impact: HIGH** — under any sustained WAL traffic, this serializes all durability work onto one syscall held under a std mutex. The whole WAL append latency is the fsync latency.

---

### M5. `WAL::replay` per-segment loop reads one byte at a time then `read_exact` for the rest
**File:** `crates/ramshield-storage/src/wal.rs:278-289`
```rust
let mut peek = [0u8; 1];
match reader.read(&mut peek) {
    Ok(0) => break,
    Ok(_) => {
        let mut hdr_buf = [0u8; HEADER];
        hdr_buf[0] = peek[0];
        if let Err(e) = reader.read_exact(&mut hdr_buf[1..]) {
```
**Complexity:** two `read` syscalls per record (one for peek, one for the rest of header). On cold storage / large WAL, this is double the syscalls. Not the hottest path (only on startup), but `read_exact` already handles EOF correctly — the peek is redundant.

**Fix:** drop the peek; call `read_exact` directly. EOF is reported as `ErrorKind::UnexpectedEof`, treat as clean EOF if the buffer is empty: **Impact: LOW** (startup only).

---

### M6. `Store::get_all_blocked_ips` iterates the entire store
**File:** `crates/ramshield-storage/src/lib.rs:466-472`
```rust
pub fn get_all_blocked_ips(&self) -> Vec<IpAddr> {
    self.inner.iter().filter(|e| e.value().value.is_blocked()).map(|e| *e.key()).collect()
}
```
**Complexity:** O(store_size) on every call. With 1M IPs and 0.1% blocked, you still scan 1M. Used by XDP reconciliation; if reconciliation runs on a timer, this is O(store) per tick.

**Fix:** maintain a `DashSet<IpAddr>` of currently-blocked IPs (insert on `Store::insert` when `block_state` becomes `Blocked`, remove on unblock). Then `get_all_blocked_ips` is O(blocked). **Impact: MEDIUM** — depends on call frequency.

---

### M7. `get_hot_subnets` formats prefixes as String per subnet per call
**File:** `src/engine/mod.rs:148-164`
```rust
let mut rows: Vec<SubnetRow> = self.store.subnet_table().iter()
    .map(|e| { ... SubnetRow { prefix: format!("{}.{}.{}", rec.prefix[0], rec.prefix[1], rec.prefix[2]), ... } })
    .collect();
rows.sort_by_key(|r| std::cmp::Reverse(r.events));
rows.truncate(100);
```
**Complexity:** O(subnets) `format!` + a full sort when you only want top-100. Also a `String` allocation per subnet. Subnet table is small (32 shards × few entries per shard), so this is bounded — but called per dashboard poll, and the sort is O(n log n) for what should be a partial selection.

**Fix:** (a) use `select_nth_unstable_by` to find the 100th element, then take prefix — O(n). (b) Cache the top-100 in a `RwLock<Vec<SubnetRow>>` updated by the subnet batch loop (every 500ms — same cadence as the snapshot). **Impact: LOW** — bounded by subnet count.

---

### M8. `Store::update_subnet_index` allocates a `DashMap` of capacity 64 per new subnet
**File:** `crates/ramshield-storage/src/lib.rs:449-453`
```rust
self.subnet_index
    .entry(sk)
    .or_insert_with(|| DashMap::with_capacity(64))
    .insert(ip_key, ());
```
**Complexity:** a `DashMap` (32 shards internally by default) allocated per subnet. With 16M /24s in IPv4 space, worst case 16M DashMap allocations. Each is ~few KB → OOM risk for an attacker spamming unique /24s.

**Fix:** use a single `DashMap<SubnetKey, Vec<IpAddr>>` with `Vec::new()`. Or use `DashSet<IpAddr>` if `dashmap` feature enabled (already imported as `DashSet<T> = DashMap<T, ()>` at line 242 — but that itself allocates a DashMap per subnet). Better: cap the per-subnet set at N=1024 entries and store as `[IpAddr; 1024] + u16 len` (bitmap-style) — fits in L1. **Impact: HIGH** — a real DoS vector if exposed; an attacker hitting 1M unique /24s allocates 1M tiny DashMaps.

---

### M9. `merge_record` allocates `BlockState::Blocked { reason: BlockReason::HighRps, ... }` and constructs `EnforceCommand` with 5 `String::from("...")` calls per block
**File:** `crates/ramshield-detection/src/lib.rs:410-419`
```rust
let cmd = EnforceCommand {
    decision_id: Uuid::new_v4(),
    policy_version: 1,
    source: "detection".into(),        // alloc
    actor: "system".into(),             // alloc
    timestamp_utc: ...,
    ttl_seconds: b.2,
    reason: b.1.as_str().into(),        // alloc
    ip: b.0,
    action: EnforceAction::Block,
};
```
**Complexity:** 3 `String` allocs per block, plus a UUID. At 10k blocks/s = 30k allocs/s, ~1MB/s garbage.

**Fix:** make `EnforceCommand` carry `&'static str` for `source`/`actor`/`reason` (it always is one of `"detection"`, `"subnet_batch"`, `"forecast"`, `"admin"`). UUIDs can be replaced with an atomic counter for monotonic IDs (the spec doesn't require UUID uniqueness across the whole system, just within an audit log). **Impact: MEDIUM** — 3 allocs × N_blocks.

---

## LOW IMPACT

### L1. Missing `#[inline]` on hot per-IP helpers
**File:** `crates/ramshield-detection/src/lib.rs`
- `status_bucket` (line 100) — `const fn`, no `#[inline]`. Used in `process_event_into_pre_aggs` (line 192) via the const table; only matters if you fall through to the slow path. Add `#[inline]`.
- `subnet_key` in `subnet.rs` and `batch.rs` — already `#[inline]`. Good.
- `cusum_step_capped`, `cusum_fired`, `ewma`, `is_exceeded` in `rate_tracker.rs:24-43` — NO `#[inline]`. These are called once per merged record (per promoted IP), at f64 arithmetic. Add `#[inline]`.
- `BloomFilter::slots` (line 48) — called twice per IP in the flush loop (line 352, 384). Add `#[inline]`.

**Impact: LOW** — these functions are 1-3 instructions, LLVM usually inlines them anyway, but explicit `#[inline]` guarantees it across crate boundaries (e.g., `cusum_*` is called from `ramshield-detection` via `use rate_tracker::*`).

---

### L2. `submit_batch` sends events one at a time through a bounded channel
**File:** `crates/ramshield-detection/src/lib.rs:168-175`
```rust
pub fn submit_batch(&self, events: Vec<ConnectionEvent>) -> Result<()> {
    for ev in events {
        self.event_tx.send(ev).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    }
    Ok(())
}
```
**Complexity:** blocking send per event. Not currently used (IPC server uses `try_send` in a loop at `server.rs:600-607`), so low impact — but the function exists and is the wrong shape for a true "submit many" entry.

**Fix:** use `crossbeam_channel::Sender::send` on a `Vec` is not possible, so the per-event send is correct. If high throughput is needed, replace with `try_send` + retry on full, and let the caller handle backpressure. Mark `#[inline(never)]` so the loop is not inlined into a hot call site. **Impact: LOW** — currently a no-op path.

---

### L3. `Store::get` clones the entire `Value`
**File:** `crates/ramshield-storage/src/lib.rs:359-365`
```rust
pub fn get(&self, key: &IpAddr) -> Option<Value> {
    let entry = self.inner.get(key)?;
    if entry.is_expired() { return None; }
    Some(entry.value.clone())
}
```
**Complexity:** for `Value::IpRecord(IpRecord { ip: IpAddr, ... ~200 B })`, this clones ~200 B plus the `Status_dist` array on every `get`. Used by `merge_record` (line 458) once per promoted IP per flush. Caller immediately mutates and re-inserts.

**Fix:** return `Option<ValueRef>` with a guard holding the shard lock, so `merge_record` can mutate in place. The `merge_record` caller is the only hot consumer. **Impact: MEDIUM** at 1M promotions/s — the per-record clone dominates merge cost.

Better: change `merge_record` to use `entry().or_insert_with()` API so it gets a single shard lock for get-modify-insert instead of get (lock+clone) + insert (lock).

---

### L4. `Store::get_stats` filters the entire inner map for blocked count
**File:** `crates/ramshield-storage/src/lib.rs:412-431`
```rust
let blocked = self.inner.iter().filter(|e| e.value().value.is_blocked()).count() as u64;
```
**Complexity:** O(store_size) per dashboard poll. If dashboard polls at 1Hz, this is 1M iterations/s of every IP to count ~1k blocked.

**Fix:** maintain `blocked_count: AtomicU64` updated on `Store::insert` when block_state changes. **Impact: MEDIUM** for high-IP-count deployments.

---

### L5. `ip_in_subnet` in batch.rs uses array equality instead of u32 compare
**File:** `crates/ramshield-detection/src/batch.rs:65-73`
```rust
pub fn ip_in_subnet(ip: IpAddr, prefix: [u8; 3]) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == prefix[0] && o[1] == prefix[1] && o[2] == prefix[2]
        }
        ...
```
**Complexity:** already O(1) but the `octets()` call copies 4 bytes. Not currently called in hot path (only tests use it). Mark `#[inline]` if kept public.

**Fix:** delete it (dead code in the hot path; only the test at line 128 uses it). **Impact: NONE** — dead code.

---

### L6. `discover_max_lsn` and `discover_max_seg` each do a fresh `read_dir` + parse
**File:** `crates/ramshield-storage/src/wal.rs:399-414, 418-461`
**Complexity:** both `discover_max_seg` (used in `Wal::open`) and `discover_max_lsn` (used in `Wal::open` too) read the same directory twice and parse filenames twice. Two iterations of `read_dir` + `filter_map` over the same files at startup.

**Fix:** do it once. Build a `Vec<(u64, PathBuf)>` from the directory, then compute max_seg and max_lsn from the same data. **Impact: LOW** — startup only.

---

### L7. `enforce_retention` re-`read_dir`s and parses filenames on EVERY `append` when retention > 0
**File:** `crates/ramshield-storage/src/wal.rs:204-223`
```rust
if g.bytes >= self.seg_max { ... }
if self.retention_max > 0 {
    enforce_retention(&self.base_dir, self.retention_max);
}
```
**Complexity:** `enforce_retention` is called on every append when `retention_max > 0`. Each call does a `read_dir` + per-entry `parse::<u64>` of the filename. For high-throughput WAL with rotation, that's `read_dir` per append — `read_dir` allocates a fresh DirEntry iterator, ~1µs each. At 10k appends/s = 10ms/s overhead.

**Fix:** call `enforce_retention` only at segment-rotation boundaries, not every append. **Impact: MEDIUM** for high WAL write rates.

---

### L8. `flush_pre_aggs_to_store` creates `HashMap::new()` for `networks` per flush
**File:** `crates/ramshield-detection/src/lib.rs:223`
```rust
let networks = HashMap::new(); // pre-agg path: merge_subnet_window derives prefix from table
```
**Complexity:** unused map — dead allocation per flush (8–64 B). Pure waste.

**Fix:** delete the line. `merge_subnet_window` doesn't take `networks`. The comment is misleading. **Impact: LOW** — 1 alloc per flush window.

---

### L9. `Metrics::record_block` does 3 `to_string()` allocations per call
**File:** `crates/ramshield-metrics/src/lib.rs:278-290`
```rust
log.push_back(BlockRecord {
    ts_ms: now_ms(),
    ip: ip.to_string(),
    reason: reason.to_string(),
    module: module.to_string(),
});
```
**Complexity:** 3 allocs per block record. Per M1 above.

**Fix:** change `BlockRecord` to hold `IpAddr` + `&'static str` for module + `Cow<'static, str>` for reason. Format on read. **Impact: MEDIUM** at 10k blocks/s.

---

### L10. `Metrics::record_batch` clones the BatchRecord into the ring
**File:** `crates/ramshield-metrics/src/lib.rs:267-275`
```rust
if let Ok(mut h) = self.batch_history.lock() {
    if h.len() >= HISTORY { h.pop_front(); }
    h.push_back(rec.clone());
}
if let Ok(mut lb) = self.last_batch.lock() { *lb = Some(rec); }
```
**Complexity:** one full `BatchRecord` clone per batch (every ~1s). `BatchRecord` is ~64 B — trivial. But the `batch_history.lock()` and `last_batch.lock()` are std `Mutex` (not `RwLock`) — every dashboard poll that reads `get_batch_history`/`get_block_log` also contends.

**Fix:** use `parking_lot::Mutex` or `RwLock<Vec<...>>` for read-heavy access. **Impact: LOW.**

---

### L11. `Metrics::get_batch_history`/`get_block_log` clone the entire deque
**File:** `crates/ramshield-metrics/src/lib.rs:309-321`
```rust
self.batch_history.lock().map(|h| h.iter().cloned().collect()).unwrap_or_default()
```
**Complexity:** O(history_size) clones per dashboard poll. With HISTORY=80, that's 80 small clones — fine. With block_log_cap=1000, 1000 clones per block-log API call — fine for the current scale.

**Fix:** return `Vec<&BatchRecord>` borrowing from the lock guard (lifetime bound). Caller can serialize without cloning. **Impact: LOW.**

---

### L12. `format!("{}.{}.{}", ...)` builds a `String` per subnet
**File:** `src/engine/mod.rs:156`
```rust
prefix: format!("{}.{}.{}", rec.prefix[0], rec.prefix[1], rec.prefix[2]),
```
**Complexity:** `String` alloc per subnet per dashboard poll. See M7.

**Fix:** keep `SubnetRow.prefix` as `[u8; 3]` (or `Ipv4Addr`) and let serialization format on demand. **Impact: LOW** — bounded by subnet count.

---

## Algorithmic observations (no O(n²) in steady state)

I did **not** find a true O(n²) algorithm in the hot path. The main suspicion would be `subnet_batch_loop` (line 571) walking the subnet table — that's O(subnets) per 500ms tick, fine. `get_ips_in_subnet` (line 458) is O(ips_in_subnet) per hot subnet, also fine.

`merge_subnet_window` (storage lib.rs:262-296) iterates `members` and calls `mark_host_v4` for each — that's O(distinct_members) per subnet per flush. With 256 hosts per /24, max 256 calls per subnet — fine.

`Store::insert` uses `DashMap::insert` which is O(1) amortized. Good.

The detection flush loop (`flush_batch`) is O(unique_ips + subnets_in_batch). Not quadratic.

---

## Lock contention analysis

| Location | Current | Recommendation | Impact |
|----------|---------|----------------|--------|
| `ramshield-storage/src/wal.rs:183` | std `Mutex<Inner>` held through `sync_data()` | switch to `parking_lot::Mutex` + dedicated writer thread (hand record via `crossbeam_channel`) | **HIGH** |
| `ramshield-metrics/src/lib.rs:196-198, 232-234` | std `Mutex<Option<BatchRecord>>`, `Mutex<VecDeque<BatchRecord>>`, `Mutex<VecDeque<BlockRecord>>` | `parking_lot::RwLock` (read-heavy: dashboard reads) | LOW |
| `ramshield-metrics/src/lib.rs:21, 34` | static `Mutex<Option<System>>` and cache `Mutex` for `get_system_usage` | once-per-second cache, contention is bounded; OK as is | NONE |
| `src/engine/mod.rs:26` | `Mutex<Option<mpsc::Receiver<EnforceCommand>>>` for one-time take | single-use, never held; OK | NONE |
| `ramshield-detection/src/lib.rs:128` | `RwLock<BloomFilter>` — write per block | Reads are dominant (every IP), writes are rare. `RwLock` is correct; parking_lot faster. Switch to `parking_lot::RwLock`. | MEDIUM |
| `ramshield-storage/src/blob_store.rs:14` | std `Mutex<Inner>` (File + cursor) | `parking_lot::Mutex`; read path returns a `Vec<u8>` anyway so contention is per-request | LOW |
| `ramshield-storage/src/atomic_ops.rs:23` | `dashmap::Entry` API, no separate lock | Already shard-locked via DashMap; correct | NONE |

---

## Summary table

| ID | File:line | Complexity | Fix | Impact |
|----|-----------|-----------|-----|--------|
| H1 | detection/lib.rs:209 | O(n) clones per flush | `iter_mut` + `mem::take` | HIGH |
| H2 | detection/lib.rs:347 | O(n) subnet_key + HashMap lookup per IP | pre-compute `sk` in `subnet_counts_of` | HIGH |
| H3 | detection/lib.rs:316 | O(subnets) `IpNetwork` ctor | one ctor per subnet; store in `subnet_counts` | MEDIUM |
| H4 | storage/lib.rs:67 | O(256) atomic stores | zero only the tail | LOW-MED |
| H5 | storage/lib.rs:316 | per-insert `Display` | drop or gate `debug!` | MEDIUM |
| M1 | detection/lib.rs:401 | 1 alloc per block | `record_block` takes `IpAddr` | MEDIUM |
| M2 | detection/lib.rs:548 | redundant `subnet_key_u128` | pass through `merge_record` | LOW-MED |
| M3 | storage/lib.rs:367 | 2N shard locks | single-pass with `Entry` | MEDIUM |
| M4 | storage/wal.rs:183 | std Mutex held through fsync | dedicated writer thread | **HIGH** |
| M5 | storage/wal.rs:278 | 2 reads per record | drop peek | LOW |
| M6 | storage/lib.rs:466 | O(store) per call | maintain `blocked_set` | MEDIUM |
| M7 | engine/mod.rs:148 | O(n log n) sort for top-100 | `select_nth_unstable` or cache | LOW |
| M8 | storage/lib.rs:449 | 1 DashMap alloc per subnet | `Vec<IpAddr>` or bounded array | **HIGH** (DoS) |
| M9 | detection/lib.rs:410 | 3 String allocs per block | `&'static str` fields in EnforceCommand | MEDIUM |
| L1 | multiple | missing `#[inline]` | add to `cusum_*`, `BloomFilter::slots` | LOW |
| L2 | detection/lib.rs:168 | one send per event | mark `#[inline(never)]`; correct as-is | LOW |
| L3 | storage/lib.rs:359 | clone full `Value` per get | return guard; `entry()` API | MEDIUM |
| L4 | storage/lib.rs:414 | O(store) per dashboard poll | `AtomicU64` counter | MEDIUM |
| L5 | detection/batch.rs:65 | dead code | delete | NONE |
| L6 | storage/wal.rs:399,418 | double `read_dir` | do it once | LOW |
| L7 | storage/wal.rs:222 | `read_dir` per append | call only at rotation | MEDIUM |
| L8 | detection/lib.rs:223 | dead `HashMap::new()` | delete | LOW |
| L9 | metrics/lib.rs:278 | 3 allocs per block | `IpAddr` + `Cow` fields | MEDIUM |
| L10 | metrics/lib.rs:196 | std Mutex on hot read path | `parking_lot::RwLock` | LOW |
| L11 | metrics/lib.rs:309 | full clone per call | borrow iterator | LOW |
| L12 | engine/mod.rs:156 | `String` per subnet | `[u8; 3]` | LOW |

## Top 5 by effort-adjusted impact

1. **M4 (WAL mutex through fsync)** — one-line `parking_lot::Mutex` swap gives immediate latency improvement under WAL load.
2. **H1 (`flush_pre_aggs_to_store` clone)** — `iter_mut` + `mem::take`; 5-line change; eliminates 1 alloc per unique IP per flush.
3. **M8 (DashMap per subnet in `subnet_index`)** — real DoS vector; swap to `Vec<IpAddr>` or bounded set.
4. **H2 (redundant `subnet_key`)** — carry the `u128` key through `IpAgg`; one-line struct change.
5. **L4 + M6 (counters for blocked)** — replace 2 O(store) scans with `AtomicU64` increments; cheap, high-leverage.

No changes were made to source files. This is an audit only.
