# RamShield BGP implementation research

Date: 2026-11-26
Branch / HEAD: `master` @ `a85a4d0d06d5de6890d93519cffecc1620b9b91b`
Scope: `crates/ramshield-bgp`, its root-daemon relationship, checked-in BGP docs, and the standards/operator requirements for a DDoS-signaling speaker.
Method: source read of every BGP module and test, live wire probes against a local TCP listener, RFC text checks, and vendor documentation comparison. No master code was changed for this report.

## Verdict

`ramshield-bgp` is a substantial implementation, not a stub: 9 modules, 2,769 source lines, 601 test lines, full attribute/FlowSpec codecs, RTBH community handling, an FSM, reconnect logic, and config validation. It is also **not interoperable with a standards-compliant BGP peer in its current state**. Three P0 wire defects prevent session establishment or FlowSpec installation:

1. OPEN length is 10 octets short.
2. FlowSpec NLRI omits the RFC 8955 `<length, NLRI value>` tuple.
3. FlowSpec MP_REACH_NLRI carries a 4-octet next hop where RFC 8955 requires length 0.

The live daemon does not use this crate at all. `src/engine/upstream.rs` writes text commands to a BIRD/FRR FIFO. Therefore the crate's protocol quality is currently a library-quality issue, not a production-path issue; the production path has a different set of observability and lifecycle gaps.

## P0 findings (blocking interoperability)

| ID | Evidence | Standard | Failure | Bounded fix | Regression |
|---|---|---|---|---|---|
| P0-1 | `crates/ramshield-bgp/src/session.rs:140` sets `msg_len = 19 + opt_params.len()`; OPEN body is 10 octets plus optional parameters. | RFC 4271 section 4.2: OPEN minimum length is 29 octets including the header. | Every compliant peer sees a length field 10 octets shorter than the bytes sent and rejects OPEN; session never reaches Established. | `let msg_len = 29usize + opt_params.len();` then `u16::try_from`. | `tests/open_length_probe.rs`: header length must equal bytes on wire. |
| P0-2 | `crates/ramshield-bgp/src/flowspec.rs:306-318` returns component bytes only; `session.rs:169-175` places them directly in MP_REACH_NLRI; `attributes.rs:301-308` appends NLRI bytes unchanged. | RFC 8955 section 4: MP_REACH_NLRI/MP_UNREACH_NLRI NLRI is one or more `<length, NLRI value>` 2-tuples; length is 1 octet below 240, otherwise 2 octets with high nibble `0xf`. | Peer parses the first component type (`0x01`) as the NLRI length, then treats the rest as malformed. No FlowSpec rule is installed. | Prefix encoded components with the RFC 8955 1/2-octet length; decode the tuple before component parsing. | `tests/flowspec_length_probe.rs`: first byte must be value length, not component type. |
| P0-3 | `crates/ramshield-bgp/src/session.rs:173` uses `next_hop: vec![0, 0, 0, 0]`. | RFC 8955 section 4: when advertising FlowSpec, Next-Hop Length MUST be 0 and the address MUST be ignored. | Strict peers reject the UPDATE as malformed; the FlowSpec action never reaches the filtering engine. | `next_hop: Vec::new()`. | `tests/flowspec_update_probe.rs`: MP_REACH next-hop length byte must be 0. |

Reproduction evidence before the scratch fix:

- `open_length_probe`: `left: 27, right: 37` (length field vs bytes).
- `flowspec_length_probe`: `left: Some(1), right: Some(6)` (component type vs NLRI length).
- `flowspec_update_probe`: `left: 4, right: 0` (next-hop length).

A verified patch fixing all three, plus focused regression tests, is at:

`/home/m/.hermes/profiles/ramshield/cache/bgp_research_p0_patch.diff`

It is applied to the local `master` worktree for verification only; it is not committed or pushed.

Scratch verification after the patch:

- `cargo test -p ramshield-bgp --all-targets` — PASS: 36 lib + 8 fault + 14 integration + 1 length + 1 UPDATE + 1 OPEN = 61 tests (re-verified 2026-11-26 on clean `a85a4d0d` worktree, exit 0).
- `cargo clippy -p ramshield-bgp --all-targets -- -D warnings` — PASS.
- `git diff --check` — PASS.

The patch is not applied to `master` and was not pushed.

## P1 findings (correctness / operability)

| ID | Evidence | Issue | Bounded fix |
|---|---|---|---|
| P1-1 | `session.rs:148` sends local hold time; `session.rs:348-373` validates peer OPEN but never stores `min(local, peer)`; `read_message` and keepalive use the local value. | RFC 4271 section 4.2 requires the negotiated hold time to be the smaller of configured and received. A peer with a shorter hold time will tear down a dead session before RamShield notices. | Store configured hold; on `validate_open`, set negotiated hold to `min(configured, open.hold_time)`. If zero, disable periodic keepalive. |
| P1-2 | `messages.rs:144-154` returns `BgpMessage::Update(body)` with the body raw; no UPDATE validation or NOTIFICATION path exists. | RFC 4271 section 6.3, RFC 7606, and RFC 8955 section 10 require malformed UPDATE handling. The crate silently accepts arbitrary UPDATE bodies. | Either implement bounded UPDATE decode + NOTIFICATION, or document the crate as send-only and remove receive claims. |
| P1-3 | `attributes.rs:426-433` `flowspec_discard()` sets only the traffic-action terminal bit. | RFC 8955 section 7.1 defines `traffic-rate-bytes 0` as discard; section 7.3 terminal bit only stops evaluation. A caller expecting discard gets no discard action. | Implement discard as `flowspec_traffic_rate(0.0)`, or combine rate 0 with terminal action. |
| P1-4 | Root `Cargo.toml` has no `ramshield-bgp` dependency; `src/engine/upstream.rs:176-209` writes BIRD/FRR text commands to a FIFO. | The BGP crate is not in the live path. HMAC, FSM, and session metrics do not protect the actual announce/withdraw operation. | Document the split clearly; if the crate is intended for production, add a feature-gated adapter and an end-to-end test. |
| P1-5 | `crates/ramshield-metrics/src/lib.rs:237-247` defines BGP counters; no code increments them. | Dashboards export `ramshield_bgp_*` series that are permanently zero/empty. | Wire counters at session/announce/withdraw/error sites, or remove the series and the claim. |
| P1-6 | `docs/development/BGP_PRODUCTION_READINESS.md:9-11` says the crate “speaks enough BGP-4”; `:26` calls it a production candidate; `:54` says it listens on TCP/179; `:94` claims no announce without a session. | These claims are contradicted by P0-1..P0-3 and by the FIFO integration. | Rewrite the status section to state the P0 blockers and the live-path split. |

## P2 findings (robustness / maintainability)

- `session.rs:235` uses `keepalive_time.max(1)`; RFC 4271 section 4.4 says periodic KEEPALIVE MUST NOT be sent when negotiated hold time is zero.
- `config.rs` rejects hold time 1/2 but allows `keepalive_time == 0` with a non-zero hold time; `run_keepalive` silently converts it to 1 second.
- `attributes.rs:305` casts `next_hop.len()` to `u8`; a public caller can wrap a >255-octet next hop.
- `flowspec.rs` public encoders do not validate prefix length vs octets, component-specific value widths, or the first-pair AND bit (RFC 8955 section 4.2.1.1).
- `messages.rs:221-227` truncates capability values to 4 octets; longer standard capabilities are silently shortened.
- `session.rs:391-406` appends a 32-byte HMAC tag after each PDU. This is a custom framing convention, not TCP-AO (RFC 5925) or TCP-MD5 (RFC 2385), and has no sequence/nonce. It requires a matching non-standard peer.
- `src/engine/upstream.rs:227-234` does not check for a short FIFO write. Commands are below `PIPE_BUF`, so Linux pipe atomicity makes this low risk, but the return value should still be checked.
- `fsm.rs` and `controller.rs` are exported but have no production callers; only tests and docs use them.
- `crates/ramshield-bgp/Cargo.toml` lists `bytes`, `tracing`, and `ramshield-metrics` without any source use. Remove them or use them.

## Architecture: live vs latent

Live production path:

`config.upstream.bgp_mode` → `src/engine/upstream.rs` → `write_bgp_fifo()` → BIRD/FRR command pipe.

The live path has no TCP/179 session, no OPEN/KEEPALIVE, no HMAC, no FSM, and no BGP metrics. It relies on the external routing daemon for all protocol behavior.

Library path:

`BgpSession` → `send_open` / `read_message` / `send_update` / `send_keepalive` / `send_notification` → `messages.rs`, `attributes.rs`, `flowspec.rs`, `rtbh.rs`.

`fsm.rs` and `controller.rs` are not called by `BgpSession`; they are library-level helpers exercised only by tests. This is not necessarily wrong, but the docs must not imply they are in the live path.

## Standards matrix

| Area | Status | Evidence |
|---|---|---|
| BGP header / marker / 19-octet KEEPALIVE | Implemented | `messages.rs`, `session.rs` |
| OPEN encode | Broken | P0-1 |
| OPEN decode / capability parse | Mostly implemented; 4-octet capability truncation | `messages.rs:180-233` |
| Hold-time negotiation | Missing | P1-1 |
| UPDATE encode (RTBH) | Implemented for IPv4 /32 + communities | `rtbh.rs` |
| UPDATE encode (FlowSpec) | Broken | P0-2, P0-3 |
| UPDATE decode / error handling | Missing | P1-2 |
| RTBH BLACKHOLE `65535:666` | Implemented | `rtbh.rs`, RFC 7999 section 5 |
| FlowSpec component codec | Implemented; missing first-pair AND and width validation | `flowspec.rs` |
| FlowSpec traffic-rate / discard | Partial; discard semantics wrong | P1-3 |
| TCP-AO / TCP-MD5 | Not implemented; custom HMAC only | `session.rs:391-406`, RFC 5925 / RFC 2385 |
| IPv6 FlowSpec | Not implemented; helpers reject IPv6 | `flowspec.rs`, `rtbh.rs` |
| Route import / RIB / FIB | Intentionally absent | crate docs and root integration |
| BMP / RPKI / BFD / graceful restart | Absent | no code or config |

## Enterprise comparison and roadmap

A top-tier BGP DDoS-signaling integration does not need to become a general routing stack. It does need to be a correct, observable, and safely recoverable speaker for a small set of actions. The relevant comparison is not “FRR minus features”; it is “what must be true before an upstream operator trusts an automated FlowSpec/RTBH source.”

Authoritative references used:

- FRR BGP: https://docs.frrouting.org/en/latest/bgp.html
- FRR BFD: https://docs.frrouting.org/en/latest/bfd.html
- FRR BMP: https://docs.frrouting.org/en/latest/bmp.html (BMP route monitoring explicitly does not cover FlowSpec)
- BIRD 3.2.2: https://bird.nic.cz/doc/bird-3.2.2.html
- RFC 4271: https://www.rfc-editor.org/rfc/rfc4271.html
- RFC 4760: https://www.rfc-editor.org/rfc/rfc4760.html
- RFC 7999: https://www.rfc-editor.org/rfc/rfc7999.html
- RFC 8955: https://www.rfc-editor.org/rfc/rfc8955.html
- RFC 8956: https://www.rfc-editor.org/rfc/rfc8956.html

Prioritized gaps:

P0 — interoperability blockers:
1. Fix OPEN length, FlowSpec NLRI length tuple, FlowSpec next-hop length.
2. Add wire-level tests that parse actual TCP bytes, not only helper round-trips.

P1 — safe production use:
3. Negotiate hold time; honor zero hold time by disabling periodic keepalive.
4. Decide send-only vs receive-capable. If send-only, remove UPDATE receive claims and return a clear error for UPDATE.
5. Fix discard semantics to `traffic-rate-bytes 0`.
6. Either wire the crate into the daemon behind a feature flag with an end-to-end test, or state that the FIFO path is the only production path.
7. Wire BGP metrics or remove them.
8. Replace custom HMAC with TCP-AO (RFC 5925) or document it as a private framing that requires a matching peer; add replay/sequence considerations if it remains.

P2 — operator-grade hardening:
9. Add BFD or a documented hold-time-only liveness policy.
10. Add BMP/route-monitoring or an equivalent local audit log for every announce/withdraw.
11. Add RPKI/origin policy if prefixes can be accepted from peers; for send-only, document the trusted-peer boundary.
12. Add graceful-restart/session-drain behavior and a rollback test.
13. Remove unused dependencies and dead exports or wire them.

P3 — scale/interop:
14. IPv6 RTBH/FlowSpec (RFC 8956) if the protected prefix can be IPv6.
15. Multi-peer/multi-session support only if the daemon will own the speaker; otherwise keep the FIFO boundary.
16. Formal interop lab with FRR/BIRD and one hardware vendor.

## Delegated audit addendum (2026-11-26)

Four independent read-only audits at `a85a4d0d` confirmed the findings above and added:

- MP_REACH UPDATEs lacked ORIGIN and AS_PATH. RFC 4760 section 3 requires both on every MP_REACH UPDATE; RFC 4271 calls them well-known mandatory. Fixed locally via `attributes::{origin_attr, as_path_attr}` used by both `send_update` and `announce_rtbh`.
- MP_REACH flags were `0xC0` (optional transitive); RFC 4760 section 3 requires optional non-transitive `0x80`. Fixed locally.
- Zero BGP ID NOTIFICATION used `(5,4)` instead of `(2,3)` (RFC 4271 section 8). Fixed locally.
- KEEPALIVE accepted any declared length; RFC 4271 section 6.1 requires exactly 19 octets. Fixed locally.
- `flowspec_discard()` set T=1; RFC 8955 section 7.3 says T=1 evaluates later rules. Fixed locally to T=0.
- FSM defects remain latent: bogus `OpenSent + OpenConfirmReceived` transition, no hold-timer actions in OpenSent/OpenConfirm, `Active` unreachable. Not fixed.
- FlowSpec first-pair AND bit is not cleared and per-type operand widths are not validated. Not fixed.
- Custom HMAC tag appended after the PDU is non-standard framing; RFC 5925 TCP-AO is the standard path. Not fixed.
- Live-path issues are owner-gated: `config.prod.toml:225-251` enables `bgp_mode="rtbh"`; no withdraw on shutdown/disable; six BGP metrics declared but never incremented; webhook uses cleartext HTTP; FIFO has no ACK; prefix validation allows broader than host routes. Not changed locally: production defaults require explicit owner authorization.

Additional primary sources: RFC 5492 (capabilities), RFC 5082 (GTSM), RFC 5880 (BFD), RFC 7854 (BMP), RFC 5925 (TCP-AO); FRR BGP/BFD/BMP docs, BIRD 3.2.2, GoBGP FlowSpec/config docs, exaBGP wiki, M3AAWG FlowSpec Best Practices.

## Documentation corrections

`crates/ramshield-bgp/README.md` must not call the crate “minimal” or “narrow”. The accurate description is a standalone BGP speaker library with substantial codec/FSM code. It must state that the root daemon uses a BIRD/FRR FIFO, not this crate.

`docs/development/BGP_PRODUCTION_READINESS.md` must say “not production-ready”, must not claim TCP/179 listening, and must separate the latent library path from the live FIFO path.

## Bottom line

The implementation is not minimal. After the locally applied fixes it is wire-correct for OPEN length, FlowSpec NLRI framing, next-hop length, ORIGIN, AS_PATH, MP_REACH flags, zero-ID NOTIFICATION, KEEPALIVE length, and discard T-bit. It is still not production-ready: no session driver, no peer capability negotiation, no standard authentication, no live metrics, and no real-peer interop evidence. Next concrete step is an FRR/BIRD interop run, then owner authorization for any master push.