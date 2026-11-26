# BGP enterprise research and application to RamShield (2026-11-26)

Scope: research-backed assessment of `crates/ramshield-bgp` at dirty worktree
`a85a4d0d06d5de6890d93519cffecc1620b9b91b`, with bounded fixes applied in that
worktree. No commit, push, production-default change, or live-peer test was
performed. The root daemon does **not** depend on the crate; its live path is
`src/engine/upstream.rs` writing a BIRD/FRR FIFO when `bgp_mode != "none"`.
All crate findings are latent until that crate is wired in.

Thesis: the crate is substantial (9 modules, ~2.7k source LOC) but was not
interoperable. It implemented the vocabulary of BGP without the session
semantics an enterprise speaker needs. The correct engineering response is
not a rewrite: keep the codecs, enforce RFC invariants at the few seams where
the current code is wrong or unsafe, and document the remaining gaps as
owner-gated rollout work.

## 1. Method

1. Pin repository state: branch `master`, HEAD `a85a4d0d...`, dirty tree.
2. Read the implementation end to end: `messages.rs`, `attributes.rs`,
   `flowspec.rs`, `session.rs`, `fsm.rs`, `rtbh.rs`, `controller.rs`, `config.rs`.
3. Check primary standards: RFC 4271, 4760, 5492, 7606, 6811, 8205, 5082,
   5925, 5880, 7854, 7999, 8955, 8956, 9117, 9234.
4. Check operator/vendor guidance: NIST SP 800-189r1, M3AAWG FlowSpec BP,
   FRR, BIRD, GoBGP, exaBGP docs, APNIC/NANOG deployment reports.
5. Check formal-methods and testing work: FSM assertion checking, MESSI,
   Lightyear, CB-VER.
6. Build executable probes against the actual encoder (`OPEN`, FlowSpec NLRI,
   FlowSpec UPDATE) before changing code; keep them as regression tests.
7. Apply only bounded fixes; re-run focused tests, formatting, and Clippy.

Evidence rule: a defect is recorded only if a primary source and a concrete
`file:line` or probe output support it. Subagent findings were treated as
leads and re-checked against the source before any edit.

## 2. State of the art

### 2.1 Core session semantics

RFC 4271 remains the base: 19-octet header, OPEN length minimum 29, hold-time
negotiation as the lower of the two non-zero values, KEEPALIVE exactly 19
octets, and FSM-driven error responses. RFC 7606 revised UPDATE error handling
because unconditional session reset let a distant attacker reset many sessions
through optional transitive attributes. Modern speakers prefer
treat-as-withdraw / attribute-discard, with logging, over blind teardown.
Sources: https://www.rfc-editor.org/rfc/rfc4271 ,
https://www.rfc-editor.org/rfc/rfc7606 ,
https://www.rfc-editor.org/rfc/rfc7606#section-3 .

RFC 4760 moved NLRI into MP_REACH_NLRI / MP_UNREACH_NLRI. MP_REACH_NLRI is
optional non-transitive (`0x80`), and a speaker MUST NOT send NLRI for an
AFI/SAFI the peer did not advertise. RFC 5492 defines capability advertisement
in OPEN. Sources: https://www.rfc-editor.org/rfc/rfc4760 ,
https://www.rfc-editor.org/rfc/rfc5492 .

Enterprise consequence: a correct emitter must remember peer capabilities,
negotiate timers, and validate message lengths before writing bytes. Encoding
a well-formed UPDATE to an unadvertised SAFI is still a protocol error.

### 2.2 DDoS signaling: RTBH and FlowSpec

RFC 7999 standardizes BLACKHOLE community `65535:666` for RTBH. NIST SP
800-189r1 recommends RTBH and FlowSpec as part of a DDoS toolkit, with ingress
validation so more-specifics are accepted only under an authorized aggregate.
Sources: https://www.rfc-editor.org/rfc/rfc7999 ,
https://csrc.nist.gov/pubs/sp/800/189/r1/ipd ,
https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-189.pdf .

RFC 8955 defines FlowSpec encoding: NLRI is a `<length, components>` tuple;
FlowSpec next-hop length is 0; action extended communities are transitive
(0x8006 traffic-rate, 0x8007 traffic-action, 0x8008 redirect). RFC 8956 covers
IPv6. RFC 9117 updates validation for unicast route congruence, iBGP and route
servers. RFC 8955 section 7.3 defines the traffic-action T bit: T=1 means
evaluate later rules, T=0 stops evaluation. Sources:
https://www.rfc-editor.org/rfc/rfc8955 ,
https://www.rfc-editor.org/rfc/rfc8956 ,
https://www.rfc-editor.org/rfc/rfc9117.html ,
https://www.rfc-editor.org/rfc/rfc8955.html#section-7.3 .

M3AAWG FlowSpec BP and recent operator reports stress operational limits:
FlowSpec is for temporary, programmatic filtering; permanent ACLs belong in
ACLs; complex matches consume TCAM; rules should be validated against unicast
state and rate limits should respect vendor granularity. Sources:
https://www.m3aawg.org/flowspec-BP ,
https://www.m3aawg.org/sites/default/files/doc_files/m3aawg-flowspec-bp-2019-02.pdf ,
https://blog.apnic.net/2025/04/04/bgp-flowspec-doesnt-suck-were-just-using-it-wrong ,
https://blog.apnic.net/2024/09/18/the-ultimate-weapon-against-ddos-bgp-flowspec ,
https://documentation.nokia.com/acg/24-7-2/books/unicast-routing-protocols-classic/c184-flowspec-valid.html .

Enterprise consequence: emitting a FlowSpec UPDATE without capability
negotiation, length tuple, zero next-hop, mandatory path attributes, and a
correct action community produces a rule a conforming router rejects or
misapplies. Validation is a receiver-side control that the product must respect
when designing destination prefixes.

### 2.3 Routing security evolution

- RPKI route origin validation: RFC 6811. Validates origin AS, not path.
- BGPsec: RFC 8205. Path validation; deployment remains limited.
- GTSM: RFC 5082. TTL=255 on eBGP; cheap defense against off-path spoofing.
- TCP-AO: RFC 5925. Standard replacement for TCP MD5; the crate's trailing
  HMAC tag is non-standard and cannot interoperate with a standard peer.
- BGP Roles / OTC: RFC 9234. Role capability in OPEN plus optional transitive
  OTC attribute (type 35) stops route leaks. FRR implements it; Juniper
  shipped it in Junos 25.2R1; IXPs report real leak suppression.
- BFD: RFC 5880. Fast failure detection independent of BGP hold timers.
- BMP: RFC 7854. Monitoring of peer state and Adj-RIB-In without being in the
  forwarding path.

Sources: https://www.rfc-editor.org/rfc/rfc6811 ,
https://www.rfc-editor.org/rfc/rfc8205 ,
https://www.rfc-editor.org/rfc/rfc5082 ,
https://www.rfc-editor.org/rfc/rfc5925 ,
https://www.rfc-editor.org/rfc/rfc5880 ,
https://www.rfc-editor.org/rfc/rfc7854 ,
https://datatracker.ietf.org/doc/html/rfc9234 ,
https://blog.cloudflare.com/rfc9234-bgp-role-model ,
https://blog.apnic.net/2025/09/05/preventing-route-leaks-made-simple-bgp-roleplay-with-junos-rfc-9234 ,
https://docs.frrouting.org/en/latest/bgp.html#bgp-roles-and-only-to-customers .

Enterprise consequence: a DDoS signaling speaker is still a BGP speaker. It
must not become a route-leak source, it must authenticate at the transport
layer using a standard mechanism, and operators need telemetry (BMP or
equivalent) before enabling it in production.

### 2.4 Enterprise implementations

FRR, BIRD 3.x, GoBGP and exaBGP provide mature session lifecycle, policy,
route refresh, graceful restart, BFD/BMP hooks, and vendor-tested FlowSpec
behavior. FRR documents BGP roles/OTC, BFD and BMP; BIRD documents large
communities and FlowSpec; GoBGP and exaBGP are typical controller-side speakers.
Sources: https://docs.frrouting.org/en/latest/bgp.html ,
https://docs.frrouting.org/en/latest/bfd.html ,
https://docs.frrouting.org/en/latest/bmp.html ,
https://bird.nic.cz/doc/bird-3.2.2.html ,
https://github.com/osrg/gobgp/blob/master/docs/sources/flowspec.md ,
https://github.com/Exa-Networks/exabgp/wiki .

For RamShield the practical choice is unchanged: keep BIRD/FRR as the live
data-plane signaling path (`src/engine/upstream.rs`) until the crate passes
real-peer interop. The crate is useful as a deterministic encoder/test harness,
not as a drop-in replacement.

### 2.5 Formal methods and model-based testing

- Pei, Massey and Zhang derived protocol assertions from BGP FSMs to detect
  invalid message sequences. This directly justifies executable transition
  tests in `fsm.rs`. https://irl.cs.ucla.edu/data/files/techreports/ucla-cs-tr040047.pdf
- MESSI (USENIX NSDI 2024) generated ~150K model-based tests and found 22 bugs
  across FRR, Quagga, GoBGP, BIRD and Batfish. https://www.usenix.org/conference/nsdi24/presentation/singha
- Lightyear (SIGCOMM 2023) modularized BGP control-plane verification and ran
  for six months on a hyperscaler network. https://arxiv.org/pdf/2204.09635v2
- CB-VER (2026) formalizes modular control-plane verification in Lean and uses
  SMT for per-node/per-edge checks. https://arxiv.org/pdf/2604.03539
- "Protocols to Code" verifies a router implementation down to Go code in
  Isabelle/HOL. https://arxiv.org/pdf/2405.06074.pdf

Enterprise consequence: full control-plane verification is out of scope for a
DDoS signaling crate. The affordable local analogue is a set of byte-level and
FSM assertions that fail on the exact historical defects. That is what the new
probes and tests are.

## 3. Invariants applied to RamShield

| ID | Invariant | Status | Evidence |
|---|---|---|---|
| I1 | Header marker 16xFF, length 19..=4096, type valid | HOLDS | `messages.rs:42-78` |
| I2 | OPEN declared length equals bytes written; minimum 29 | FIXED | `session.rs:142-155`; `tests/open_length_probe.rs` |
| I3 | Hold time = min(local, peer); zero disables keepalive | FIXED | `session.rs:407-411`, `291-298`; unit tests |
| I4 | SAFI 133 UPDATE only after peer advertised capability | FIXED | `session.rs:183-187`, `413-417`; unit + probe |
| I5 | MP_REACH UPDATE carries ORIGIN + AS_PATH | FIXED | `attributes.rs:181-194`; `session.rs`, `rtbh.rs:61-62` |
| I6 | MP_REACH flags optional non-transitive (`0x80`) | FIXED | `session.rs`, `rtbh.rs:89` |
| I7 | FlowSpec NLRI has length tuple; next-hop length 0 | FIXED | `flowspec.rs`, `session.rs:177`; `tests/flowspec_length_probe.rs` |
| I8 | Discard action T=0 (stop rule evaluation) | FIXED | `attributes.rs:459` |
| I9 | KEEPALIVE exactly 19 octets | FIXED | `messages.rs:164-168` |
| I10 | Bad BGP identifier NOTIFICATION `(2,3)`; hold expiry `(4,0)` | FIXED | `session.rs`, `fsm.rs:160-164,190` |
| I11 | Legal FSM transitions; Active reachable; no bogus `OpenSent+OpenConfirmReceived` | FIXED | `fsm.rs:125-180`; `tests/bgp_tests.rs` |
| I12 | Bounded socket writes/reads | HOLDS | `session.rs` `write_all_with_timeout`, `read_message` |
| I13 | Constant-time HMAC comparison | HOLDS BUT NON-STANDARD | `lib.rs:32-49`; RFC 5925 TCP-AO needed |
| I14 | Receive-side UPDATE validation / treat-as-withdraw | PARTIAL | `messages.rs:177-267` validates body/NLRI/attr bounds; no RIB/treat-as-withdraw |
| I15 | BMP / telemetry / operator visibility | DECLARED ONLY | `crates/ramshield-metrics/src/lib.rs:237-247`; no increments outside metrics crate |
| I16 | RPKI/ROV, OTC, GTSM | ABSENT | no capability code 9/35 or TTL checks in crate |
| I17 | Real-peer interop evidence | ABSENT | no FRR/BIRD canary run |

## 4. Fixes applied in this work

Wire/session fixes already applied earlier in the same dirty worktree:

- OPEN length computed from assembled bytes (`29 + opt_params`).
- FlowSpec NLRI RFC 8955 length tuple on encode/decode, strict increasing type
  order, bounded decode end offset.
- FlowSpec MP_REACH next-hop length 0, reserved octet 0.
- ORIGIN IGP and AS_PATH SEQUENCE on both FlowSpec and RTBH UPDATEs.
- MP_REACH flags optional non-transitive.
- Discard T-bit cleared.
- KEEPALIVE length checked.
- Bad BGP identifier NOTIFICATION `(2,3)`.
- `announce_rtbh(ip, local_asn, peer_asn)`; all call sites and tests updated.

New fixes applied from this research round:

- `BgpSession` records peer OPEN capabilities (`peer_flowspec`) and refuses to
  emit SAFI 133 before RFC 4760 §5 negotiation (`session.rs:57,183,413-417`).
- Hold time negotiated per RFC 4271 §6.2; zero disables keepalives
  (`session.rs:106,259,291,407-411`).
- FSM: `Connect + ConnectRetryTimerExpired` now enters `Active`; removed the
  illegal `OpenSent + OpenConfirmReceived -> Established` transition; hold
  expiry in OpenSent/OpenConfirm sends NOTIFICATION `(4,0)`
  (`fsm.rs:125-180`).
- Added focused regression tests for hold negotiation, zero hold, capability
  gating, Active state, and hold-expiry notification.
- FlowSpec `FlowSpecNLRI::encode` rejects an AND bit set on the first numeric
  pair and rejects a pair whose value length does not match the operator's
  operand width (RFC 8955 §4.2.1.1, §4.2.1).
- Received UPDATE bodies now enforce RFC 4271 §4.3 length, IPv4 withdrawn-NLRI
  bounds, and path-attribute length bounds before callers see the bytes
  (`messages.rs:177-267`).
- OPEN capability decode no longer truncates unknown/oversized values: unknown
  capabilities are skipped, MP-BGP code 1 must be exactly 4 octets
  (`messages.rs:310-350`).
- Config rejects `hold_time=0` with a non-zero `keepalive_time` (RFC 4271
  §4.2/§4.4) (`config.rs:96-102`).
- FlowSpec decode clears the AND bit on the first pair (RFC 8955 §4.2.1.1)
  (`flowspec.rs:285-289`).
- Received UPDATE path attributes enforce well-known flags (optional=0,
  transitive=1, partial=0) per RFC 4271 §5 (`messages.rs:233-244`).
- FlowSpec helpers accept CIDR (`a.b.c.d/len`), mask host bits, and encode
  the prefix length per RFC 8955 §4.2.2.1 (`flowspec.rs:299-326`).
- RTBH MP_REACH next-hop is the RFC 7999 discard address `192.0.2.1`, not
  unspecified `0.0.0.0` (`rtbh.rs:75-84`).

Skipped deliberately: no new dependency, no full RIB, no FIB writes, no new
async runtime or timer wheel. `ponytail:` boundary: the crate remains an
encoder/emitter; upgrade to a full speaker only when a real peer is wired in
and interop evidence exists.

## 5. Remaining gaps and dispositions

P0 / merge-blocking if the crate is ever wired in:

- G1 Receive-side UPDATE framing is now validated (body length, withdrawn IPv4
  NLRI bounds, path-attribute bounds). Full RFC 7606 treat-as-withdraw and RIB
  semantics remain absent. PARTIALLY FIXED.
- G2 Custom HMAC tag after PDU (`lib.rs:32-49`) is not TCP-AO/TCP-MD5. NOT FIXED.
  Interop requires RFC 5925 in the socket layer.

P1:

- G3 No peer role / OTC (RFC 9234), no RPKI/ROV integration, no GTSM TTL check.
  NOT FIXED; owner decision needed before new session capabilities.
- G4 Six BGP metrics exist but are never incremented outside the metrics crate.
  NOT FIXED; owner-gated because live-path changes are involved.
- G5 No withdraw on shutdown/disable; FIFO writes are unacknowledged.
  NOT FIXED; owner-gated change to `src/engine/upstream.rs`.
- G6 Real-peer interop not run. NOT RUN.

P2:

- G7 FlowSpec first-pair AND bit and per-type operand widths are now rejected by
  the encoder; receiver-side validation guidance is still absent. PARTIALLY FIXED.
- G8 `fsm.rs` is still not the driver of `session.rs`; timers exist as fields
  and helper tasks, not one authoritative loop. NOT FIXED.

Owner-gated production defaults (do not change without explicit authorization):

- `config.prod.toml:241` `bgp_mode = "rtbh"`, `:244` FIFO path,
  `:247` protected prefix, `:251` BLACKHOLE community.
- `crates/ramshield-config/src/validate.rs` upstream validation.

## 6. Verification performed

Exact commands run in `/home/m/vehicle_of_rationalism/ramshield/beta/rs`:

- `cargo fmt --all` then `cargo fmt --all -- --check` — PASS.
- `cargo test -p ramshield-bgp --all-targets` — PASS:
  46 unit + 8 fault-injection + 14 integration + 1 + 1 + 1 = 71 tests.
- `cargo clippy -p ramshield-bgp --all-targets -- -D warnings` — PASS.
- `git diff --check` — PASS.
- No `cargo test --workspace`, no CI run, no live-peer test: NOT RUN.

Logs:

- `/home/m/.hermes/profiles/ramshield/cache/bgp_phd_tests_round2.log`
- `/home/m/.hermes/profiles/ramshield/cache/bgp_phd_clippy.log`

## 7. Roadmap to enterprise readiness

1. Wire receive-side UPDATE decode with RFC 7606 error handling and focused
   malformed-attribute tests.
2. Replace trailing HMAC with TCP-AO (or explicitly scope the crate to a
   trusted controller link) before any real peer.
3. Add RFC 9234 role capability + OTC path attribute for route-leak safety.
4. Increment existing BGP metrics from session/controller paths; add BMP or a
   minimal equivalent for peer state and Adj-RIB visibility.
5. Run FRR and BIRD interop canaries in a lab; capture PCAP and decoded logs.
6. Only then propose wiring the crate into the daemon and changing production
   defaults, with owner authorization.

Sources are primary RFCs, operator guidance, vendor documentation and
peer-reviewed work cited above. No CI run, no real-peer run, and no production
change are claimed by this document.
