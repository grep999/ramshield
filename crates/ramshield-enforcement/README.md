# ramshield-enforcement

IP blocking enforcement with WAL-backed persistence and optional XDP integration. This crate is the bridge between detection decisions and actual traffic blocking — it takes `EnforceCommand` values from the detection/forecasting engine and applies them to the Store, WAL, and (optionally) kernel-level XDP maps.

## Architecture

```
EnforceCommand (from Forecaster/Engine)
        │
        ▼
EnforcementEngine::apply(command)
        ├── Store::update_ip() — set block_state = Blocked
        ├── Store::blocked_set — add/remove from reverse index
        ├── Wal::append() — persist for crash recovery
        └── XdpEnforcer (optional) — update kernel BPF maps
                │
                ▼
        XDP program drops packets from blocked IPs
```

## EnforcementEngine

```rust
pub struct EnforcementEngine {
    store: Arc<Store>,
    wal: Arc<Wal>,
    metrics: Arc<Metrics>,
    xdp: Option<XdpEnforcer>,  // None when XDP is disabled
}
impl EnforcementEngine {
    pub fn new(store: Arc<Store>, wal: Arc<Wal>, metrics: Arc<Metrics>, xdp: Option<XdpEnforcer>) -> Self
    pub fn apply(&self, cmd: EnforceCommand) -> EnforceAction
    pub fn replay_wal(&self) -> Result<()>
}
```

### apply()

The central method. Takes an `EnforceCommand` and returns an `EnforceAction`.

**Block path:**
1. Check if IP is already blocked → return `AlreadyBlocked`.
2. `store.update_ip()`: set `block_state = Blocked { reason, since_ns }`.
3. Add to `store.blocked_set`.
4. `wal.append()`: write WAL record for crash recovery.
5. If XDP enabled: `xdp.block(ip)`.
6. `metrics.record_block()`: log the block for the dashboard.
7. Return `Blocked { ip, ttl_secs }`.

**Unblock path:**
1. Check if IP is not blocked → return `AlreadyUnblocked`.
2. `store.update_ip()`: set `block_state = Clean`.
3. Remove from `store.blocked_set`.
4. `wal.append()`: write WAL record.
5. If XDP enabled: `xdp.unblock(ip)`.
6. Return `Unblocked { ip }`.

### replay_wal()

Called once at startup. Reads all WAL records in order and applies them:
- Block records: re-apply the block (skip if TTL expired).
- Unblock records: remove the block.

This ensures that blocks survive process restarts. WAL replay is idempotent — applying the same block twice returns `AlreadyBlocked` which is safely ignored.

## XdpEnforcer

```rust
pub struct XdpEnforcer { ... }
impl XdpEnforcer {
    pub fn new(interface: &str, mode: XdpMode) -> Result<Self>
    pub fn block(&self, ip: IpAddr) -> Result<()>
    pub fn unblock(&self, ip: IpAddr) -> Result<()>
    pub fn reload_program(&self) -> Result<()>
}
```

Wraps the `ramshield-xdp` crate's eBPF program. Updates the `BLOCKED_IPS` BPF hashmap from user space. The XDP program in kernel space reads this map and drops packets from blocked IPs.

**Modes:**
- `Skb` (socket buffer): works on all NICs, uses the generic network stack.
- `Drv` (driver): native NIC driver support, higher performance.
- `Offload`: NIC hardware offload, fastest but limited NIC support.

**Capabilities required:** `cap_net_admin`, `cap_bpf`, `cap_perfmon`. Without these, XDP attachment fails gracefully — the enforcement engine falls back to Store-only blocking (no kernel-level drops).

## TTL management

Blocked IPs can have an optional TTL (time-to-live). When the TTL expires:

1. **Lazy expiry:** `Store::get()` and `Store::update_ip()` check `last_seen_ns` against the TTL on every access. Expired blocks are silently cleaned up.
2. **Periodic sweep:** The engine's main loop calls `evict_batch()` periodically, which removes all expired entries in bulk.

The lazy + periodic approach avoids a dedicated expiry thread while ensuring blocks don't persist indefinitely.

## WAL record format

```rust
pub enum WalRecord {
    Block { ip: IpAddr, reason: String, ttl_secs: Option<u64>, ts_ns: u64 },
    Unblock { ip: IpAddr, ts_ns: u64 },
}
```

Serialized as JSON, compressed with LZ4, checksummed with CRC32. Each record is ~100 bytes uncompressed, ~40 bytes compressed. At 1 block/second, a 10-segment WAL with 10MB segments holds ~7 days of block history.

## Thread safety

`EnforcementEngine` is `Send + Sync`. The `apply()` method takes `&self` (not `&mut self`) — concurrent enforcement commands are serialized by the Store's DashMap and the WAL's Mutex. In practice, enforcement commands arrive one at a time from the detection engine's batch flush loop.

## Data flow

```
Detection engine tick (50ms)
    → Detects threshold breach
    → Creates EnforceCommand::Block { ip, reason, ttl }
    → Sends via crossbeam channel
    → EnforcementEngine::apply()
        → Store updated
        → WAL persisted
        → XDP map updated (if enabled)
        → Metrics recorded
    → Dashboard shows new block in /api/history/blocks
```

## Dependencies

`ramshield-types` (EnforceCommand, EnforceAction), `ramshield-storage` (Store, Wal), `ramshield-metrics` (Metrics), `ramshield-xdp` (optional, behind `xdp` feature flag).

## Tests

- WAL roundtrip: block → WAL write → restart simulation → replay → block active.
- Unblock cancels block: WAL records cancel out correctly.
- Idempotency: double-block returns AlreadyBlocked, double-unblock returns AlreadyUnblocked.
- TTL expiry: block with short TTL expires correctly.
- XDP integration: mock XDP enforcer verifies block/unblock map updates.
