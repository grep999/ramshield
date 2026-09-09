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

Instruction sent from the detection/forecasting engine to the enforcement layer.

```rust
pub enum EnforceCommand {
    Block {
        ip: IpAddr,
        reason: String,
        ttl_secs: Option<u64>,
    },
    Unblock {
        ip: IpAddr,
    },
}
```

`Block` carries a reason string (logged and returned in IPC responses) and an optional TTL. `None` TTL means permanent block until explicit unblock. The enforcement engine writes this to the WAL and applies it to the Store and optionally to XDP maps.

## EnforceAction

The result of applying an `EnforceCommand` — returned to the caller (typically the detection engine or IPC handler) to confirm what happened.

```rust
pub enum EnforceAction {
    Blocked { ip: IpAddr, ttl_secs: Option<u64> },
    Unblocked { ip: IpAddr },
    AlreadyBlocked { ip: IpAddr },
    AlreadyUnblocked { ip: IpAddr },
}
```

Idempotent: applying the same block twice returns `AlreadyBlocked` instead of double-counting. This matters for WAL replay (crash recovery) where the same command may be applied multiple times.

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
pub enum BlockDecision {
    Blocked { reason: String, ttl_secs: Option<u64> },
    AlreadyBlocked,
    Failed { reason: String },
}
```

Returned from the Store's block path. `Failed` is currently unused but reserves space for future error conditions (e.g., WAL write failure).

## IpNetwork

```rust
pub struct IpNetwork {
    pub addr: IpAddr,
    pub prefix_len: u8,
}
```

Represents a CIDR network (e.g., `10.0.0.0/8`). Used by the subnet tracking system to group IPs into /24 (v4) or /64 (v6) networks for swarm detection. The `prefix_len` determines the grouping granularity.

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
