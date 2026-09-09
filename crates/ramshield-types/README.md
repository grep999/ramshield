# ramshield-types

## Problem

RamShield has 9 crates that all need to share the same data structures: connection events, enforcement commands, error types, IP network abstractions. Without a shared types crate, any two crates that need to exchange data would form a circular dependency. This crate breaks that cycle by defining all shared domain types in one place with zero logic.

## How it works

Pure data definitions with serde derives. No methods beyond Display, constructors, and serde. Every other crate imports types from here instead of defining their own.

### ConnectionEvent (the fundamental unit)

```rust
pub struct ConnectionEvent {
    pub ip: IpAddr,             // source IP (parsed once at IPC boundary)
    pub timestamp_ns: u64,      // nanosecond arrival time
    pub bytes: u64,             // response body size
    pub status_code: u16,       // HTTP status (bucketed to 5 categories)
    pub proto_fingerprint: u32, // protocol fingerprint (JA3, etc.)
}
```

Every network connection observed by the reverse proxy becomes a `ConnectionEvent` that enters RamShield through the IPC wire. Parsed once, never re-parsed downstream.

### EnforceCommand (the enforcement instruction)

```rust
pub struct EnforceCommand {
    pub decision_id: Uuid,      // unique ID for WAL replay dedup
    pub policy_version: u64,    // policy schema version
    pub source: String,         // "detection", "forecasting", "manual"
    pub actor: String,          // human-readable actor name
    pub timestamp_utc: i64,     // UTC seconds
    pub ttl_seconds: u64,       // 0 = permanent until explicit unblock
    pub reason: String,         // human-readable reason
    pub ip: IpAddr,             // target IP
    pub action: EnforceAction,  // Block or Unblock
}

pub enum EnforceAction { Block, Unblock }
```

The `decision_id` (UUID v4) enables deduplication. Applying the same command twice returns `EnforceResult { applied: false }` — critical for WAL replay after crashes.

### EnforceResult (outcome of enforcement)

```rust
pub struct EnforceResult {
    pub decision_id: Uuid,
    pub committed: bool,       // WAL record written
    pub applied: bool,         // Store state changed
    pub wal_lsn: Option<u64>,  // WAL sequence number
    pub xdp_applied: bool,     // kernel XDP map updated
    pub error: Option<String>,
}
```

### BlockReason (why an IP was blocked)

```rust
pub enum BlockReason {
    HighRps,           // rate exceeded threshold
    SubnetBatch,       // /24 or /64 swarm detected
    ForecastAnomaly,   // forecasting module decided
    EntropyAnomaly,    // entropy-based detection
    ManualBlock,       // operator CLI command
}
```

Has `as_str()` for stable wire tokens and `from_reason_str()` with alias expansion (`syn_flood` → `HighRps`).

### IpNetwork (CIDR abstraction)

```rust
pub struct IpNetwork { pub addr: IpAddr, pub prefix_len: u8 }
impl IpNetwork {
    pub fn ipv4_subnet(ip: Ipv4Addr) -> Self  // /24
    pub fn ipv6_subnet(ip: Ipv6Addr) -> Self  // /64
    pub fn of_ip(ip: IpAddr) -> Self          // canonical subnet
    pub fn contains(&self, ip: IpAddr) -> bool
    pub fn pack(&self) -> u128                // for DashMap keys
}
```

Used by subnet tracking to group IPs into /24 (v4) or /64 (v6) networks.

### Error types

```rust
pub enum RsError {
    NotFound(String),
    CapacityExceeded { limit_mb: usize },
    Serde(String),
    Io(std::io::Error),
    CorruptWal { offset: u64 },
    RecordTooLarge { size: usize, max: usize },
}

pub type Result<T> = std::result::Result<T, RsError>;
```

### Durability (WAL sync levels)

```rust
pub enum Durability { None, Flush, Fsync, GroupCommit }
```

### BoundedVecDeque (fixed-capacity ring buffer)

```rust
pub struct BoundedVecDeque<T> { pub cap: usize }
impl<T> BoundedVecDeque<T> {
    pub fn push(&mut self, item: T)  // evicts front when full
}
```

Used for batch history and block log in the metrics module.

## Dependencies

```
ramshield-types
  ← thiserror, serde, uuid

Used by: ALL other RamShield crates
  → ramshield-config (Durability)
  → ramshield-detection (ConnectionEvent, EnforceCommand, BlockReason)
  → ramshield-storage (IpNetwork, BlockReason, RsError)
  → ramshield-enforcement (EnforceCommand, EnforceResult, BlockReason)
  → ramshield-forecasting (EnforceCommand)
  → ramshield-metrics (BatchRecord uses types from here)
  → ramshield-protocol (ConnectionReport)
```

This is the leaf of every dependency path. It has zero internal dependencies.

## What to read next

- Every other crate README — they all import types from here
