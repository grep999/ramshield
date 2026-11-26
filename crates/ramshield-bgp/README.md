# ramshield-bgp

Minimal eBGP speaker library for DDoS mitigation signaling. It sends RTBH
(RFC 7999) and FlowSpec (RFC 8955/8956) UPDATEs over one TCP/179 session.
It does not import routes, compute a routing table, or modify the kernel FIB.

Status: library crate, off by default. The root daemon does not depend on this
crate; `src/engine/upstream.rs` writes announce/withdraw commands to a
BIRD/FRR FIFO when `upstream.bgp_mode != "none"`.

## Layout

| File | Role |
| --- | --- |
| `src/lib.rs` | Public API, `BgpError`, HMAC-SHA256 verification |
| `src/config.rs` | `BgpConfig`, TOML loading, invariant validation, key redaction |
| `src/messages.rs` | 19-octet header, OPEN/KEEPALIVE/NOTIFICATION decode; UPDATE body stays raw |
| `src/fsm.rs` | Six-state RFC 4271 FSM; events map to `TransitionAction` |
| `src/session.rs` | TCP connect, OPEN capability, UPDATE/KEEPALIVE/NOTIFICATION, reads, reconnect |
| `src/attributes.rs` | Path attributes, MP_REACH_NLRI, communities, extended communities |
| `src/rtbh.rs` | IPv4 /32 RTBH announce encoder and community verifier |
| `src/flowspec.rs` | FlowSpec components, RFC 8955 operator codec, NLRI ordering |
| `src/controller.rs` | Schmitt trigger hysteresis; emits RTBH bytes or withdrawal |

## Wire behavior

- BGP-4 only. Fixed marker `0xFF` x 16; PDU length 19-4096.
- OPEN advertises version 4 and MP-BGP capability AFI 1 / SAFI 133.
- Inbound OPEN validation: version 4, configured peer ASN, non-zero BGP ID,
  hold time `0` or `>= 3`, no ID collision. Mismatches send NOTIFICATION.
- Keepalive interval: explicit `keepalive_time` or negotiated `hold_time / 3`,
  minimum 1 second.
- Reads are bounded by hold time (90s before OPEN); writes by a 10s deadline.
- Reconnect uses exponential backoff 1s, 2s, 4s ... 30s, capped by
  `max_reconnect_attempts` (default 10).
- Optional HMAC-SHA256: a 32-byte tag is appended after each PDU and compared
  in constant time. This is a WIP framing convention, not TCP-AO (RFC 5925)
  or TCP-MD5. Do not use it with a peer expecting standard BGP authentication.

## Announcements

`announce_rtbh(ip)` builds an IPv4 /32 UPDATE with ORIGIN IGP, COMMUNITY
BLACKHOLE `65535:666` and NO_EXPORT `65535:65281`, and MP_REACH_NLRI SAFI 1.

`FlowSpecNLRI::udp_reflection(dest, ports)` builds a UDP source-port filter
for one IPv4 destination. `FlowSpecNLRI::rtbh_flowspec(dest)` builds a
destination-prefix-only filter. `BgpSession::send_update` carries FlowSpec in
MP_REACH_NLRI SAFI 133 with a mandatory traffic-filtering extended community.

Components must be in strictly increasing type order. Prefix components
(types 1/2) carry `<type, prefix_len, prefix_octets>`; other components carry
`<type, {operator, value}+>` with the end-of-list bit set on the final pair.

## Configuration

Required TOML fields: `local_asn`, `peer_asn`, `peer_ip` (string).

Optional fields and defaults:

| Field | Default |
| --- | --- |
| `peer_port` | `179` |
| `router_id` | derived from ASN / peer address |
| `hold_time` | `90` |
| `keepalive_time` | `hold_time / 3` |
| `connect_retry_time` | `120` |
| `max_reconnect_attempts` | `10` |
| `schmitt_engage_secs` | `5` |
| `schmitt_disengage_secs` | `15` |
| `peer_shared_key` | none (hex-encoded bytes) |

`validate()` rejects ASN 0, port 0, hold time 1 or 2, keepalive above hold
time, zero connect-retry or engage interval, disengage interval not greater
than engage, iBGP with zero `router_id`, and an empty shared key.
`BgpConfig`'s `Debug` prints `peer_shared_key` as `[REDACTED]`.

## Schmitt trigger

`SchmittTrigger::record_flap()` saturates the flap counter. `should_engage()`
fires once the configured flap count is reached. `should_disengage()` requires
both a full engagement window and a quiet window after the last flap.
`apply_trigger()` emits an RTBH announce only for an IPv4 peer and latches
only a successfully encoded message; disengage returns `None`.

## Boundaries

- One peer, one session. No iBGP, route reflection, multi-session, or FIB writes.
- RTBH and FlowSpec helpers are IPv4-only. IPv6 input is rejected.
- Peers are trusted infrastructure; the crate does not validate prefix origin.
- HMAC is non-standard framing. TCP-AO migration is an owner decision.
- Restrict TCP/179 to the peer address; do not expose it to the internet.

See `../../docs/development/BGP_PRODUCTION_READINESS.md` for rollout gates.

## Tests

```sh
cargo test -p ramshield-bgp --all-targets
```

Covers header validation, OPEN capabilities, FlowSpec codec and ordering,
RTBH communities, FSM transitions, HMAC tamper rejection, config parsing and
redaction, reconnect/keepalive behavior, and Schmitt hysteresis.
