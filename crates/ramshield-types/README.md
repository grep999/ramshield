# ramshield-types

Shared domain types used across every RamShield crate. This crate is the type-level glue — it defines the events, commands, and errors that flow between the IPC layer, detection engine, storage, enforcement, and forecasting modules. Nothing here has logic; it's pure data definitions with serde derives.

## ConnectionEvent

The fundamental unit of work. Every network connection observed by the reverse proxy becomes a `ConnectionEvent` that enters RamShield through the IPC wire.

```rust
pub struct ConnectionEvent {
    pub ip: IpAddr,
    pub timestamp_ns: u64,
    pub bytes: u64,
    pub status_code: u16,
    pub proto_fingerprint: u32,
}
```

Fields:
- `ip` — source IP (v4 or v6). Parsed once at the IPC boundary; never re-parsed downstream.
- `timestamp_ns` — nanosecond-resolution arrival time. Used by batch windows and TTL expiry. The IPC layer stamps this; the client does not provide it.
- `bytes` — response body size. Feeds the bandwidth anomaly signal in the detection engine.
- `status_code` — HTTP status code bucketed into 5 categories (2xx, 3xx, 4xx, 5xx, other). Status distribution per IP is a key signal for botnet detection.
- `proto_fingerprint` — opaque protocol fingerprint (TLS JA3, HTTP/2 settings hash, etc.). Used for protocol-anomaly detection without parsing payload.

## EnforceCommand

Instruction sent from the detection/forecasting engine to the enforcement layer. Carries full audit metadata for WAL persistence and decision deduplication.

```rust
pub struct EnforceCommand {
    pub decision_id: Uuid,        // unique ID for dedup
    pub policy_version: u64,      // policy schema version
    pub source: String,           // "detection", "forecasting", "manual"
    pub actor: String,            // human-readable actor name
    pub timestamp_utc: i64,       // UTC seconds
    pub ttl_seconds: u64,         // 0 = permanent until explicit unblock
    pub reason: String,           // human-readable reason
    pub ip: IpAddr,               // target IP
    pub action: EnforceAction,    // Block or Unblock
}

pub enum EnforceAction {
    Block,
    Unblock,
}
```

The `decision_id` (UUID v4) enables the enforcement service to deduplicate commands — applying the same command twice returns `EnforceResult` with `applied: false`. This matters for WAL replay (crash recovery) where the same command may be reapplied.

## EnforceResult

Outcome of applying an `EnforceCommand`:

```rust
pub struct EnforceResult {
    pub decision_id: Uuid,
    pub committed: bool,       // WAL record written
    pub applied: bool,         // Store state changed
    pub wal_lsn: Option<u64>,  // WAL sequence number
    pub xdp_applied: bool,     // kernel XDP map updated
    pub error: Option<String>, // error message if partial failure
}
```

## EnforcementError

```rust
pub enum EnforcementError {
    Wal(String),
    Storage(String),
    Xdp(String),
    Duplicate(Uuid),
    InvalidCommand(String),
}
```

`Duplicate` is returned when a command with the same `decision_id` is applied twice. `Xdp` is non-fatal — XDP errors don't roll back storage state (fail-open design).

## Error types

```rust
pub enum RamshieldError {
    StorageFull { limit_mb: usize },
    IpcFrameTooLarge { max_bytes: usize },
    AuthFailed { reason: String },
    Expired { ip: IpAddr },
    Serialize(String),
}
```

`StorageFull` is returned when the RAM limit is hit and no evictable entries remain. `IpcFrameTooLarge` protects against memory exhaustion from malformed clients. `AuthFailed` is raised when HMAC verification fails or the nonce was already seen (replay protection). `Expired` is returned when querying an IP that was evicted by TTL.

## BlockDecision

```rust
pub struct BlockDecision {
    pub ip: IpAddr,
    pub reason: BlockReason,
    pub ttl_secs: Option<u64>,
    pub batch_subnet: Option<IpNetwork>,
}
```

Returned from the detection engine. The `batch_subnet` field is set when the block applies to an entire subnet (/24 or /64) rather than a single IP.

## BlockReason

```rust
pub enum BlockReason {
    HighRps,
    SubnetBatch,
    ForecastAnomaly,
    EntropyAnomaly,
    ManualBlock,
}
```

Has `as_str()` for stable wire tokens and `from_reason_str()` with alias expansion (e.g., `syn_flood` → `HighRps`, `anomaly` → `EntropyAnomaly`).

## IpNetwork

```rust
pub struct IpNetwork {
    pub addr: IpAddr,
    pub prefix_len: u8,
}
impl IpNetwork {
    pub fn new(addr: IpAddr, prefix_len: u8) -> Result<Self, &'static str>
    pub fn ipv4_subnet(ip: Ipv4Addr) -> Self  // /24
    pub fn ipv6_subnet(ip: Ipv6Addr) -> Self  // /64
    pub fn of_ip(ip: IpAddr) -> Self          // canonical subnet
    pub fn contains(&self, ip: IpAddr) -> bool
    pub fn pack(&self) -> u128                // for HashMap keys
    pub fn family(&self) -> u8                // 4 or 6
}
```

Represents a CIDR network. Used by the subnet tracking system to group IPs into /24 (v4) or /64 (v6) networks for swarm detection. The `pack()` method serializes the network into a `u128` for use as a DashMap key.

## Durability

```rust
pub enum Durability {
    None,
    Flush,
    Fsync,
    GroupCommit,  // default
}
```

WAL durability level. `GroupCommit` batches fsyncs for throughput; `Fsync` fsyncs every append for maximum safety.

## BoundedVecDeque

```rust
pub struct BoundedVecDeque<T> {
    pub cap: usize,
}
impl<T> BoundedVecDeque<T> {
    pub fn new(cap: usize) -> Self
    pub fn push(&mut self, item: T)  // evicts front when full
    pub fn len(&self) -> usize
    pub fn is_empty(&self) -> bool
    pub fn iter(&self) -> impl Iterator<Item = &T>
}
```

Fixed-capacity ring buffer used for batch history and block log. Zero-allocation push when not at capacity.

## RsError

```rust
pub enum RsError {
    NotFound(String),
    CapacityExceeded { limit_mb: usize },
    Serde(String),
    Io(std::io::Error),
    CorruptWal { offset: u64 },
    RecordTooLarge { size: usize, max: usize },
}
```

## Re-exports

The crate re-exports `IpAddr` and `IpNetwork` from `std::net` and its own types, making `ramshield_types` the single import point for domain types across the workspace.

## Design rationale

Separating types into their own crate breaks circular dependencies. The detection crate needs `ConnectionEvent`; the enforcement crate needs `EnforceCommand`; the protocol crate needs `ConnectionReport` (a wire-format sibling of `ConnectionEvent`). Without a shared types crate, any two of these would form a cycle. The types crate sits at the bottom of the dependency graph with zero logic — just struct definitions and serde derives.

## Dependencies

`serde` (with `derive` feature), `std::net::IpAddr`. Nothing else. This is intentionally the lightest crate in the workspace.

## Tests

- Serde round-trip for every type: serialize → deserialize → assert_eq.
- `IpNetwork` parsing: valid CIDR, invalid prefix length, IPv6 /64.
- `EnforceAction` idempotency: Block + Block = AlreadyBlocked.
- `RamshieldError` Display formatting for user-facing messages.
