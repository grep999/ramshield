# ramshield-protocol

## Problem

Client applications (Nginx modules, custom proxies, scripts) need to send connection events to RamShield and receive block/unblock responses. Without a defined wire format, every integration would need custom parsing. Without authentication, anyone who can reach the IPC port can send fake events or issue unauthorized blocks. The protocol module defines the message schema and cryptographic envelope that makes integrations safe and interoperable.

## How it works

### Wire format

Newline-delimited JSON over TCP. One JSON object per line. No streaming, no framing complexity — each `\n` terminates a frame.

```
Client → Server: {"type":"report_connections","events":[...]} \n
Server → Client: {"type":"batch_ok","accepted":950,"rejected":0} \n
```

### Message envelope

```rust
pub struct Message {
    pub version: u16,     // PROTOCOL_VERSION (currently 1)
    pub body: Body,
}

pub enum Body {
    Request(Request),
    Response(Response),
}
```

The version field enables forward-compatible protocol evolution. Future versions can add fields without breaking existing clients.

### Request types

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
```

`deny_unknown_fields` prevents silent acceptance of typos (e.g., a misspelled TTL field won't silently default to permanent block).

### Response types

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    IpStatus { ip, blocked, threat, ewma_rps, reason },
    Ok { message, state },
    BatchOk { accepted, rejected },
    Error { code: u32, message },
    Stats(Stats),
    IpDetail(IpDetail),
}
```

### HMAC-SHA256 authentication

Every frame can carry an auth envelope:

```json
{
  "auth": {"key_id": "k1", "ts_ms": 1725900000000, "sig": "hex_hmac"},
  "type": "report_connections",
  "events": [...]
}
```

```rust
pub fn sign(key: &[u8], ts_ms: u64, payload: &[u8]) -> String
pub fn verify(keys: &[(String, Vec<u8>)], key_id: &str, ts_ms: u64, sig_hex: &str, payload: &[u8], replay: Option<&ReplayStore>) -> Result<(), &'static str>
```

The signature covers `<ts_ms>.<raw_json_without_auth>`. Timestamp prevents replay across frames. `verify()` uses constant-time comparison (XOR-diff loop) to prevent timing attacks.

### ReplayStore

```rust
pub struct ReplayStore { ... }
impl ReplayStore {
    pub fn new(capacity: usize, ttl: Duration) -> Self
    pub fn check_and_record(&self, key_id: &str, nonce: &[u8]) -> Result<(), &'static str>
}
```

Per-key LRU nonce store. 1024 entries, 65s TTL (2× MAX_CLOCK_SKEW + 5s). `check_and_record()` returns `Err` if the nonce was already seen within the TTL window. Prevents an attacker from capturing and re-sending a valid frame.

## Dependencies

```
ramshield-protocol
  ← serde, serde_json, hmac, sha2, hex, ahash

Used by:
  → ramshield-detection (ConnectionEvent from ConnectionReport)
  → ramshield-enforcement (EnforceCommand from BlockIp/UnblockIp)
  → src/ipc/server.rs (parse requests, verify auth, format responses)
  → src/cli.rs (sign requests, format commands)
```

Leaf crate — no RamShield-internal dependencies. The IPC server imports this crate's types directly.

## Key constants

```rust
pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_CLOCK_SKEW_MS: u64 = 30_000;
```

## What to read next

- `src/ipc/server.rs` — implements the TCP server using these types
- `src/cli.rs` — command-line client using these types
- `docs/IPC.md` — human-readable protocol documentation
