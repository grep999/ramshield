# ramshield-protocol

Wire-format definitions, HMAC authentication, and serialization for RamShield's TCP JSON IPC protocol. This crate defines every message type that crosses the wire between client applications and the RamShield server, plus the cryptographic envelope that protects frames in transit.

## Wire format

The IPC protocol is newline-delimited JSON over TCP. Each frame is a single JSON object terminated by `\n`. Request frames carry a `"type"` field that dispatches to the handler; response frames carry a `"type"` field that identifies the response kind.

```
Client → Server: {"type":"report_connections","events":[...]} \n
Server → Client: {"type":"batch_ok","accepted":950,"rejected":0} \n
```

Maximum frame size is configurable (default 32MB). Frames exceeding this limit are dropped and the connection is closed. This protects against memory exhaustion from a single malformed client.

## Request types

| Type | Description | Key fields |
|------|-------------|------------|
| `check_ip` | Query IP status (blocked, threat score, EWMA rate) | `ip` |
| `block_ip` | Manually block an IP | `ip`, `reason`, `ttl_secs` |
| `unblock_ip` | Remove a block | `ip` |
| `get_ip_stats` | Detailed per-IP statistics | `ip` |
| `get_stats` | Global server statistics | — |
| `report_connection` | Single connection event | `ip`, `bytes`, `status_code`, `proto_fp` |
| `report_connections` | Batch of connection events | `events: [ConnectionReport, ...]` |
| `flush` | Force immediate batch flush (debug) | — |

## Response types

| Type | Description |
|------|-------------|
| `ip_status` | IP query result: blocked flag, threat score, EWMA rate, reason |
| `ok` | Acknowledgement with message string |
| `batch_ok` | Batch accepted/rejected counts |
| `error` | Error with HTTP-style code (400, 404, 500, 503) and message |
| `stats` | Server statistics: IPs tracked, blocked count, RAM usage, uptime, evictions |
| `ip_detail` | Full per-IP detail: count, EWMA, threat, state, bytes, timestamps |

## Message envelope

All frames are wrapped in a `Message` envelope:

```rust
pub struct Message {
    pub version: u16,     // PROTOCOL_VERSION (currently 1)
    pub body: Body,
}

pub enum Body {
    Request(Request),
    Response(Response),
}

impl Message {
    pub fn request(req: Request) -> Self
    pub fn response(resp: Response) -> Self
}
```

The version field enables forward-compatible protocol evolution. The `Body` enum discriminates request from response at the envelope level.

## Request and Response enums

```rust
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    CheckIp { ip: String },
    BlockIp { ip: String, reason: String, ttl_secs: Option<u64> },
    UnblockIp { ip: String },
    GetIpStats { ip: String },
    GetStats,
    GetStatus,
    ReportConnection { ip: String, bytes: u64, status_code: u16, proto_fp: u32 },
    ReportConnections { events: Vec<ConnectionReport> },
    Flush,
}

pub struct ConnectionReport {
    pub ip: IpAddr,
    pub bytes: u64,
    pub status_code: u16,
    pub proto_fp: u32,
}

pub struct Stats {
    pub ips_tracked: usize,
    pub blocked: u64,
    pub ram_bytes: usize,
    pub ram_limit_mb: usize,
    pub uptime_secs: u64,
    pub evictions: u64,
}

pub struct IpDetail {
    pub ip: String,
    pub count: u64,
    pub ewma_rps: f64,
    pub threat: f32,
    pub state: String,
    pub bytes_in: u64,
    pub first_seen_s: u64,
    pub last_seen_s: u64,
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    IpStatus { ip: String, blocked: bool, threat: f32, ewma_rps: f64, reason: Option<String> },
    Ok { message: String, state: Option<String> },
    BatchOk { accepted: u64, rejected: u64 },
    Error { code: u32, message: String },
    Stats(Stats),
    IpDetail(IpDetail),
}
```

All types derive `Serialize` and `Deserialize` via serde. `deny_unknown_fields` on Request prevents field typos from being silently accepted (e.g., a wrong TTL field name can't accidentally block IPs permanently). The IPC server uses `serde_json` for framing; this crate is format-agnostic.

## HMAC-SHA256 authentication

Every frame can carry an `"auth"` envelope:

```json
{
  "auth": {
    "key_id": "k1",
    "ts_ms": 1725900000000,
    "sig": "hex_hmac_sha256"
  },
  "type": "report_connections",
  "events": [...]
}
```

The signature covers `<ts_ms>.<raw_json_without_auth>` — the timestamp is prepended to prevent signature reuse across frames. The server maintains a per-key `ReplayStore` (bounded LRU + 10s TTL) to reject replayed signatures.

### auth module

```rust
pub fn sign(key: &[u8], ts_ms: u64, payload: &[u8]) -> String
pub fn verify(key: &[u8], ts_ms: u64, payload: &[u8], sig_hex: &str) -> Result<(), AuthError>
```

`sign()` produces the hex-encoded HMAC. `verify()` recomputes and compares in constant time (via the `hmac` crate's `verify()` method). Timing attacks are neutralized.

### ReplayStore

```rust
pub struct ReplayStore { ... }
impl ReplayStore {
    pub fn new(capacity: usize) -> Self
    pub fn insert_if_new(&self, key_id: &str, ts_ms: u64, sig: &str) -> bool
}
```

`insert_if_new()` returns `true` if the nonce is fresh, `false` if already seen. The LRU evicts oldest entries when capacity is reached. TTL expiry happens lazily on access (no background thread).

## Protocol version

```rust
pub const PROTOCOL_VERSION: u32 = 1;
```

Included in the `stats` response so clients can detect version mismatches. Future protocol changes will bump this constant and gate new fields behind version checks.

## Dependencies

`serde`, `serde_json`, `hmac`, `sha2`, `hex` — all lightweight, audited crates. No async runtime required; the protocol layer is pure data manipulation.

## Tests

- Serde round-trip for every Request and Response variant.
- HMAC sign → verify round-trip with valid and invalid keys.
- Replay detection: same nonce rejected on second call.
- Frame size limits: oversized payloads are caught before deserialization.
- Malformed JSON: graceful error response, no panic.
