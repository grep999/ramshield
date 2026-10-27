# ramshield-protocol

```text
Client (Nginx module / script / proxy)
        │
        │  TCP line-delimited JSON
        ↓
   ┌─────────────────────────────┐
   │ Message {                   │
   │   version: 1,               │
   │   auth: {key_id, ts, sig},  │
   │   body: Request | Response  │
   │ }                           │
   └─────────────────────────────┘
        │
        ↓
   HMAC-SHA256 verification
   + ReplayStore nonce check
        │
        ↓
   IPC Server → Detection/Enforcement
```

## Why it exists

Client applications — Nginx modules, custom proxies, load balancers, monitoring scripts — need a way to send connection events to RamShield and receive block/unblock responses. Without a defined wire format, every client would need custom parsing. This crate specifies the message envelope, request/response types, and authentication protocol that all clients and the IPC server share.

## How it works

### Message Envelope

Every message on the wire is a JSON object wrapped in a `Message`:

```rust
pub struct Message {
    pub version: u16,         // currently 1
    pub auth: Option<Auth>,   // HMAC-SHA256 authentication
    pub body: Body,
}

pub enum Body {
    Request(Request),
    Response(Response),
}
```

The version field allows backward-compatible protocol evolution — the IPC server rejects messages with unknown versions before parsing the body.

### Request Types

```rust
pub enum Request {
    Connection(ConnectionReport),  // client → RamShield: new connection
    Block(IpAddr),                 // client → RamShield: block this IP
    Unblock(IpAddr),               // client → RamShield: unblock this IP
    Stats,                         // client → RamShield: current stats
    IpInfo(IpAddr),                // client → RamShield: details for this IP
}
```

`ConnectionReport` is the primary request type — it carries the source IP, upstream IP, status code, response time, and protocol from a proxied connection:

```rust
pub struct ConnectionReport {
    pub ip: String,           // source IP (String, not IpAddr — preserves original format)
    pub upstream_ip: String,  // backend server IP
    pub status: u16,          // HTTP status code
    pub response_time_ms: u64,
    pub protocol: String,     // "http", "https", "grpc"
}
```

IPs are `String` rather than `IpAddr` because clients send IP literals in various formats (`10.0.0.1`, `::ffff:10.0.0.1`, `2001:db8::1`). Normalization happens server-side.

### Response Types

```rust
pub enum Response {
    Blocked { ttl_secs: u64 },          // IP was blocked
    Unblocked,                          // IP was unblocked
    StatsResponse(Stats),               // current system stats
    IpInfoResponse(IpDetail),           // per-IP details
    Error { code: u16, message: String }, // something went wrong
}
```

`Stats` and `IpDetail` are detailed structs carrying the same data the dashboard serves — but over the IPC TCP connection instead of HTTP:

```rust
pub struct Stats {
    pub blocked_ips: u64,
    pub total_events: u64,
    pub rps: f64,
    pub uptime_secs: u64,
}

pub struct IpDetail {
    pub ip: String,
    pub threat_score: f32,
    pub event_count: u64,
    pub blocked: bool,
    pub first_seen: u64,
    pub last_seen: u64,
}
```

### HMAC Authentication

Every frame carries an `auth` object alongside its body:

```json
{"auth": {"key_id": "k1", "ts_ms": 1700000000000, "sig": "<hex>"}, "type": "check_ip", "ip": "1.2.3.4"}
```

The signing API is:

```rust
pub fn sign(key: &[u8], key_id: &str, ts_ms: u64, payload: &[u8]) -> Result<String, &'static str>
pub fn verify(
    keys: &[(String, Vec<u8>)], // (key_id, key bytes)
    key_id: &str,
    ts_ms: u64,
    sig_hex: &str,
    payload: &[u8],
    replay: &ReplayStore,
) -> Result<(), &'static str>
```

**The MAC input is `ts_ms` ++ `"."` ++ `key_id` ++ `payload`** — the decimal
timestamp, a single `.` separator, the key id, then the payload bytes, with
no trailing separator:

```
HMAC-SHA256(key, "<ts_ms>.<key_id><payload>")
```

`key_id` is inside the MAC input on purpose: two key ids holding identical
key bytes must not produce the same signature, or a frame signed under one
key id would be accepted under another.

The payload is the **compact, key-sorted serialization of the frame with
the `auth` object removed**.

The server does not MAC the bytes you sent. It parses the frame into a
JSON value, removes `auth`, and re-serializes what is left. That
re-serialization sorts the keys, so the bytes the server signs are the
sorted ones — not yours. A client that signs its own byte order produces
a different MAC and is rejected.

Sign the sorted form: `json.dumps(obj, separators=(',', ':'), sort_keys=True, ensure_ascii=False)`
in Python, or `serde_json::to_vec` of a `BTreeMap` in Rust. `ensure_ascii=False`
is required: `serde_json` writes non-ASCII as UTF-8, and the escaped form
Python produces by default is different bytes. This is what
`docs/IPC.md` shows.

`verify()` recomputes the MAC, checks it matches, and rejects a timestamp
further than `MAX_CLOCK_SKEW_MS` (10 seconds) from the server clock.

The layout is pinned by a golden vector in `src/auth.rs`
(`golden_vector_pins_the_mac_input_layout`), computed independently of this
crate. If you change the MAC input, that test fails and the protocol
changes for every client.

### Replay Protection

`ReplayStore` prevents replay attacks — an attacker capturing a valid signed
frame and re-sending it:

```rust
pub struct ReplayStore {
    cap: usize,
    per_key_cap: usize,
    ttl: Duration,
    // ...
}
```

The nonce is **not** client-supplied. It is the HMAC-SHA256 digest the
verifier computes, so a replayed frame presents the same digest and is
caught:

```rust
pub fn check_and_record(&self, key_id: &str, nonce: &[u8]) -> Result<(), &'static str>
```

`check_and_record` returns `Ok(())` on first use, `Err("replay")` for a live
duplicate, and `Err("capacity")` when a global or per-key limit is reached.
Saturation **rejects** new entries rather than evicting live replay
markers, so capacity pressure cannot reopen a replay window. Expired
entries are removed lazily.

The default TTL is 1 second, which is shorter than the skew window and is
only suitable for tests. The IPC server uses a 65-second TTL — several
times the 10-second skew window — so a frame signed by a clock running
fast cannot be replayed in the gap between the skew window and the store
window, and the store is bounded at 1024 entries globally and 256 per
key.

### Wire Format

Messages are newline-delimited JSON on a TCP connection. The IPC server reads one line at a time, deserializes it into a `Message`, processes the body, and writes back a response line. This is deliberately simple — no framing, no length prefixes — so clients can be implemented in any language with a JSON library.

## Uniqueness

**Replay protection built into the protocol.** Most IPC protocols for DDoS detectors skip authentication entirely. This one includes HMAC signing, timestamp skew checking, and nonce-based replay prevention — critical when the IPC connection carries block/unblock commands that an attacker could replay to unblock their own IPs.

**String IPs, not IpAddr.** The protocol deliberately preserves the original IP format from the client. Normalization (`IpAddr` parsing, IPv4-mapped IPv6 handling) happens server-side, not in the wire format. This prevents ambiguity when clients send `::ffff:10.0.0.1` vs `10.0.0.1`.

**Line-delimited JSON.** Deliberately simple. No protobuf, no custom binary framing. A curl command can send events:

```bash
echo '{"version":1,"body":{"Request":{"Connection":{"ip":"10.0.0.1","status":200,"response_time_ms":12,"protocol":"http"}}}}' | nc 127.0.0.1 7890
```

## Dependencies

**Reads from:** `ramshield-types` (`IpAddr`, `IpNetwork`, `BlockReason`), `hmac`, `sha2`, `hex` (HMAC computation), `ahash` (the replay store's `AHashMap`).

**Written by:** the root `ramshield` package - its IPC server parses the messages and sends the responses, and its dashboard module serves the IPC stats endpoint. Neither is a crate of its own.

**Read by:** external clients (Nginx modules, scripts, custom proxies).

## Benchmarks

No standalone benchmarks — message parsing is measured implicitly through the IPC server benchmarks. At 150K events/second, the JSON parsing overhead is <0.1ms per message.

## Testing

25 tests covering: message serialization round-trip, HMAC sign+verify with valid and expired timestamps, replay detection (same nonce sent twice), version rejection (version=2 returns error), and malformed JSON handling.
