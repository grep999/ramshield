# ramshield-types

The shared vocabulary of the workspace: the structs and enums that several
crates agree on, the error types the storage and enforcement crates speak,
and the CIDR abstraction the detection and storage crates key their tables
by. It is not the whole boundary vocabulary - `ConnectionReport` lives in
`ramshield-protocol`, `IpRecord` in `ramshield-storage`, and
`DashboardSnapshot` in `ramshield-metrics`.

```text
┌──────────────────────────────────────────────┐
│              ramshield-types                  │
│                                              │
│  ConnectionEvent    EnforceCommand           │
│  BlockDecision      EnforceResult            │
│  EnforceAction      EnforcementError         │
│  BlockReason        RsError                  │
│  IpNetwork          Durability               │
│  Result<T>          EvidenceSource           │
└───────────┬──────────────────┬───────────────┘
            │                  │
     ┌──────┴──────┐   ┌──────┴──────┐
     │  detection   │   │  enforcement  │
     │  storage     │   │  protocol     │
     │  forecasting │   │  root package │
     └─────────────┘   └─────────────┘
```

## Why it exists

Seven crates in the workspace - `ramshield-detection`,
`ramshield-storage`, `ramshield-enforcement`, `ramshield-forecasting`,
`ramshield-protocol`, `ramshield-config` and the root `ramshield`
package - need the same data structures: connection events, enforcement
commands, error types, IP network abstractions.
Without a shared type crate, each would define its own versions, and the
conversion boilerplate and the drift come with that.

The crate is not a pure type crate. It carries logic that would otherwise
be copied into every consumer:

- `src/error.rs` - the `BlockReason` wire vocabulary and its reverse
  mapping, plus the reason strings the IPC and WAL layers share.
- `src/ip_network.rs` - CIDR normalisation and prefix-length validation.
  `IpNetwork::new` masks the host bits and rejects a prefix that the
  address family cannot represent, and deserialization goes through it,
  so a hand-crafted frame cannot inject an out-of-range prefix.
- `src/util.rs` - the fixed-capacity ring buffer.

## What it defines

### ConnectionEvent - the fundamental data unit

```rust
pub struct ConnectionEvent {
    pub ip: IpAddr,
    pub timestamp_ns: u64,
    pub bytes: u64,
    pub status_code: u16,
    pub proto_fingerprint: u32,
    pub l7: Option<L7Metadata>,
}
```

The `l7` field is optional and defaults to `None`, which keeps the
legacy L3/L4 contract wire-compatible for producers that do not fill it.

### L7Metadata - optional application-layer telemetry

```rust
pub struct L7Metadata {
    pub host_hash: u64,
    pub route_hash: u64,
    pub method: HttpMethod,
    pub version: HttpVersion,
    pub latency_us: u64,
    pub request_bytes: u32,
    pub response_bytes: u64,
    pub http2: Option<Http2Telemetry>,
}
```

`HttpMethod` and `HttpVersion` are integer-valued enums, `Other` first
and `#[default]`, so an unknown method or version round-trips as `Other`
rather than failing.

### BlockDecision - what the detection side decided

```rust
pub struct BlockDecision {
    pub ip: IpAddr,
    pub reason: BlockReason,
    pub ttl_secs: Option<u64>,
    pub batch_subnet: Option<IpNetwork>,
}
```

`batch_subnet` names the batch that triggered the block: an IPv4 /24 or
an IPv6 /64.

### EnforceCommand - the decision payload

```rust
pub struct EnforceCommand {
    pub decision_id: Uuid,
    pub policy_version: u64,
    pub source: String,
    pub actor: String,
    pub timestamp_utc: i64,
    pub ttl_seconds: u64,
    pub reason: String,
    pub ip: IpAddr,
    pub cidr: Option<IpNetwork>,
    pub action: EnforceAction,
    pub evidence_source: EvidenceSource,
}
```

This is a struct, not an enum. Every field is always present, except
`cidr`, which is `None` for a single-IP block - the `action` field
distinguishes block from unblock, and `reason` explains why. `decision_id`
is the deduplication key: the emitting crate generates the UUID, and
enforcement returns the original result for a decision it has already
applied rather than applying it twice.

### EvidenceSource - first-party enforcement provenance

`EvidenceSource` is intentionally limited to `LocalSignals`,
`FleetSignals`, and `Operator`. Community/public reputation is not an
authoritative enforcement source and therefore cannot be represented in
the command vocabulary.

### EnforceAction - block or unblock

```rust
pub enum EnforceAction {
    Block,
    Unblock,
}
```

Two variants. No "pause", no "throttle", no "rate-limit" - those would
require the enforcement service to maintain per-IP state machines. The
design keeps the command vocabulary small. Enforcement is not stateless,
though: it keeps a 65 536-entry decision-idempotency cache, the set of
blocked IPs, and a TTL expiry index.

### EnforceResult - what happened

```rust
pub struct EnforceResult {
    pub decision_id: Uuid,
    pub committed: bool,
    pub applied: bool,
    pub wal_lsn: Option<u64>,
    pub xdp_applied: bool,
    pub error: Option<String>,
}
```

`committed` says the decision was written to the WAL, `applied` says the
store accepted it, and `xdp_applied` says the eBPF map was updated. They
are separate because they can disagree - a decision can be durable and
not yet applied.

### IpNetwork - CIDR representation

```rust
pub struct IpNetwork {
    pub addr: IpAddr,
    pub prefix_len: u8,
}
```

`IpNetwork::new(addr, prefix_len)` masks the host bits and rejects a
prefix the address family cannot represent (/33 on IPv4, /129 on IPv6).
`ipv4_subnet(ip)` returns the /24 containing the IP, `ipv6_subnet(ip)`
the /64. `pack()` and `unpack()` convert between the network and a
`u128` of its address bits - the prefix length is not encoded in the
packed form. `pack()` is not on a production path: the subnet tables
build their `u128` keys with `ramshield_storage::subnet_key_v4` and
`subnet_key_v6`, which mask the host bits themselves. `contains(ip)`
checks membership without allocating.

### Durability - how a write is made durable

```rust
pub enum Durability {
    None,
    Flush,
    Fsync,
    GroupCommit,
}
```

`GroupCommit` is the enum's `#[default]`, but the shipped configuration
default is `Flush`. The variant names the policy the WAL layer applies
when it appends a record: `None` buffers, `Flush` flushes explicitly,
`Fsync` syncs each append, and `GroupCommit` batches the sync.

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

pub enum EnforcementError {
    Wal(String),
    Storage(String),
    Xdp(String),
    Duplicate(Uuid),
    InvalidCommand(String),
}
```

`RsError` is the error type of `ramshield-storage` - the store and the
WAL - and `Result<T>` is its alias. The other crates use their own
`anyhow` or `thiserror` types. `EnforcementError` is defined here but
used by the enforcement crate: it separates a WAL failure from a storage
failure from an XDP failure, and it says whether a command was rejected
(`InvalidCommand`) or simply arrived twice (`Duplicate`).

## Uniqueness

**EnforceCommand is a struct, not an enum.** Most DDoS systems model
enforcement as a two-variant enum (`Block { ip, reason }` /
`Unblock { ip }`). RamShield uses a flat struct with all fields present
always - simpler to serialize, simpler to deserialize, no match arms to
maintain. The `action` field does the work the enum variants would do.

**IpNetwork derives `Hash`.** It can be used as a map key directly.
`pack()` is a separate `u128` form of the address bits, and `unpack()`
turns one back into a network. It is not the key the subnet tables use -
they build that with the storage crate's own `subnet_key_v4` and
`subnet_key_v6`.

**One canonical BlockReason vocabulary.** The IPC layer and the WAL
share the eight wire tokens - `high_rps`, `subnet_burst`,
`forecast_anomaly`, `entropy_anomaly`, `manual`, `l7_high_rps`,
`l7_cost`, `http2_stream_abuse`. `from_reason_str` is the reverse
mapping, and it also accepts the legacy aliases (`syn_flood`,
`volumetric`, `slowloris`, `anomaly`, `manual_unblock`,
`manual_block`) plus the mesh gossip tokens, which have no canonical
variant and fall back to `ManualBlock`.

## Dependencies

**Dependencies:** `serde`, `thiserror`, `uuid`. IP addresses use Rust's
standard-library `std::net` types; CIDR prefixes are represented by the
crate's own `IpNetwork` type. This crate does not depend on `ipnet`.

The `storage` feature exists and is empty; no crate in the workspace
enables it.

**Written by:** the crate itself. It is not a pure type crate - the
reason-string mapping, the CIDR normalisation and the ring buffer are
all in it.

**Read by:** `ramshield`, `ramshield-config`, `ramshield-detection`, `ramshield-enforcement`, `ramshield-forecasting`, `ramshield-protocol`, `ramshield-storage`.

## Testing

12 tests: nine in `src/ip_network.rs` (IPv4 and IPv6 network creation,
membership across address families, `pack`/`unpack` round-trip, prefix
bytes, a deserialized prefix out of range being rejected, an invalid
prefix length, and host bits being masked for both byte-aligned and
non-byte-aligned prefixes), one in `src/error.rs` (the `l7_cost` reason
round-trip), and two in `src/command.rs` (the enforcement provenance
vocabulary excluding external reputation, and a CIDR target keeping its
normalised network).

`crates/ramshield-types/tests/readme_claims.rs` checks this file
against the crate: the dependency list, the list of crates that read it,
the types documented here, and the test count above.
