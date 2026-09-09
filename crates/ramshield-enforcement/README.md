# ramshield-enforcement

```text
EnforceCommand (from detection/forecasting)
           │
           ↓
    EnforcementService::run()  ←── tokio mpsc::channel(4096)
           │
    ┌──────┴──────┐
    ↓             ↓
 Store.update   XdpApplier.apply
    │               │
    ↓               ↓
 WAL.append    XDP kernel map
    │
    ↓
 TTL ring scheduler (auto-unblock after TTL)
```

## Why it exists

Detection and forecasting produce decisions — "block this IP for 3600 seconds" or "unblock it now." But a decision sitting in memory is worthless. This crate executes those decisions: writes them to the WAL (crash safety), applies them to the Store (in-memory state), pushes them to XDP (kernel-level packet drop), and schedules automatic unblocking after the TTL expires. Without enforcement, detection is just expensive logging.

## How it works

### EnforcementService

An actor that runs on a dedicated tokio task. It receives `EnforceCommand`s through an `mpsc::channel(4096)` — large enough to buffer burst decisions during an attack without backpressure blocking detection:

```rust
pub struct EnforcementService {
    store: Arc<Store>,
    metrics: Arc<Metrics>,
    xdp: Box<dyn XdpApplier>,
    wal: Option<Arc<Wal>>,
    processed_decisions: HashSet<Uuid>,     // dedup by decision_id
    processed_order: VecDeque<Uuid>,        // LRU eviction for dedup set
    blocked_ips: HashSet<IpAddr>,           // current block state
    expirations: HashMap<IpAddr, Instant>,  // TTL expiry timestamps
    buckets: BTreeMap<Instant, Vec<IpAddr>>, // time-bucketed expiration
    epoch: Instant,
    shutdown: Arc<AtomicBool>,
}
```

### Decision Processing

Every incoming command is checked against `processed_decisions` — a `HashSet` that deduplicates by `decision_id`. This prevents the same block decision from being applied twice if detection fires multiple times in rapid succession (the `processed_order` `VecDeque` evicts the oldest entries when the set exceeds 65K).

Processing path:

1. **Block command**: `store.update_ip()` sets `BlockState::Blocked`, then `wal.append()` writes the decision, then `xdp.apply_block()` pushes to the kernel XDP map. If the XDP interface is unavailable, enforcement degrades to in-band only — the Store's `blocked_set` is still updated, and IPC responses still report blocked status.

2. **Unblock command**: `store.update_ip()` sets `BlockState::Unblocked`, `wal.append()` records the unblock, `xdp.apply_unblock()` removes from XDP, and the expiration entry is removed.

### TTL Ring

Block decisions include a `ttl_secs` field. When a block is applied, `schedule_expiration(ip, instant)` places the IP into a time-bucketed `BTreeMap<Instant, Vec<IpAddr>>`. A background loop wakes every second, checks the earliest bucket, and issues unblock commands for any IPs whose TTL has elapsed.

The ring uses `Instant::now()` (monotonic clock) — not `SystemTime` — so NTP adjustments don't cause premature or delayed unblocks.

### WAL Replay

On startup, `replay_wal_into_store()` reads the WAL and replays all block entries into the Store. It returns pairs of `(IpAddr, remaining_secs)` — IPs that were blocked at crash time with their remaining TTL. `restore_expirations()` reschedules these in the TTL ring, so blocking survives daemon restarts.

### XdpApplier Trait

The kernel XDP interface is abstracted behind a trait — enabling testing without root privileges:

```rust
pub trait XdpApplier: Send + Sync {
    fn apply_block(&self, ip: IpAddr) -> Result<(), EnforcementError>;
    fn apply_unblock(&self, ip: IpAddr) -> Result<(), EnforcementError>;
    fn reconcile(&self, blocked: &HashSet<IpAddr>) -> Result<(), EnforcementError>;
}
```

Two implementations exist:
- `StubXdpApplier`: no-op, used in tests and when XDP is disabled.
- `AyaXdpApplier`: loads the compiled BPF program from `ramshield-xdp::BPF_ELF`, attaches to a network interface via `aya`, and maintains a hash map of blocked IPs that the kernel program checks on every packet.

### Block Reasons

```rust
pub enum BlockReason {
    RateLimit,     // EWMA threshold exceeded
    SubnetBurst,   // subnet swarm gate fired
    PulseWave,     // burst-spacing correlation
    Manual,        // operator-initiated via IPC
}
```

Each reason maps to a stable string (`"rate_limit"`, `"subnet_burst"`, `"pulse_wave"`, `"manual"`) for wire protocol and Prometheus labels.

## Uniqueness

**Deduplication prevents block storms.** During a DDoS, detection may fire the same block decision hundreds of times per second. The `processed_decisions` HashSet ensures each `decision_id` is applied exactly once — preventing redundant XDP map writes and WAL appends that would consume disk and CPU.

**Time-bucketed TTL ring.** Most TTL systems scan the full expiration map every second. This one groups IPs by expiry time in a `BTreeMap` — checking only the earliest bucket. When thousands of IPs share the same TTL (common in subnet blocks), the scan is O(1) instead of O(N).

**Graceful degradation.** If XDP fails to load (wrong kernel, missing permissions), enforcement falls back to in-band mode — the Store still tracks blocks, IPC still reports them, only the kernel-level packet drop is lost. The daemon runs degraded but functional.

## Dependencies

**Reads from:** `ramshield-types` (`EnforceCommand`, `EnforceAction`, `BlockReason`, `IpNetwork`), `ramshield-storage` (`Store`, `Wal`), `ramshield-metrics` (`Metrics`), `ramshield-config` (for WAL config).

**Written by:** `ramshield-detection` (sends `EnforceCommand` via channel), `ramshield-forecasting` (sends `EnforceCommand` via channel).

**Read by:** `ramshield-dashboard` (block log), `ramshield-metrics` (block counters).

## Benchmarks

No standalone benchmarks — enforcement latency is measured end-to-end in `benches/hot_paths.rs` through the Store update path. XDP map operations are measured separately in kernel-space benchmarks (not yet implemented).

## Testing

28 tests covering: dedup by decision_id, TTL ring expiry timing, WAL replay round-trip, `StubXdpApplier` mock behavior, concurrent block+unblock race conditions, and graceful degradation when XDP is unavailable.
