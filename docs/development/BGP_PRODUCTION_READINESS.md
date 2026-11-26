# BGP production readiness

This document is the BGP boundary. It mirrors MESH_PRODUCTION_READINESS.md
and covers the `ramshield-bgp` crate plus the `src/engine/upstream.rs` BGP
announce/withdraw integration.

## Current status

The crate is implemented and tested but **not production-ready** and not enabled by default. It is a substantial BGP speaker library. At HEAD `a85a4d0d` it had wire defects preventing interoperability with a standards-compliant peer: OPEN length, FlowSpec NLRI length, FlowSpec next-hop length, ORIGIN/AS_PATH, MP_REACH flags, zero-ID NOTIFICATION, KEEPALIVE length, and discard T-bit. Those defects are fixed in the local worktree; see `BGP_IMPLEMENTATION_RESEARCH_2026-11-26.md` for exact file:line evidence and reproduction.

Implemented on the current tree:

- `ramshield-bgp`: BGP FSM (6 states), OPEN/KEEPALIVE/UPDATE/NOTIFICATION
  framing, HMAC-SHA256 peer authentication, RTBH community encoding,
  FlowSpec NLRI encoding with component ordering, Schmitt trigger controller,
  and config parsing with key redaction. The FSM and controller are library-level helpers, not called by `BgpSession`.
- `src/engine/upstream.rs`: BGP announce/withdraw via FIFO (BIRD/FRR command
  pipe), Schmitt trigger hysteresis, and saturation detection. This is the only live BGP path.
- `ramshield-metrics`: BGP session state, announce/withdraw counts, HMAC
  failures, FSM flaps, and last error exposed in Prometheus output. These series are not incremented by current code.
- Tests: 13 unit/integration tests + 8 fault injection tests (FSM convergence,
  HMAC tamper rejection, header validation, Schmitt trigger, config redaction).

Current implementation is not a production candidate. The remaining gaps include the P0 wire defects, hold-time negotiation, UPDATE receive handling, live-path metrics, and explicit owner decisions.

## Boundaries

B1. **Feature boundary.** BGP stays off unless `upstream.bgp_mode` is
configured (not `"none"`). The default build and default config remain unchanged.

B2. **Session boundary.** The speaker maintains a single TCP/179 session to
one peer. It does not import routes, calculate routing tables, or modify
the kernel FIB. Route reflection, iBGP, and multi-session are out of scope.

B3. **Authentication boundary.** HMAC-SHA256 authenticates the BGP message
payload. The key is configured via `peer_shared_key` in TOML (hex-encoded).
The current implementation does not negotiate TCP-AO (RFC 5925) or TCP-MD5.
Changing that is an owner decision and is out of scope here.

B4. **Origin trust boundary.** The speaker announces the configured
`protected_prefix` (a /32 or /128 host route). It does not validate the
origin of the prefix or prevent a compromised peer from announcing
arbitrary prefixes. Peers are trusted infrastructure, not untrusted
internet clients.

B5. **Resource boundary.** Messages are bounded to 4096 bytes (BGP standard),
reconnect attempts to 10 with exponential backoff (max 30s), and hold time
to 90s default. The Schmitt trigger requires 5 flaps to engage and 15s
quiet to disengage (configurable).

B6. **Exposure boundary.** The library connects to a peer on TCP/179; it does
not listen. Public internet exposure is out of scope. Deploy behind a firewall
that restricts outbound port 179 to the peer's IP address.

B7. **Rollout boundary.** No default enablement and no fleet rollout from
this document. Rollout is staged: single-node proof, two-node canary,
single-node production canary, then fleet. Each stage has an explicit
pass/fail.

## Definition of done

A BGP change is production-ready only when all of these are true:

1. Exact-SHA gates pass:
   - `cargo fmt --all --check`
   - `cargo check --workspace --all-targets`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace --lib --bins --tests`
   - `cargo test -p ramshield-bgp --all-targets`
   - `cargo audit`
   - `cargo deny check`
2. BGP tests pass, including:
   - FSM transitions: Idle → Connect → OpenSent → OpenConfirm → Established
   - HMAC tamper rejection
   - Header validation (bad marker, bad length, unknown type)
   - RTBH announce encoding with BLACKHOLE + NO_EXPORT communities
   - FlowSpec NLRI component ordering
   - Schmitt trigger engage/disengage hysteresis
   - Config debug key redaction
   - Config from_toml parsing (including HMAC key)
3. Fault-injection proof passes:
   - FSM establish → notify → restart converges
   - Hold timer expiry returns to Idle
   - HMAC rejects tampered, wrong-key, and empty-key messages
   - Header rejects malformed input
   - Schmitt trigger engages after threshold, disengages after quiet window
4. Operational proof passes:
   - Config has complete BGP fields and no logged key material
   - BGP session state is visible in metrics/logs
   - Announce/withdraw counts are observable
   - Fail-closed behavior: no announce without a session (library path only; the live FIFO path does not enforce this)
5. Owner sign-off exists for any change to HMAC→TCP-AO migration, production
   default, or BGP coupling.

## Work plan

### Phase B1 - close the implementation gate

- DONE: Fix `decode_open` to parse optional parameters as byte length (RFC 4271)
- DONE: Fix `MpReachNlri` reserved byte (always present per RFC 4760)
- DONE: Fix RTBH NLRI encoding (prefix_len + prefix, not raw octets)
- DONE: Add BGP metrics (session, announce, withdraw, HMAC failures, flaps, error)
- DONE: Add fault injection tests (FSM, HMAC, header, Schmitt, config)
- Exit: all automated gates pass and BGP crate tests are green.

### Phase B2 - operational hardening

- Confirm metrics/logging expose session state, announce/withdraw outcomes,
  HMAC failures, and FSM flaps.
- Confirm startup rejects a public bind without credentials (if applicable).
- Run a single-node canary with a real BGP peer; observe one announce and
  one withdraw cycle.

Exit: canary runbook records pass/fail and no silent loss.

### Phase B3 - rollout

- Stage: single-node -> two-node canary -> single-node production canary -> fleet.
- Roll back by setting `upstream.bgp_mode = "none"`; no BGP data migration required.
- Do not enable by default or change mesh in this phase.

Exit: owner-approved rollout with rollback evidence.

## Non-goals for this task

- TCP-AO (RFC 5925) or TCP-MD5 migration.
- Route import, FIB manipulation, or iBGP.
- Production default enablement.
- Mesh BGP coupling.
- A full fleet rollout.

## Exact-SHA evidence

Recorded on branch `chore/untracked-cleanup` at the current worktree HEAD:

| Command | Exit | Result |
| --- | --- | --- |
| `cargo fmt --all --check` | 0 | PASS |
| `cargo check --workspace --all-targets --locked` | 0 | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | PASS |
| `cargo test --workspace --lib --bins --tests --locked` | 0 | PASS |
| `cargo test -p ramshield-bgp --all-targets --locked` | 0 | PASS, 21 passed |
| `cargo audit` | 0 | PASS |
| `cargo deny check` | 0 | PASS |