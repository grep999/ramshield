# RamShield Mesh Transport — TLS Security Architecture

**Status:** FORWARD-LOOKING DESIGN PROPOSAL. Not approved. Not implemented.
**Scope:** `crates/ramshield-mesh/src/transport.rs` (TCP mesh transport, HMAC-SHA256
envelope authentication) and the configuration surface that feeds it.
**Repository state at time of writing:** branch `feature/mesh-f01-hmac-edge-secrets`,
HEAD `2d827fafbbd2b28f49ff6ca5b4924f5d0aebee02`.
**Deliverable:** a reviewable design document for the repository owner. No production
code was written; no repository file was created, modified or deleted.

---

## 0. Approval dependency — read this first

The controlling remediation roadmap (`ramshield-remediation-roadmap-v1.2.md`, §
"Executive directive") states:

> **Cryptography:** Keep HMAC. Do not add Ed25519, asymmetric signatures, or a
> public-key identity system. Default to retaining the current key-distribution
> model during this remediation. […] Do not implement pairwise-key migration
> without explicit owner approval.

and §2 (F-01) states:

> Keep HMAC. Do not introduce asymmetric cryptography.
> […] Preserve current key distribution for this remediation unless the owner
> explicitly approves migration.

**TLS is an asymmetric-cryptography migration.** Certificate-based authentication
requires per-node key pairs and signature verification, which is precisely the
class of change the roadmap currently prohibits for this remediation cycle.
Therefore:

1. **This design cannot be implemented under the current approval.** It is a
   proposal for a *future, separately approved* workstream, or for an amendment
   to the roadmap.
2. The F-01 remediation currently in flight is **not** to be disturbed by it. F-01's
   HMAC hardening is a valid, bounded patch on its own terms, and its residual-risk
   statement (`docs/development/audit-2026-10-20/F01_REPORT.md` §7) remains the
   accurate description of today's transport.
3. The owner has a legitimate third option that this document does **not** prejudge:
   accept the residual identity risk and defer TLS. That is explicitly one of the
   paths the roadmap permits ("If isolation is not provided, mark it as residual
   risk only if the owner accepts that risk").

Section 9 enumerates the exact roadmap and document clauses that a TLS adoption
would have to amend, with the current text quoted and the required change stated.
Nothing below should be read as a recommendation to bypass those gates.

---

## 1. Goals and non-goals

### 1.1 The problem being solved

Today, each configured edge (this node ↔ one peer) holds **one shared HMAC
secret** at **both** endpoints (`MeshPeer.auth_key`,
`crates/ramshield-mesh/src/transport.rs:96`; `MeshHandle.trusted_keys`,
`transport.rs:547`). Outbound frames and application ACKs are signed with the
edge secret and verified with the same secret (`sign`/`verify`,
`transport.rs:1795-1812`). Inbound frames are additionally bound to the source IP
of the claimed peer's configured address (`same_ip` check, `transport.rs:1280-1290`),
which is defence in depth and fails when two peers share an IP.

A valid MAC therefore proves **possession of the edge secret**, not **which of the
two endpoints produced the frame**. The consequences, stated in the F-01 report
(`docs/development/audit-2026-10-20/F01_REPORT.md` §7.1):

* Compromise of one endpoint yields that edge in **both** directions.
* The compromised node can forge frames that its own peer accepts as coming from
  the *other* endpoint, and then propagate them — laundering the forged origin
  cluster-wide.
* Unique per-endpoint cryptographic identity is **not provided**, and cannot be
  provided with symmetric HMAC.

### 1.2 Why symmetric schemes cannot fix this (a short proof, to close off a
common false fix)

* **Per-node symmetric secrets** (the pre-F-01 model) re-introduce F-01a: a node
  that holds peer B's secret can forge frames claiming to be B. The verifier
  selects its key from the *claimed* identity (`transport.rs:1297`), so possession
  of B's secret is sufficient to impersonate B. This is exactly the defect F-01a
  recorded.
* **Directional per-edge secrets** (a separate A→B key and B→A key per edge) do
  **not** help: both endpoints of the edge still hold both directions' keys, so a
  compromised A can still produce frames that B accepts as "from B".
* **Any scheme in which both endpoints hold the same verification material**
  makes the two endpoints cryptographically indistinguishable. This is a
  structural property, not an implementation detail.

**Conclusion:** closing the identity gap requires *asymmetric* per-endpoint
credentials. TLS is one way to obtain them (mTLS with per-node certificates).
Ed25519-signed envelopes with per-node key pairs is another. Both require owner
approval under the current roadmap. TLS additionally buys channel confidentiality,
standard tooling and a standard protocol — which is why it is worth a proposal,
and why the cost model below matters.

### 1.3 Goals

| # | Goal | How TLS achieves it |
|---|---|---|
| G1 | **Per-endpoint authentication.** A receiver can attribute a frame to exactly one node, cryptographically. | Mutual TLS: the client presents a certificate bound to its node identity; the server verifies the chain and the pin, and therefore knows which node connected. The application layer then requires the envelope's claimed `node_id` to equal the TLS-authenticated identity. |
| G2 | **Channel confidentiality.** Blocklists, IPs, expiry metadata and WAL batch contents are not visible to a passive network observer. Today the envelope body is plaintext JSON over TCP; the MAC protects integrity, not secrecy. | TLS record layer. |
| G3 | **Channel integrity.** Confidentiality of the *framing* is provided by the record layer's AEAD; the existing HMAC envelope MAC remains a redundant, defence-in-depth layer during transition. | TLS 1.3 AEAD (AES-GCM / ChaCha20-Poly1305). |
| G4 | **Standard tooling and interoperability.** Operators can use `openssl s_client`, Wireshark, standard certificate tooling, and off-the-shelf proxies for debugging and future migration to external systems. | TLS is the interoperability substrate. |
| G5 | **Close F-01a** in the audit backlog (`docs/development/AUDIT_FINDINGS_BACKLOG.md:20`), which is currently recorded as `DEFER — needs asymmetric keys or strictly pairwise MACs + config change`. | mTLS with per-node credentials is exactly "asymmetric keys". |
| G6 | **Preserve the existing authorisation model.** Edge-scoped secrets continue to define *which peers may talk to me at all*, so a TLS peer that is not in my configured peer set is still rejected regardless of certificate validity. | Existing `trusted_keys` / `peer_auth_keys` checks retained. |

### 1.4 Non-goals — what TLS explicitly does **not** fix

These must be stated in `docs/SECURITY_MODEL.md` and in the mesh release gate if
TLS is adopted, so that adopting TLS is not mistaken for closing other findings.

* **CRDT correctness.** AWORSet convergence, dot ordering, tombstone semantics
  (`crates/ramshield-mesh/src/aworset.rs`) are unaffected by transport
  authentication. F-02, F-03, F-04, F-05, F-06 remain exactly as open as they are.
* **WAL durability and the causal-acceptance contract.** The rule that a frame may
  be acknowledged only after durable WAL append (`ReceivedMeshMessage::acknowledge_with_lsn`,
  `transport.rs:323`, and its documented contract at `transport.rs:311-322`) is
  unchanged. TLS authenticates *who sent* a frame; it says nothing about whether
  the receiver persisted it.
* **Recovery-proof validity.** The `RecoveryProof` binding (generation, recovery id,
  boundary LSN, snapshot digest, frame count — `transport.rs:248-255`) is an
  application-level protocol. TLS does not validate it; it only prevents a third
  party from altering it in flight.
* **Peer authorisation policy.** Whether node 7 *should* be allowed to send
  block deltas to node 3 is a policy question. TLS authenticates the peer; the
  policy (allow-list, roles, what a peer may originate) must still be written.
* **DoS resistance.** **TLS does not stop a flood.** It adds a CPU-expensive
  handshake that an attacker can also force. A flood of TCP SYNs or of TLS
  ClientHellos is a different attack from the one RamShield mitigates with XDP.
  TLS must not be allowed to create a new amplification or CPU-exhaustion surface
  — see §8.
* **Node-level compromise containment.** TLS stops a compromised node from
  impersonating its peers. It does not stop a compromised node from emitting
  *validly attributed* garbage within its own authorisation scope.
* **Availability of the mesh.** TLS adds expiry, clock, rotation and
  certificate-management failure modes that do not exist today (§8).

---

## 2. Options

Four options, with real trade-offs. "Cost" means new dependencies, new
operational surface, per-connection cost, and migration risk.

### Option A — In-process TLS via rustls on the existing TcpStream

The mesh transport itself performs the handshake. `TcpStream` is replaced by
`tokio_rustls::client::TlsStream` / `server::TlsStream` at the two call sites
that already exist: the outbound connect in `send_to_ack` (`transport.rs:1200`)
and the inbound accept in `read_stream` (`transport.rs:1247`, spawned from the
accept loop at `transport.rs:628`). The newline-delimited JSON framing
(`read_bounded_frame`, `transport.rs:1773`) is unchanged — TLS sits *below* it.

**Buys:** G1–G6 in full. Per-endpoint identity, confidentiality, standard
tooling. No extra process, no extra port, no proxy in the data path.

**Costs:**
* New dependencies: `rustls` 0.23, `tokio-rustls` 0.26, `rustls-pki-types`, plus a
  crypto provider (§7). All are MIT/Apache-2.0/ISC/BSD-3-Clause — inside the
  current `deny.toml` allow-list — but this is the first time the mesh crate
  gains a crypto stack, and the first time the daemon gains a TLS stack at all
  (`docs/FEATURE_0.5_L7_MESH_UPSTREAM.md:177-179` explicitly avoided one:
  *"This avoids adding a second TLS stack to the daemon."*).
* Per-connection cost: a TLS 1.3 full handshake is 1-RTT after TCP and costs
  asymmetric crypto. rustls' own benchmark (rustls 0.23.37 + aws-lc-rs 1.16.0,
  Xeon E-2386G) reports **full TLS 1.3 handshakes: 3,160/s client, 6,786/s
  server** (ECDSA), and **resumed TLS 1.3 handshakes: 6,735/s client, 7,249/s
  server** ([rustls perf report, 2026-03-07](https://rustls.dev/perf/2026-03-07-report/)).
  A separate localhost lab measured a full TLS 1.3 handshake at **p50 1.507 ms**
  versus **0.021 ms** for a bare TCP connect (~72×), with OpenSSL session reuse
  giving ~1.5× more connections/sec for TLS 1.3 ([ShopperCove lab,
  2026-09-30](https://www.shoppercove.com/blog/openssl-tls-handshake-full-vs-resume-localhost-lab)).
  These are CPU-bound costs, so they scale with connection *rate*, not with
  payload size.
* Memory: one TLS session per live connection (rustls keeps transcript and
  buffers). The current design is one TCP connection **per frame**, so this is
  only material if the per-connection-per-frame pattern is kept.
* Operational surface: certificate lifecycle, expiry, clock dependence, pinning
  correctness, resumption behaviour. All new.

**Failure modes:**
* A certificate or pin mistake turns into a mesh partition (§8).
* If the peer identity is derived from the *claimed* `node_id` rather than from
  the verified certificate, the design silently reproduces the current defect
  while appearing to fix it. This is the single highest-risk implementation
  mistake and is the reason §3.5 is written the way it is.
* rustls panics/failure modes are a new class of failure; untrusted input is
  parsed by a large C-free but non-trivial state machine. Mitigation: bounded
  handshake timeouts, and the existing `MAX_PEER_READERS` semaphore.

**Assessment:** the only option that delivers G1 (per-endpoint identity) without
also moving identity out of the daemon. It is the reference option against which
the others are measured.

### Option B — TLS terminated by a sidecar (stunnel or equivalent)

The mesh speaks plain TCP to `127.0.0.1`; a sidecar terminates TLS towards the
peer and forwards plaintext locally.

**Buys:** G2, G3, G4, and — *if the sidecar is configured with mutual TLS and
per-node client certificates* — G1 at the network boundary.

**Costs:**
* No new Rust dependencies; the TLS stack lives outside the audited binary.
  This is attractive given the supply-chain posture in §7.
* New operational surface is *larger*, not smaller: a second process per node
  (or a shared proxy), its own config, its own certs, its own logs, its own
  failure modes, plus the systemd/Kubernetes wiring (`systemd/`,
  `deploy/k8s/deployment.yaml`) to update.
* **The identity gap does not automatically close.** The mesh process sees a
  plain TCP stream from localhost. To attribute a frame to a specific node, the
  sidecar must convey *which* peer the connection came from. A generic TLS
  terminator does not do that. The usual workarounds are:
  * one local listening port per remote peer (operationally ugly, and the mesh
    dials one configured address per peer — `MeshPeer.address` — so this actually
    maps onto the existing config model);
  * the PROXY protocol (not supported by `stunnel`; requires a different
    terminator);
  * or a `SO_ORIGINAL_DST`-style lookup, which is fragile.
  Without one of these, the mesh still authenticates with a shared edge secret
  and **the residual risk is unchanged** — while the operator has paid for a TLS
  stack and gained only confidentiality.
* The security boundary moves to the loopback interface. Anything that can write
  to that socket can inject plaintext frames; the mesh would have to trust the
  local proxy completely.
* Debugging becomes harder: the mesh's own logs no longer show the TLS state, and
  the on-wire evidence requires the sidecar's logs.

**Failure modes:** sidecar crash or restart drops all mesh connections; a
mis-mapped local port silently attributes a peer's traffic to the wrong identity
(the exact bug class TLS was adopted to remove); certificate management moves to
a second configuration surface with its own drift.

**Assessment:** legitimate, and the right answer for some deployments (already
running a mesh-wide proxy). It is a *poor* answer for the stated goal unless
peer-identity propagation to the mesh is designed in explicitly, and it is a
poor answer for the stated "small architecture" preference of this project. It
also cannot satisfy G6 (edge-scoped authorisation) without re-implementing the
peer mapping inside the mesh anyway.

### Option C — Hybrid: keep the HMAC edge secret under TLS (or migrate fully off it)

Two sub-variants:

* **C1 — TLS + HMAC retained (belt and braces).** TLS supplies confidentiality,
  channel integrity and per-endpoint identity; the existing envelope MAC and ACK
  MAC remain and remain *mandatory* on TLS-protected channels.
* **C2 — TLS only.** The HMAC envelope MAC is dropped once TLS is proven, and the
  envelope keeps only `ts_ms`, `node_id`, `body`.

**C1 buys:** the existing key-distribution model is *preserved* — the roadmap's
default. Each edge still shares one secret; TLS adds a per-node certificate on
top. Migration is additive: existing `peer_auth_keys` keep working, and existing
`docs/FEATURE_0.5_L7_MESH_UPSTREAM.md` provisioning guidance stays valid.
Authorisation (edge membership) stays exactly where it is; identity (endpoint
attribution) is newly supplied by TLS.

**C1 costs:** two authentication layers to reason about; two places to get
"which key" right; the HMAC becomes redundant for G1 but is retained for G6.
Small CPU cost (one HMAC-SHA256 over a ≤64 KiB frame — negligible against a
handshake).

**C2 buys:** one authentication layer, simpler mental model, marginally cheaper
frames, and the eventual removal of a shared secret from the threat surface.

**C2 costs:** it removes the edge-level authorisation signal that today makes a
non-configured peer rejectable *without* any certificate infrastructure, and it
changes the key-distribution model outright — a bigger approval ask, and the
exact thing the roadmap says not to do without approval. It also means a TLS
misconfiguration (e.g. a verifier that accepts anything) is a total auth failure
rather than a layered one.

**Assessment:** **C1 is the recommended migration shape.** It is additive,
keeps the roadmap's default key distribution, and lets the owner close F-01a
without a key-distribution migration. C2 is a reasonable *later* simplification
once TLS is proven in production, but it should not be the first step.

### Option D — Do not adopt TLS; close the gap another way

Stated honestly, because it is a real option and in some circumstances it is the
right one.

**D1 — Accept the residual risk.** Roadmap §2 explicitly permits this: mark the
isolation verdict "not provided" as an accepted residual risk, recorded in the
mesh release gate. The residual risk is then a *documented and owned* limitation
rather than an open question.

**D2 — Close it with Ed25519 envelope signing instead of TLS.** Per-node Ed25519
key pairs; each frame's MAC is replaced by a signature over the same payload that
the HMAC covers (`(ts_ms, node_id, body)`); each node's *public* key is
distributed to its peers. This closes G1 (per-endpoint identity) with **no
transport change, no handshake, no session management, no certificate expiry,
no clock dependence for the identity check, and no TLS stack**. Cost: one new
dependency (`ed25519-dalek` + `curve25519-dalek`), signature verification
instead of a symmetric MAC (roughly an order of magnitude more CPU per frame —
but per *frame*, not per *connection*), and a key-distribution model change
(node public keys instead of edge secrets) that itself needs owner approval
under the same clauses TLS does.

**D3 — Reduce blast radius without new crypto.** Fewer edges (star vs full mesh),
network segmentation of the mesh port, mTLS at the network perimeter via a
switch/IPSEC tunnel. These reduce exposure; they do not provide identity, and
the roadmap explicitly says "Operational mitigations are not equivalent to
cryptographic isolation."

**When D is right:**
* The mesh remains experimental and disabled in production (roadmap §"Release"),
  the deployment is small and fully trusted, and the owner prefers to spend the
  effort on F-02…F-08 correctness first — which is a *defensible* priority
  ordering, because CRDT/WAL correctness affects data safety while identity
  isolation affects attribution.
* The environment forbids new dependencies (air-gapped, certified build,
  FIPS/custom provider constraints) — note that `ring` and `aws-lc-rs` are the
  two providers and one of them can usually be satisfied; if *neither* can, D2
  (Ed25519 via `ed25519-dalek`) is cheaper than TLS and closes the same gap.
* The deployment already terminates mesh traffic through a shared, properly
  configured mutual-TLS proxy that propagates peer identity (Option B done well).

**When D is wrong:** the fleet is multi-tenant, peers are shared across trust
domains, a node may be operated by a different party, or the residual risk must
be closed to satisfy a compliance requirement. In those cases the residual risk
is not acceptable and an asymmetric mechanism is required.

**Summary table**

| | A: in-process rustls | B: sidecar terminator | C1: TLS + HMAC | C2: TLS only | D1: accept | D2: Ed25519 |
|---|---|---|---|---|---|---|
| Per-endpoint identity (G1) | yes | only with explicit peer mapping | yes | yes | no | yes |
| Confidentiality (G2) | yes | yes | yes | yes | no | no |
| Standard tooling (G4) | yes | yes | yes | yes | no | no |
| Preserves key distribution | no (adds per-node keys) | depends | **yes** | no | yes | no |
| New in-process dependencies | rustls + provider | none | rustls + provider | rustls + provider | none | ed25519-dalek |
| Per-connection cost | high (mitigable) | high | high (mitigable) | high (mitigable) | none | none (per-frame only) |
| Closes F-01a | yes | conditional | yes | yes | no | yes |
| Approval burden | high | high | high | highest | lowest (explicit acceptance) | high |

---

## 3. Transport integration

### 3.1 Where the handshake sits relative to the existing framing

The current wire format is:

```
TCP stream
  └─ frame := JSON(Envelope{ts_ms, node_id, body, mac}) + b"\n"
```

`Envelope` is defined at `transport.rs:231-237`; the body is a `WireMessage`
(`transport.rs:114-118`) which is either a standard `MeshMessage`
(`transport.rs:59-71`) or a `RecoveryWireMessage`
(`transport.rs:142-165`). The reader is `read_bounded_frame`
(`transport.rs:1773`), which accumulates bytes until a newline is seen, bounded
by `MAX_FRAME = 64 KiB` (`transport.rs:24`).

TLS replaces the transport under the framing:

```
TCP stream
  └─ TLS 1.3 records (encrypted)
       └─ frame := JSON(Envelope{ts_ms, node_id, body, mac}) + b"\n"
```

**The framing is unchanged.** The application ACK
(`AckEnvelope`, `transport.rs:389-397`) travels back over the *same* connection
(`transport.rs:1452-1454`) and is therefore inside the same TLS session — which
is exactly the property we want, since the ACK is the durability receipt.

**Concretely, the code change is small and local:**
* `transport.rs:1200` — `TcpStream::connect(peer)` becomes
  `TlsConnector::connect(peer)`; the resulting `TlsStream` is used exactly as
  the `TcpStream` is today (`write_all`/`read_bounded_frame_with_timeout`).
* `transport.rs:1247` — `read_stream(&self, mut stream: TcpStream, remote)` becomes
  generic over the stream type, and the accept path performs the server handshake
  before spawning the reader (or inside it).
* The `MAX_FRAME` bound, the `MAX_FRAME_READ_TIMEOUT` (5 s), the
  `MAX_PEER_WRITE_TIMEOUT` (3 s) and the `MAX_APPLICATION_ACK_WAIT` (2 s) all
  continue to apply. **A separate, tighter handshake timeout is required** —
  see §8.3.

`read_bounded_frame` is already generic over `R: AsyncRead + Unpin`
(`transport.rs:1773`), so it works unchanged on a `TlsStream`.

### 3.2 The awkward part: one connection per frame

This is the crux of the integration problem and it is specific to this codebase.

`send_to_ack` (`transport.rs:1164-1246`) does, for **each** frame:

1. `TcpStream::connect(peer)` — a **new** TCP connection (`transport.rs:1200`);
2. write one frame;
3. read exactly one ACK frame from the same connection;
4. drop the connection.

The inbound side matches: `read_stream` reads **one** frame, handles it, writes
**one** ACK, and returns (`transport.rs:1247-1455`). The connection is
single-use. The current mesh is therefore a strict request/response protocol
over one-shot connections.

Consequences for TLS:

* A TLS handshake would occur **once per frame**, i.e. once per anti-entropy
  chunk, once per WAL batch, and once per recovery frame. The anti-entropy
  snapshot is chunked at `MAX_SYNC_FRAME_ENTRIES = 16` entries
  (`transport.rs:27`), and a recovery stream is bounded by
  `MAX_RECOVERY_FRAMES = 8192` (`transport.rs:29`) — so a single large sync can
  be thousands of connections and thousands of handshakes.
* The cost is *per connection*, so it scales with frame rate, not with data
  volume. A 64 KiB frame and a 200-byte frame cost the same to hand-shake.

**Therefore TLS must be paired with one of two mitigations, and the second one
is strongly preferred:**

**Mitigation 1 (mandatory): TLS 1.3 session resumption (PSK).** rustls supports
stateful server-side session storage and client-side ticket storage:
`ServerConfig.session_storage` (default: 256 sessions in memory with the `std`
feature) and `ServerConfig.ticketer`; `ClientConfig.resumption` with
`ClientSessionMemoryCache` ([rustls `ServerConfig`
docs](https://docs.rs/rustls/latest/rustls/server/struct.ServerConfig.html),
[rustls `ClientConfig`
docs](https://docs.rs/rustls/latest/rustls/client/struct.ClientConfig.html)).
A resumed TLS 1.3 handshake is still 1-RTT but performs **no** asymmetric
cryptography and **no** certificate verification — the observed cost is roughly
half of a full handshake in the rustls benchmark (6,735/s vs 3,160/s client).

**Mitigation 2 (strongly recommended): connection reuse / multiplexing.** Keep
one long-lived TLS connection per peer and multiplex frames over it, with a
request/response correlation ID in the envelope. This converts the per-frame
handshake into a per-connection handshake, i.e. from O(frames) to O(peers).
It is also a **larger change** than TLS itself: it alters the connection
lifecycle, requires correlation IDs (which interacts with the ACK binding work
in F-02/F-03), and needs reconnect/backoff logic. It should be a separate,
sequenced phase (§10).

**Explicitly rejected: TLS 1.3 0-RTT early data.** rustls exposes
`ClientConfig.enable_early_data` (default `false`) and
`ServerConfig.max_early_data_size`, so it is available — but early data is
replayable by construction (RFC 8446 §2.3; RFC 8470 §5 discusses the replay
exposure for HTTP, and the same property holds for any application data). A
replayed mesh frame is not harmless: it can re-apply a block delta, re-trigger a
recovery completion, or duplicate an ACK. **Early data must remain disabled for
the mesh transport.** Resumption without 0-RTT keeps the replay question out of
scope entirely.

### 3.3 Quantified impact against the mesh throughput target

`AGENTS.md` states the mesh target as "10K ops/sec per node (CRDT merge is
O(log n))". Strictly this is a *CRDT merge* target, not a frame rate; the
network frame rate is bounded by the 50 ms coalescing window and 16 entries per
frame (`condition_sync.rs:56`, `transport.rs:27`), and by the 30 s repair
interval (`condition_sync.rs:57`).

Estimating conservatively — a worst case in which every merged op becomes its
own frame at the coalescing-window limit:

* 10,000 frames/s/node ÷ 16 entries/frame → 625 frames/s/node.
* Full TLS 1.3 handshake at 3,160/s (client) → 625 × (1/3160) ≈ **0.20 core** for
  handshakes on the sending side; the same again on the receiving side.
* With resumption at 6,735/s → ≈ **0.09 core** per side.
* If the frame rate ever approached 10,000/s (a different, much more demanding
  workload), full handshakes would need ≈ **3.2 cores** per side — beyond a
  single core, and far beyond the current cost of one HMAC over the same data.

**Conclusion:** at the *stated* merge-rate target, per-connection TLS with
resumption is affordable (order of a tenth of a core). At a frame rate one
order of magnitude higher it is not, and connection reuse becomes mandatory
rather than desirable. This must be **measured**, not assumed: §10 Phase 5 adds a
benchmark that reports handshakes/second, resumed fraction, and CPU per frame.

### 3.4 Peer identity check: certificate verification, application layer, or both?

**Both, in this specific split.** This is the most important integration
decision in the document.

* **Cryptographic endpoint identity** comes from the TLS handshake. On the server
  side, the client's certificate chain is verified during the handshake; the
  resulting verified identity is a property of the *connection*, not of the
  payload.
* **Claimed-identity consistency** stays in the application layer. Today the
  receiver selects the verification key from the *claimed* `node_id`
  (`transport.rs:1297`) — that selection must change. Under TLS the receiver must
  instead:
  1. obtain the **verified** peer identity from the TLS session
     (`ServerConnection::peer_certificates()` in rustls, or the peer identity
     resolved by the custom verifier);
  2. look up the configured peer by that **verified** identity;
  3. require that the envelope's `node_id` equals the verified identity
     (reject on mismatch);
  4. verify the envelope MAC with *that peer's* edge secret (C1) and apply the
     existing timestamp, size, bounds and `WalBatch` source checks.

If step 3 is omitted, an authenticated node could claim to be a different node
and the receiver would verify against the wrong edge secret. If step 1 is
skipped and identity is taken from the claim, the entire design collapses back
to the current defect.

Note the ordering constraint that already exists and must be preserved: the
roadmap requires "authentication failures occur before state mutation", and the
current code rejects before pushing onto the incoming queue
(`transport.rs:1297-1329`). The TLS handshake completes before any frame is
read, so a failed identity check still precedes state mutation.

### 3.5 Protocol-level effects of TLS on the wire

* **Frame sizes grow by the TLS record overhead** (a few dozen bytes per record,
  plus ~1.2 KB extra if a post-quantum keyshare is negotiated — see §7.3). The
  `MAX_FRAME = 64 KiB` bound applies to the *plaintext* frame, so it is unaffected;
  but the TCP payload will now exceed 64 KiB slightly. If a peer or an
  intermediate device enforces a 64 KiB frame limit, it must be told about TLS
  record overhead.
* **The `mac` field remains meaningful** under C1 (it is an HMAC over the
  plaintext, i.e. an end-to-end integrity check *within* the TLS session). Under
  C2 it becomes redundant.
* **`ts_ms` and `MAX_CLOCK_SKEW_MS = 30_000`** (`transport.rs:25`) remain the
  replay/freshness guard. TLS does not replace them: a TLS session is
  replayable at the application layer by anyone who can write into the session,
  and the freshness window is what limits that.

---

## 4. Identity and key management

This is the crux, and it is where most of the operational risk lives.

### 4.1 Option 4a — Self-signed certificate per node, pinned peer fingerprint

**Model.** Each node has one key pair and one self-signed certificate whose
identity encodes the node ID (e.g. a URI SAN `spiffe://ramshield/node/7`, or a
DNS SAN `node-7.ramshield.mesh`). Each node is configured with the expected
public identity of each of its peers.

**Two ways to pin, with different operational properties:**

* **P1 — Pin the SPKI hash** (HPKP-style, pin the SubjectPublicKeyInfo
  SHA-256). A certificate can be *renewed* (new validity dates, new serial)
  without changing the pin, as long as the key pair is reused. This is the
  desirable property, because certificate renewal is the routine event and pin
  churn is what causes outages.
* **P2 — Pin the whole certificate DER.** Simpler, but every renewal changes the
  pin on every peer.

**Recommendation: pin the SPKI hash, and implement it with a custom
`rustls::client::danger::ServerCertVerifier`** (the `danger` module is
explicitly provided for exactly this: "Something that can verify a server
certificate chain, and verify signatures made by certificates",
[rustls `client::danger`](https://docs.rs/rustls/latest/rustls/client/danger/)).
The custom verifier would: verify the chain against a pinned anchor, check the
SAN contains the expected node identity, check the validity window, and compare
the SPKI hash to the configured pin. It must also implement
`verify_tls12_signature`/`verify_tls13_signature` correctly (delegating to
rustls' helpers) — a hand-written verifier is a security-critical component and
must be reviewed like one.

*Alternative to the `danger` module:* add the peer's self-signed certificate to
a `RootCertStore` and use the default webpki verifier with a `ServerName` that
matches the SAN. This is simpler and stays on well-trodden code, at the cost of
pinning the whole certificate (P2). **This is the lower-risk starting point**,
and it is a good Phase 4 choice if the owner prefers minimal custom crypto code.
Recommend starting with the trust-anchor approach and only moving to SPKI pins
if renewal churn proves painful.

**Server-side peer identity (mTLS).** The server must require a client
certificate. rustls provides `WebPkiClientVerifier` and
`ClientCertVerifierBuilder` for this
([rustls `WebPkiClientVerifier`](https://docs.rs/rustls/latest/rustls/server/struct.WebPkiClientVerifier.html)).
Without client certificates, TLS authenticates only the server and **G1 is not
achieved** — the receiver still cannot tell which node connected.

**Provisioning:**
* Per node: one key pair + one certificate, generated **offline** and distributed
  to the node. The node holds its own private key (never shared with peers).
* Per peer link: the peer's **public** identity (certificate or SPKI pin).
  Unlike today, this is not a shared secret — it is a public value, so
  distribution is not a secret-distribution operation. That is a genuine
  simplification relative to the current model, where a secret must be placed at
  both ends of every edge.

**Storage:**
* Private key: file on disk with `0600`, owned by the service user, path
  configured (e.g. `/etc/ramshield/mesh/node.key`); or in a `Secret`/tmpfs mount.
  Private keys must be excluded from logs, from the dashboard config view, and
  from the SBOM/provenance artefacts.
* The `MeshConfig` `Debug` impl already redacts secrets
  (`crates/ramshield-config/src/sections/security.rs:193-211`); the new fields
  must be added to that redaction, and `crates/ramshield-config/src/validate.rs:523`
  already asserts the rendered config contains no key material — the new test must
  assert the same for the private key path/contents.

**Rotation:**
* Node key rotation is **per node**, not per edge. Today, rotating a shared edge
  secret requires a coordinated change at *both* ends of *every* edge touching
  that node. With per-node credentials, rotating a node's key requires changing
  the *peers'* pinned values for that node — still N-1 peers, but the change is a
  *public* value change, so it can be pushed by configuration management without
  the coordination risk of a secret hand-off.
* **Overlap window:** during rotation, peers must accept *both* the old and new
  pin for that node for a defined period. This requires the pin list per peer to
  be a *set*, not a scalar. Design the config for that from the start.

**Revocation:**
* There is no CRL/OCSP story here — the mesh is offline and rustls' webpki does
  not implement CRL checking. Revocation is therefore **manual**: remove the
  revoked node's identity from the peers' configured lists, or rotate the
  compromised node's key and roll the new pin out. This is a real limitation of
  the self-signed model and must be written down in the runbook, together with a
  target time-to-revoke.
* **A compromised node's private key cannot be un-compromised.** The only
  remedy is key rotation plus an operational response to whatever the compromised
  node did inside its own authorisation scope. TLS limits the blast radius to
  that scope; it does not undo the compromise.

**Mapping onto the operator's existing config.** The current mesh config is a
set of **parallel arrays** (`MeshConfig`, `security.rs:149-178`): `peers`,
`peer_node_ids`, `peer_auth_keys`, plus a vestigial `auth_key`. Validation
(`validate.rs:152-206`) enforces one entry per peer, unique node IDs, unique
edge secrets, minimum secret length. Adding TLS in the same parallel-array style
(`peer_tls_pins`, or `peer_transport` mode array) is possible but will be
awkward and error-prone — a missing entry in a parallel array shifts
meaning. **Recommendation: introduce a structured peer list** and keep the
parallel arrays as a deprecated-but-supported legacy form for one release, then
migrate. That is a larger config change and should be scoped separately.

### 4.2 Option 4b — Small internal CA

**Model.** One CA key (ideally held offline, in a separate signing host or
HSM). It signs per-node certificates. Each node trusts the CA (one trust
anchor) and is configured with the CA fingerprint.

**Buys:** one trust anchor instead of N−1 pins; certificate renewal does not
touch peer configuration at all (the CA re-signs); a natural place to encode
policy in issuance (node ID, role, validity period) rather than in each peer's
config; a natural path to a future two-tier hierarchy (cluster CA → site CAs).

**Costs:**
* The CA key is now the crown jewel of the whole mesh. Its compromise
  compromises every node. This is a strictly larger blast radius than the
  self-signed model, where compromising a node only affects that node.
* CA availability is needed for renewal. A mesh with a 2-year CA expiry and no
  offline renewal path is a scheduled outage.
* It introduces a *new* trust concept into a project that has deliberately kept
  the trust model minimal and auditable, and it partially contradicts the
  roadmap's "keep the architecture deliberately small" framing.

**Rotation:** CA key rotation is the hard case (needs a cross-signed or
dual-trust transition). Node cert rotation is easy (re-sign with the same CA).

**Revocation:** still no CRL/OCSP in rustls' default stack; the practical
mechanism is to remove the CA trust anchor or issue a "revoked" node list that
the mesh checks. Weak.

**Recommendation:** the internal CA is the better model **if and only if** the
deployment already has CA infrastructure (HashiCorp Vault, step-ca, an existing
corporate PKI, cfssl) and an offline CA key. If the mesh is the only thing that
needs PKI, **the self-signed + pin model is the better fit**: it has fewer
moving parts, a smaller blast radius, and no offline-signing-host dependency.
RamShield is a self-hosted product; its users frequently have no PKI at all.

### 4.3 Option 4c — Per-edge certificates

Give each edge its own certificate/key, held by both endpoints.

**Why not:** this reproduces the current defect exactly. A per-edge private key
held by both endpoints means the two endpoints remain cryptographically
indistinguishable — the same within-edge indistinguishability the F-01 patch
worked to eliminate across edges. Per-edge certificates buy confidentiality and
standard tooling but **do not close F-01a**.

The only variant of per-edge certificates that does close the gap is
"per-edge certificate, **per-node private key**" — i.e. each node holds a
distinct key per edge, and the certificate binds both node identities plus the
edge. That is more complex (N−1 keys and N−1 certificates per node) with no
security advantage over one certificate per node. **Rejected**: it trades
simplicity for nothing.

### 4.4 Recommendation

**Per-node self-signed certificate + per-peer pinned identity (4a), mTLS,
TLS 1.3-only, with the HMAC edge secret retained during migration (Option C1).**

Rationale in one line: it is the smallest change that adds per-endpoint identity
without changing the existing edge-secret key-distribution model, so it can be
implemented as an additive layer under the roadmap's "preserve current key
distribution" default — with the owner's approval for the parts that
unavoidably require it.

### 4.5 Configuration schema — is a new schema required, and is it backward
compatible?

**Yes, a new schema is required.** There is no existing representation for a
node's own identity or a peer's expected identity. The current schema has no
version field at all: `Config` (`crates/ramshield-config/src/lib.rs:22-48`) is a
flat struct of `#[serde(default)]` sections, and `MeshConfig` is a flat section
of parallel arrays.

**Backward compatibility is achievable and should be designed in.** The change is
**additive and versioned by presence, not by a version number**:

```toml
[mesh]
enabled = true
node_id = 1
listen_addr = "10.0.0.11:7900"
peers = ["10.0.0.12:7900"]
peer_node_ids = [2]
peer_auth_keys = ["<edge-1-2-hex-secret>"]   # unchanged, still required in C1
auth_key = ""

# --- new, all optional, absent => today's exact behaviour ---
mesh_tls_enabled = false                 # global switch; default false
mesh_tls_cert = "/etc/ramshield/mesh/node.crt"
mesh_tls_key  = "/etc/ramshield/mesh/node.key"
mesh_peer_modes = ["hmac"]               # parallel to peers; "hmac" | "tls"
mesh_peer_pins = [""]                    # parallel to peers; hex SPKI pin(s), "" = unset
```

Properties:
* Every new field has `#[serde(default)]`; an existing config file continues to
  load and behaves **identically** (`mesh_tls_enabled = false` ⇒ the transport
  code path is byte-for-byte the current one).
* The roadmap requires "Preserve configuration compatibility unless a necessary
  change is approved and versioned" (§2, F-01, Implementation). This satisfies
  that: it is additive, defaults preserve behaviour, and the new fields are
  versioned by the presence of `mesh_tls_enabled`/`mesh_peer_modes`.
* A **config schema version field** is not required for this change, but adding
  one (`config_version = 2` at the top level, defaulting to 1) is worth doing at
  the same time if the owner wants a general mechanism, because the parallel-array
  peer model is already straining (§4.1) and the next config change will want it.
* Environment overrides (`crates/ramshield-config/src/env.rs:107-146`) must be
  extended for the new fields, following the existing
  `RAMSHIELD_MESH__*` pattern.
* `MeshConfig`'s custom `Debug` impl (`security.rs:193-211`) must redact the new
  secret-bearing fields, and `validate.rs:523`'s "rendered config contains no
  key material" test must be extended.

### 4.6 What the repository owner must approve (identity-specific)

1. **A per-node asymmetric key pair** (new key material on every node) —
   an amendment to the "Keep HMAC … do not introduce asymmetric cryptography"
   directive, and a change to the key-distribution model (nodes now hold private
   keys, not just shared secrets).
2. **A new key material type on disk** (private key files) and its protection
   (permissions, ownership, rotation procedure).
3. **The certificate/CA model** — self-signed + pins vs internal CA (§4.4
   recommends self-signed + pins; the owner may prefer a CA, and that choice
   changes the operational dependency set).
4. **The new configuration fields** and the policy for when `peer_auth_keys`
   may be removed (C1 vs C2).
5. **Adding the TLS dependency set** (§7), against the project's dependency
   posture and the `docs/FEATURE_0.5_L7_MESH_UPSTREAM.md:177-179` decision to
   avoid a TLS stack in the daemon.

---

## 5. Migration and rollback

### 5.1 Staged path

**Stage 0 — Decision and approval (no code).** The owner decides between
"accept residual risk" (D1) and "approve an asymmetric migration" (this
document). If the latter, the roadmap is amended (§9) and a separate workstream
is opened. F-01 is closed either way: as *HMAC hardening PASS / peer-isolation
PARTIAL*, or as *peer-isolation RESOLVED by a later, separately approved
mechanism*.

**Stage 1 — Transport abstraction (no behaviour change).** Introduce a
`MeshStream` abstraction (or a small enum) that the existing code paths use
instead of a raw `TcpStream`, with two implementations: plain and TLS. Default
plain. This stage produces no externally visible change, and it makes the TLS
work in Stage 2 a small diff. Exit gate: existing mesh tests pass unchanged.

**Stage 2 — TLS transport behind a disabled-by-default flag.** Implement the
handshake, mTLS, the verified-identity lookup, the claimed-vs-verified check, and
the pin/anchor validation. `mesh_tls_enabled = false` by default. Exit gate:
new integration tests in `crates/ramshield-mesh/tests/` prove — wrong pin
rejected, unconfigured peer rejected, expired certificate rejected, mismatched
`node_id` in the envelope rejected, TLS-protected ACK accepted, and the
no-TLS path unchanged.

**Stage 3 — Mixed mode.** Both edges of an edge must agree on the transport
mode. **Mode is a configuration fact, not an in-band negotiation** (see §8.4
for why). `mesh_peer_modes` says per peer whether this node speaks TLS to it.
Because it is configuration, an attacker on the path cannot downgrade an edge by
tampering with bytes — the worst they can do is break it. That is a strictly
better failure mode than sniffing-based negotiation.

Practical sequence per edge:
1. Generate node key pairs and certificates for **both** endpoints; install the
   private keys on each node; install the peer's public identity on each node.
2. Verify with `openssl s_client`/`openssl x509` out of band.
3. Flip node A's `mesh_peer_modes[peer_B] = "tls"` (A now speaks TLS to B).
   A↔B breaks *until* B is flipped, because B still speaks plain TCP.
4. Flip node B. The edge is now TLS on both sides.
5. Observe, then proceed to the next edge.

A brief edge outage during steps 3–4 is acceptable and expected. A **safer**
variant is to run the TLS listener on a **separate port** during the mixed
period, so both transports are simultaneously live and the flip is a
connection-target change with no outage at all. Cost: a second listener, a
second port in the firewall, and a temporary duplication in the accept path.
**Recommended for a first production migration**; the single-port mode flip is
acceptable for a test cluster.

**Stage 4 — Full cutover.** All edges TLS. `peer_auth_keys` still populated
(C1) but now redundant for identity. Observation period; measure.

**Stage 5 — Optional simplification (C2).** Remove the envelope MAC and the
`peer_auth_keys` requirement. **This is a separate approval**: it changes the
key-distribution model and is the point at which the roadmap's "preserve current
key distribution" default is genuinely abandoned. Do not bundle it with Stage 4.

### 5.2 Rollback

Because mode is configuration-driven and additive, rollback is a configuration
change plus a restart:

* **Roll back an edge:** set `mesh_peer_modes[peer] = "hmac"`. The peer must be
  rolled back too (it will reject TLS or fail to parse it). The HMAC path is
  unchanged code, so rollback is not a code path that has been deleted.
* **Roll back a whole fleet:** set `mesh_tls_enabled = false` everywhere; the
  transport returns to the current behaviour.
* **Rollback must not require reverting a commit.** The TLS code must remain
  present and selectable; only the configuration changes. This is a strong
  argument for keeping TLS behind a flag rather than removing the old path.
* **Rollback is only clean if the HMAC path is preserved** — which is another
  argument for C1 over C2 during the migration.

### 5.3 What happens to the existing config keys

* `peer_auth_keys`: **keep** under C1. Under C2, keep for one release with a
  deprecation warning, then remove in a documented, versioned change.
* `auth_key`: already vestigial (`security.rs:172-177`, `transport.rs:548-551`,
  `F01_REPORT.md` §7.4). TLS does not change that. If C2 lands, it can finally be
  removed, and the roadmap's "preserve configuration compatibility" is honoured by
  a deprecation window.
* `peers` / `peer_node_ids`: unchanged in the additive design. If the owner
  approves the structured-peer-list refactor (§4.1), that is a separate,
  versioned config change with a deprecation window.

---

## 6. Performance

### 6.1 Published handshake costs (cited)

| Measurement | Value | Source |
|---|---|---|
| rustls 0.23.37 (aws-lc-rs 1.16.0), Xeon E-2386G, TLS 1.3 **full** handshake, ECDSA | 3,160/s client, 6,786/s server | [rustls perf report 2026-03-07](https://rustls.dev/perf/2026-03-07-report/) |
| rustls, TLS 1.3 **resumed** handshake | 6,735/s client, 7,249/s server | same |
| rustls, TLS 1.3 **full** handshake, RSA | 4,767/s client, 2,357/s server | same |
| Localhost lab, Python/OpenSSL 3.5, TLS 1.3 full handshake | p50 **1.507 ms** (p95 2.948 ms) | [ShopperCove lab 2026-09-30](https://www.shoppercove.com/blog/openssl-tls-handshake-full-vs-resume-localhost-lab) |
| Same lab, bare TCP connect | p50 **0.021 ms** (≈72× cheaper) | same |
| Same lab, `openssl s_time` TLS 1.3 | 617.3 new vs 922.7 reuse conn/s (**≈1.49×**) | same |
| Same lab, TLS 1.2 | 714.3 new vs 2,946.7 reuse conn/s (≈4.13×) | same |
| TLS 1.3 handshake crypto work (ECDSA) | 2× KeyGen, 1× SecretGen, **2× Verify** (verify is the expensive op) | [wolfSSL TLS 1.3 performance analysis](https://www.wolfssl.com/tls-1-3-performance-part-2-full-handshake-2/) |
| TLS 1.3 round-trip structure | 1-RTT full handshake; 0-RTT only with PSK and replayable | [RFC 8446](https://www.rfc-editor.org/rfc/rfc8446) §2.3, §4.2.1 |

Note the asymmetry the benchmarks make visible: **ECDSA verification (the
server's cost) is 2 Verify operations per TLS 1.3 handshake**, and RSA is
cheaper to verify but more expensive to sign. For a mesh where both ends
initiate connections, the choice of certificate key algorithm matters and
should be measured on the target hardware. Ed25519 (available in both `ring`
and `aws-lc-rs`) is typically the fastest to verify — a reason to prefer it for
node certificates.

### 6.2 Per-connection vs reused connections — the decisive comparison

Current cost per frame, today: one TCP connect (1 RTT) + one HMAC-SHA256 over
≤64 KiB + one ACK read. **No per-connection CPU.**

| Model | Per frame, added CPU (client side) | Per frame, added latency |
|---|---|---|
| Plain TCP + HMAC (today) | ~0 (one HMAC) | 1 RTT (TCP) |
| TLS 1.3 full handshake per frame | ≈ 1/3160 s ≈ **316 µs** | TCP 1 RTT + TLS 1 RTT = **2 RTT** |
| TLS 1.3 resumed per frame | ≈ 1/6735 s ≈ **148 µs** | **2 RTT** |
| TLS 1.3 full/resumed, **reused connection** | ≈ **0** (amortised over the connection) | **1 RTT** (TCP only) |

The latency column matters as much as the CPU column: per-frame TLS doubles the
round trips of every mesh exchange, which directly affects ACK latency
(`MAX_APPLICATION_ACK_WAIT = 2 s`, `transport.rs:37`) and therefore how quickly
a sender may advance its replication cursor.

### 6.3 Interaction with the 10K ops/sec/node target

The `AGENTS.md` target of "10K ops/sec per node (CRDT merge is O(log n))" is a
**merge** rate, not a frame rate; frames are coalesced over a 50 ms window and
chunked at 16 entries (`condition_sync.rs:56`, `transport.rs:27`).

* At a 625 frames/s derived rate, per-connection TLS with resumption costs
  ≈0.09 core per side (§3.3) — affordable, but not free, and it must be
  measured.
* At a hypothetical 10K frames/s, it costs ≈3.2 cores per side for full
  handshakes — **not affordable**.
* **Therefore connection reuse is not optional in the general case.** It is
  optional *only* if the measured frame rate stays low. The honest engineering
  position: **TLS with resumption is a prerequisite; connection reuse is the
  follow-on that makes TLS scale to the stated target under adverse conditions.**

### 6.4 Memory

* Per live TLS connection, rustls holds the handshake transcript, key schedule
  and I/O buffers. Under the current one-frame-per-connection pattern, peak
  concurrent connections are bounded by `MAX_PEER_WRITERS = 64`
  (`transport.rs:35`) outbound and `MAX_PEER_READERS = 256` (`transport.rs:34`)
  inbound — so the memory is bounded, but the *allocation churn* is not: at
  625 handshakes/s, the per-connection allocations are the dominant cost.
* With connection reuse, memory becomes O(peers) rather than O(in-flight
  frames) — a further argument for reuse.
* Session/ticket caches: rustls defaults to 256 server sessions in memory with
  the `std` feature; the client cache should be sized to the peer count
  (≤256 peers, `MAX_CONFIGURED_PEERS`, `transport.rs:33`).

### 6.5 Frame size overhead

TLS adds ~29–40 bytes of record overhead per record plus the handshake
flight. A frame is written as a single `write_all` of JSON plus `b"\n"`, so it
becomes one or two TLS records. With rustls' default features, `prefer-post-quantum`
is on, which adds an X25519MLKEM768 keyshare (~1.2 KB) to the ClientHello —
**per handshake**. That is significant for a mesh that opens many short-lived
connections. **Recommendation: disable `prefer-post-quantum` for the mesh
transport** (i.e. do not use rustls' default feature set), or accept the byte
cost as the price of post-quantum readiness. This is a small decision with a
large bandwidth effect and should be made explicitly rather than by default.

---

## 7. Dependencies and supply chain

### 7.1 Proposed dependency set

| Crate | Version | Licence | Why |
|---|---|---|---|
| `rustls` | 0.23.x (0.23.40 in the local registry) | Apache-2.0 OR ISC OR MIT | TLS 1.3 implementation |
| `tokio-rustls` | 0.26.x | MIT OR Apache-2.0 | async integration with the existing tokio runtime (`tokio` with `net`, `io-util`, `time`, `sync` — `crates/ramshield-mesh/Cargo.toml`) |
| `rustls-pki-types` | 1.x | MIT OR Apache-2.0 | certificate/key DER types |
| `rustls-webpki` | 0.103.x | ISC | chain validation (transitive) |
| provider: `ring` | 0.17.x | Apache-2.0 AND ISC | crypto provider |
| provider: `aws-lc-rs` (+ `aws-lc-sys`) | 1.16.x | Apache-2.0 OR ISC | alternative crypto provider |
| `rcgen` | 0.13.x | MIT OR Apache-2.0 | **dev-dependency only** — generating test certificates |
| `pem` / `x509-parser` | — | — | **dev-dependency only**, if used for test fixtures |

Transitive additions for `rustls` itself: `once_cell`, `subtle`, `zeroize`,
`rustversion` (and `untrusted` via `rustls-webpki`).

### 7.2 Licence and policy compatibility

`deny.toml` allows `["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause",
"ISC", "Unicode-3.0", "Zlib"]`. Every proposed crate is inside that set
(`rustls`: Apache-2.0 OR ISC OR MIT; `rustls-webpki`: ISC; `ring`: Apache-2.0
AND ISC; `aws-lc-rs`: Apache-2.0 OR ISC; `rcgen`: MIT OR Apache-2.0).
`deny.toml` also sets `bans.wildcards = "deny"` (no `*` version requirements —
satisfied by the table above), `bans.multiple-versions = "warn"`, and
`sources.unknown-registry = "deny"` with only crates.io allowed (satisfied).
**No `deny.toml` change is required** — but the existing "remaining cargo-deny
duplicate-version warnings are intentionally visible" note in `BACKLOG.md`
(A-034) means the new tree may add more warnings that must be *reviewed*, not
suppressed.

### 7.3 Crypto provider choice

rustls 0.23 defaults to `aws-lc-rs`; `ring` is an alternative feature
([rustls crate docs](https://docs.rs/rustls/0.23.41/rustls/index.html)). The
relevant trade-offs for *this* project:

* `aws-lc-rs` / `aws-lc-sys`: builds C and assembly; for non-FIPS builds
  "the only build requirement is a C/C++ compiler" (pre-generated bindings, no
  bindgen/CMake/Go), but **FIPS builds additionally require CMake, Go and
  bindgen** ([aws-lc-rs FAQ](https://aws.github.io/aws-lc-rs/faq.html)). It is
  the default and is the more actively maintained provider.
* `ring`: pure-Rust-plus-assembly, no C toolchain requirement beyond a compiler,
  but it is not FIPS-capable and has a narrower platform matrix.
* The `Containerfile` builds in `rustlang/rust:nightly-2026-08-29` (Debian-based,
  has a C compiler) and ships a `distroless/cc-debian12:nonroot` runtime, so
  either provider is buildable. `aws-lc-sys` links statically, which suits
  distroless; this should be verified in CI rather than assumed.
* **Recommendation:** make the provider an explicit, single choice in the
  workspace (`rustls` with `default-features = false` plus the chosen provider),
  so there is exactly one crypto stack in the binary and the licence/audit
  surface is unambiguous. Default to `aws-lc-rs` unless FIPS or platform support
  argues for `ring` — but check the target platforms first (RamShield's
  `Containerfile` is Debian; `ring` also supports the target well).
* Also decide explicitly whether to keep rustls' default features. `default`
  = `["aws_lc_rs", "logging", "prefer-post-quantum", "std", "tls12"]`.
  Disabling `tls12` **removes the TLS 1.2 fallback and therefore the TLS 1.2
  downgrade surface** (§8.4) — strongly recommended. Disabling
  `prefer-post-quantum` saves ~1.2 KB of ClientHello per handshake (§6.5).

### 7.4 What the "no new dependency" posture implies — honestly

The repository does not state a blanket "no new dependencies" rule. What it
actually contains, which this design must engage with:

* Roadmap v1.2 §0, **Out of scope unless separately approved**: *"Unrelated
  dependency upgrades, cleanup, or public API redesign."* and *"Ed25519 or any
  asymmetric-cryptography migration."*
* Roadmap v1.2 §1 step 7: *"Inspect workspace manifests, feature definitions,
  scripts, and required system dependencies."* — dependencies are an explicit
  review item.
* `docs/FEATURE_0.5_L7_MESH_UPSTREAM.md:177-179`: for the upstream webhook,
  *"production TLS can be terminated by the deployment's existing local proxy.
  […] This avoids adding a second TLS stack to the daemon."* — a deliberate
  product decision **not** to embed TLS.
* `docs/development/MESH_WORKING_ROADMAP.md:19`: *"add a metrics dependency only
  if the deployment already standardizes on one"* — a consistent, conservative
  pattern of not pulling in libraries for the sake of a feature.
* `docs/SECURITY_MODEL.md:43-45` (Supply chain): committed lockfile, `--locked`
  release builds, `cargo audit` + `cargo deny`, SBOM and provenance for release
  archives.

**Analysis.** The posture is not "never add a dependency"; it is *"do not add
dependencies casually, and do not add one whose operational/audit cost exceeds
its benefit."* A TLS stack is the archetypal case where that judgement goes
against the change: it is a large, security-critical, C-adjacent dependency tree
with its own CVE surface, and it directly contradicts the stated "no TLS stack in
the daemon" decision.

Two honest ways to satisfy the posture:

1. **Amend it explicitly, for TLS only**, with the owner accepting the audit
   surface: committed lockfile update, `cargo deny`/`cargo audit` clean, SBOM and
   provenance regenerated, the dependency tree documented in
   `docs/SECURITY_MODEL.md` §Supply chain, and a policy for provider updates.
2. **Keep the TLS stack out of the daemon** (Option B), accepting that peer
   identity must then be propagated explicitly to the mesh (§2, Option B).

There is no third option that gets per-endpoint identity without either a TLS
stack or an asymmetric signing library. If even a signing library is unacceptable,
then **D1 (accept the residual risk)** is the only remaining path, and it must be
recorded as an accepted residual risk in the mesh release gate.

---

## 8. Risks

### 8.1 Certificate expiry causes a mesh partition

**Mechanism.** rustls' webpki verification rejects a certificate outside its
validity window. An expired node certificate makes the edge fail — and because
the mesh's failure mode is "peer unreachable", the symptom is a silent
convergence stall, not an error message.

**Mitigations.**
* Long validity windows (1–3 years is reasonable for an internal mesh) — but see
  §8.2 on the clock, and §4.1 on renewal churn.
* **Expiry monitoring that is separate from the mesh**: expose the configured
  `notAfter` of the local certificate and of every configured peer identity as an
  operator-visible signal, with an alert at (say) 90/30/7 days out. A partition
  at 03:00 because nobody noticed an expiry is an unacceptable outcome for a
  security product.
* **Overlap on renewal**: peers accept both the old and the new pin of a peer
  during a rotation window (pin sets, not scalars — §4.1).
* A documented emergency procedure: how to re-pin a peer whose certificate
  expired, and how to do it without first re-establishing trust (out-of-band
  channel required — §4.1 "revocation is manual").
* Consider an explicit `notAfter` grace in the custom verifier — with the
  understanding that it weakens the property; the safer answer is monitoring
  plus a runbook.

### 8.2 Clock skew affects certificate validity

**Mechanism.** Certificate validity is checked against wall-clock time
(`SystemTime`), and rustls uses the system clock by default (`ServerConfig.time_provider`
/ `ClientConfig.time_provider` allow overriding it). A node whose clock is wrong
by more than the certificate's remaining validity window will reject valid peers
or accept expired ones.

**Important nuance.** The mesh *already* depends on clock accuracy — the
`ts_ms` freshness window is `MAX_CLOCK_SKEW_MS = 30_000`
(`transport.rs:25`), enforced at `transport.rs:1304` and `transport.rs:1227`. So
the deployment already requires NTP-grade synchronisation. TLS does not add a
new class of clock dependency, but it **raises the tolerance from seconds to
days/months**: a 30-second skew requirement is a nuisance; a 30-day skew
requirement is an outage. A node whose clock jumps backwards by years will
present an expired certificate.

**Mitigations.**
* The mesh's existing 30 s skew requirement should be documented as also covering
  certificate validity, and the mesh should *measure* and report clock skew
  against peers (it is measurable from `ts_ms` on every frame) rather than
  assuming it.
* Use `not_before` with a generous backdate (e.g. 24 h) and a `not_after` with a
  generous forward date, so ordinary NTP drift and small misconfigurations do
  not break the edge.
* Consider issuing certificates with a long `not_before` backdate specifically to
  absorb clock problems.

### 8.3 Pinning mistakes cause an outage

**Mechanism.** A wrong pin, a wrong file, a truncated hex string, or a peer
configured with the *other* end's identity produces a handshake failure on every
frame of that edge.

**Mitigations.**
* **Validate at startup, not at first use.** At boot, when `mesh_tls_enabled`
  is true, the daemon should load the local key/certificate and *every*
  configured peer identity and verify that they are well-formed and internally
  consistent (parseable, correct SAN/node-ID mapping, non-empty, correct length)
  — fail fast with a clear message, exactly as `validate_mesh` already does for
  the current config (`validate.rs:152-206`). This converts a mid-flight
  partition into a startup failure with an actionable error.
* **A pre-flight self-test** that dials each peer and performs a handshake
  before declaring the mesh healthy.
* **Overlap lists** so that a rotation never needs an atomic two-sided change.
* **A clear diagnostic on handshake failure** — "pin mismatch" versus
  "certificate expired" versus "untrusted issuer" must be distinguishable in the
  logs, and must not print key material.

### 8.4 Downgrade attacks

**Mechanism.** If the transport mode is negotiated *in band* (e.g. by sniffing
the first bytes, or by a client-supplied "can we do plain?" flag), an active
attacker on the path can strip the TLS ClientHello and force the plain/HMAC
path — re-opening exactly the identity gap TLS was adopted to close. This is
the classic TLS-stripping attack and it is why TLS must be negotiated with an
authenticated, integrity-protected mechanism, or not at all.

**Mitigations for this design.**
* **Mode is configuration, not negotiation.** `mesh_peer_modes` states, per peer,
  which transport this node uses. There is no in-band negotiation to strip. An
  attacker can cause a connection failure (a DoS, which TLS does not prevent) but
  cannot cause a *silent downgrade to a weaker authenticated mode*, because the
  weak mode is only ever used when the operator configured it.
* **TLS 1.3 only** (disable rustls' `tls12` feature) removes the TLS 1.2
  fallback and its downgrade path, and removes the legacy renegotiation/ CBC
  machinery entirely.
* **Pin the negotiated version**: the accepted version should be asserted to be
  TLS 1.3, and the accepted cipher suite should be from an approved set.
* **No "fall back to plain on TLS failure"** behaviour. This is the single most
  important implementation rule for this design. A failure to authenticate must
  be a failure, never a downgrade.
* If the implementation ever adds a protocol-version field to the envelope, it
  must be inside the authenticated payload — the existing MAC covers
  `(ts_ms, node_id, body)` (`transport.rs:1183`), so adding a version field to
  `body` or to the signed tuple keeps it authenticated.

### 8.5 The added failure surface of a TLS stack versus a hand-rolled HMAC

Today the transport's entire cryptographic surface is `sign`/`verify`
(`transport.rs:1795-1812`): `Hmac<Sha256>` from the `hmac` crate, ~20 lines,
over a canonical JSON payload, with the key selection and the identity checks
written by the project and fully visible in one file. Replacing or supplementing
it with rustls adds:

* A certificate parser and chain validator (`rustls-webpki`) operating on
  attacker-controlled bytes.
* A key-schedule and record layer with AEAD, sequence numbers, replay
  protection, and per-record state.
* A session/ticket subsystem with its own lifetime and key-management semantics
  (`ticketer`, `session_storage`).
* A provider abstraction with two backends and a FIPS-specific build path.
* An ecosystem CVE surface (rustls, webpki, ring/aws-lc) that must be tracked
  and patched — with `cargo audit` and `cargo deny` in CI, per
  `docs/SECURITY_MODEL.md`.

**Mitigations.**
* Keep the surface small: TLS 1.3 only, one provider, no client-side
  certificate compression, no early data, `key_log` disabled (rustls does not log
  key material by default — `ServerConfig.key_log` default does nothing), and no
  `enable_secret_extraction`.
* Bound the handshake: a **separate, tighter handshake timeout** than the 5 s
  frame-read timeout, so a stalled handshake cannot occupy one of the 256
  `MAX_PEER_READERS` permits indefinitely. A peer that cannot complete a
  handshake in, say, 2 s is failed fast.
* Keep the existing HMAC under C1 so that a TLS-layer defect does not remove the
  last line of defence during the migration period.
* Retain the existing per-frame bounds (`MAX_FRAME`, entry caps, digest
  validation) — TLS does not make the application payload safe.

### 8.6 New DoS surface: TLS handshakes are CPU-expensive

TLS *adds* work that an attacker can force. A flood of TCP connections that
each trigger a full handshake costs the server asymmetric crypto per attempt.
This is the mirror image of the problem RamShield exists to solve.

**Mitigations.**
* Session resumption means a repeat client pays far less than a full handshake.
* Rate limits and connection-count caps at the listener (the existing
  `MAX_PEER_READERS` semaphore and the outbound `MAX_PEER_WRITERS` semaphore are
  the right shape).
* Consider an initial-rate limiter on the accept path.
* **Do not** let the TLS handshake path bypass the XDP-level protections: the
  mesh port should be protected like any other.
* Be explicit in `docs/SECURITY_MODEL.md` that TLS does not mitigate volumetric
  attacks and that the mesh is a control plane, not a data plane.

### 8.7 Risk register summary

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R1 | Expired certificate → partition | Medium | High | Long validity, expiry alerting, pin overlap, runbook (§8.1) |
| R2 | Clock skew → spurious validity failure | Medium | High | NTP enforcement, backdated `not_before`, skew measurement (§8.2) |
| R3 | Pinning mistake → outage | Medium | Medium | Startup validation, pre-flight self-test, overlap lists (§8.3) |
| R4 | Downgrade to HMAC | Low (if config-driven) | **Critical** | Mode is config, TLS1.3-only, no fallback-on-failure (§8.4) |
| R5 | Claimed-vs-verified identity confusion | **High if implemented naively** | **Critical** | Verified identity drives key selection; claimed ID must match (§3.4) |
| R6 | TLS stack defect / CVE | Medium | High | Small surface, one provider, `cargo audit`/`deny` in CI (§8.5) |
| R7 | Handshake CPU exhaustion | Medium | Medium | Resumption, rate limits, handshake timeout, XDP protection (§8.6) |
| R8 | Per-frame handshake cost breaks the throughput target | Low at current rates | Medium | Resumption (mandatory), connection reuse (strongly recommended), benchmark (§6) |
| R9 | `stunnel`/sidecar mis-maps peer identity → gap not closed | High if Option B chosen naively | **Critical** | Explicit peer-identity propagation; otherwise do not use Option B (§2 B) |
| R10 | Rollback after C2 (HMAC removed) | — | High | Keep C1 (HMAC retained) so rollback is a config change (§5.2) |

---

## 9. Approval gates and open questions

### 9.1 Roadmap clauses that a TLS adoption would need to amend

Quoted from `ramshield-remediation-roadmap-v1.2.md`, with the required change:

| # | Clause | Current text (abridged) | Required change |
|---|---|---|---|
| G-1 | Executive directive, cryptography | *"Keep HMAC. Do not add Ed25519, asymmetric signatures, or a public-key identity system. Default to retaining the current key-distribution model during this remediation. … Do not implement pairwise-key migration without explicit owner approval."* | Needs an explicit, dated exception or an amendment permitting a **per-node asymmetric credential** (certificate) **in addition to** the existing edge secrets. Note this clause is about *this remediation*; a separately approved workstream may sidestep it, but the owner should say so explicitly. |
| G-2 | §0, Out of scope | *"Ed25519 or any asymmetric-cryptography migration."* and *"Unrelated dependency upgrades, cleanup, or public API redesign."* | TLS is an asymmetric-cryptography migration. Must be moved into scope for a named workstream. The dependency clause is "unrelated" upgrades — a *related* dependency addition arguably does not fall under it, but this should be confirmed rather than assumed. |
| G-3 | §2 F-01, Default decision | *"Preserve current key distribution for this remediation unless the owner explicitly approves migration. Pairwise HMAC may be recommended, but must not be implemented unilaterally."* | Explicit owner approval required for any change to the key-distribution model. Option C1 preserves it and still needs approval because per-node private keys are new key material. |
| G-4 | §2 F-01, Implementation | *"Preserve configuration compatibility unless a necessary change is approved and versioned."* | The additive `mesh_tls_*` / `mesh_peer_modes` / `mesh_peer_pins` fields satisfy this, provided they are approved and versioned. |
| G-5 | §2 F-01, Acceptance/reporting | *"If isolation is not provided, mark it as residual risk only if the owner accepts that risk."* | The owner must either **accept** the residual risk (closing F-01a as accepted) or **approve** the asymmetric migration. This is the pivotal decision. |
| G-6 | §13, Final release gates (Mesh verdict) | *"… HMAC limitations are documented and accepted by the owner …"* | If TLS lands, this gate must be rewritten to describe the *post-TLS* state; if TLS does not land, the acceptance must be recorded explicitly, not left implicit. |
| G-7 | §11, Execution order | F-01 is step 1, F-02…F-08 follow, with *"one core finding per patch"* and a stop-for-review between steps. | A TLS workstream must be a **separate, sequenced workstream**, not interleaved into the F-01…F-08 sequence, and must not be started before F-02…F-08 land — otherwise the identity change and the correctness changes land together, which the roadmap's patch discipline exists to prevent. |
| G-8 | §12, Verification/reporting | *"Do not combine results from different SHAs."* / §14 *"do not expand scope"* | Any TLS implementation must follow the same exact-SHA evidence discipline, and Phase 0 must be its own reviewed decision record. |

### 9.2 Repository documents that would need updating (not gates, but
consequences)

* `docs/SECURITY_MODEL.md` — the trust model, the supply-chain section (new
  dependency tree), and the explicit statement that TLS does not stop a flood.
* `docs/FEATURE_0.5_L7_MESH_UPSTREAM.md` — §2 "Key provisioning" and "What the
  guarantee is, and is not" (currently the authoritative statement of the
  residual risk); §3's "avoids adding a second TLS stack to the daemon"
  rationale is invalidated by an in-process TLS decision and must be revisited.
* `docs/development/AUDIT_FINDINGS_BACKLOG.md` — F-01a's status changes from
  `DEFER` to whatever the owner decides.
* `docs/development/audit-2026-10-20/F01_REPORT.md` — §7 residual-risk statement
  is the historical record; it should be annotated, not rewritten.
* `AGENTS.md` — the mesh throughput target and the hot-path guidance may need a
  TLS-aware note.

### 9.3 Open questions requiring an answer before implementation

1. **Does the owner approve an asymmetric migration at all, or is the residual
   risk accepted?** Everything else is downstream of this. (G-5)
2. **Self-signed + pins, or an internal CA?** Depends on whether the deployment
   has PKI infrastructure and an offline CA key-holder. The CA model has a larger
   blast radius; the self-signed model has a larger operational burden per
   renewal. (§4.4)
3. **What is the certificate/key custody story?** Where does the private key
   live, who can read it, who performs rotation, and is there an offline
   procedure for "the node's certificate expired and nobody is available"?
4. **Is the structured-peer-list config refactor in scope, or does the design
   stay on parallel arrays?** Parallel arrays are already a footgun; the TLS
   fields multiply them. (§4.5)
5. **Is connection reuse (multiplexing) in scope for this workstream, or is
   TLS-with-resumption accepted as sufficient?** This determines whether the
   transport change is "small" or "structural", and it interacts with the F-02/F-03
   ACK-binding work. (§3.2, §6.3)
6. **Which crypto provider, and which rustls feature set?** `ring` vs
   `aws-lc-rs`; whether `tls12` is disabled (recommended: yes); whether
   `prefer-post-quantum` is disabled (recommended: yes, unless PQ readiness is
   a requirement). (§7.3)
7. **What is the target revocation time-to-revoke, and is manual revocation
   acceptable?** Without CRL/OCSP, revocation is a configuration push. (§4.1)
8. **Is certificate expiry monitoring a release gate?** If the answer is no, R1
   (expired certificate → silent partition) is accepted as a risk and must be
   recorded. (§8.1)
9. **Does the mesh need to run behind NAT or through a load balancer?** The
   current IP binding (`transport.rs:1280`) already assumes routable, stable peer
   addresses. TLS with a hostname SAN relaxes that, which is an improvement, but
   it also changes the deployment model.
10. **Does the deployment need FIPS?** This determines the crypto provider and
    whether `aws-lc-rs` must be built in FIPS mode (which needs CMake/Go/bindgen
    per the aws-lc-rs FAQ) — a build-pipeline change, not just a manifest change.
11. **What is the acceptable mixed-mode outage?** Zero-downtime (second port)
    versus a brief per-edge outage (in-place flip). (§5.1)
12. **Is `stunnel`/a sidecar already deployed in the target environments?** If
    yes, Option B deserves a serious re-evaluation, because it would satisfy the
    dependency posture at the cost of the peer-identity propagation problem.

---

## 10. Phased implementation plan

Each phase ends in something **reviewable and testable**. Sizing is indicative
(engineer-days) and excludes review latency. All phases are subject to Phase 0
approval and to the roadmap's patch discipline (one core concern per patch,
failing-before regression test, exact-SHA evidence, independent review).

**Phase 0 — Decision record and roadmap amendment (0.5–1 d, no code).**
Deliverable: a signed decision record choosing among {accept residual risk,
TLS/Option A+C1, Ed25519/Option D2, sidecar/Option B}; the exact roadmap
clauses to be amended (G-1…G-8); and, if TLS is chosen, the answers to open
questions 2, 3, 6, 8, 10.
*Exit:* owner approval recorded; the roadmap amendment is explicit; F-01's
closure status is settled (PASS/PARTIAL vs RESOLVED-by-later-work) and the
audit backlog reflects it.

**Phase 1 — Transport abstraction and TLS dependency groundwork (2–3 d).**
Introduce a stream abstraction over `TcpStream` in `transport.rs` so that plain
and TLS paths are interchangeable; add `rustls`/`tokio-rustls` behind a
non-default cargo feature; update `Cargo.lock`; run `cargo deny check` and
`cargo audit`; extend `deny.toml` only if required; regenerate SBOM/provenance
if the build does so.
*Exit:* `cargo test --workspace` green and unchanged; a new test asserts that
the default (feature-off) path is byte-identical in behaviour; `cargo deny` and
`cargo audit` clean with the new tree; the dependency addition is documented in
`docs/SECURITY_MODEL.md` §Supply chain.

**Phase 2 — Certificate generation and identity tooling (2–4 d).**
Offline tooling to generate a node key pair and a self-signed certificate with a
node-ID SAN (script using `openssl`, or a small `ramshield-cli` subcommand);
pin computation (SPKI SHA-256) and formatting; a `dev-dependency` on `rcgen` for
test fixtures; documentation of the offline procedure.
*Exit:* a reproducible command produces a key pair, a certificate, and a pin; a
test verifies the SAN contains the node ID and the pin matches the key; the
procedure is documented in a runbook.

**Phase 3 — Configuration schema and validation (2–3 d).**
Add `mesh_tls_enabled`, `mesh_tls_cert`, `mesh_tls_key`, `mesh_peer_modes`,
`mesh_peer_pins` to `MeshConfig` with `#[serde(default)]`; extend
`crates/ramshield-config/src/validate.rs::validate_mesh` (well-formedness,
length, parseability, per-peer mode validity, one-of constraints); extend the
`MeshConfig` `Debug` redaction and the "no key material in rendered config"
test; extend `env.rs` overrides; wire `src/engine/boot.rs` (`boot.rs:255-320`)
to load the local key/certificate and construct the peer identity map.
*Exit:* existing config files load unchanged and behave identically; a new
config test asserts a TLS-enabled config validates; a test asserts an invalid
pin length or mode is rejected with a clear message; the redaction test passes.

**Phase 4 — TLS transport with mTLS and verified-identity binding (5–8 d).**
The core work: handshake in `send_to_ack` (`transport.rs:1200`) and in the accept
path (`transport.rs:628`, `transport.rs:1247`); server-side client-certificate
verification; the verified-identity lookup; the claimed-vs-verified equality
check (§3.4); the **no-fallback-on-failure** rule; a separate handshake timeout;
session resumption enabled on both sides; `tls12` disabled.
*Exit:* new integration tests in `crates/ramshield-mesh/tests/` prove: correct
pair connects and exchanges a frame + ACK; a wrong pin is rejected; a certificate
signed by an untrusted anchor is rejected; an expired certificate is rejected; a
certificate whose SAN does not match the node ID is rejected; a **valid
certificate from node B claiming `node_id = C` is rejected** (this is the test
that proves the identity gap is closed — the case that HMAC cannot express);
the pre-TLS HMAC path is unaffected; handshakes are bounded by the handshake
timeout; resumption is observed on the second connection.

**Phase 5 — Performance validation (2–3 d).**
A benchmark in `crates/ramshield-mesh/benches/` measuring: frames/s sustained
per peer with TLS vs without; handshakes/s; resumed-handshake fraction; CPU per
frame; memory per connection; and the effect of connection reuse.
*Exit:* a recorded report comparing against the `AGENTS.md` 10K ops/sec/node
target and the rustls benchmark figures in §6.1, with an explicit verdict on
whether connection reuse is required before production enablement.

**Phase 6 — Mixed-mode operation, runbooks and operational readiness (3–5 d).**
Per-edge flip procedure; separate-port variant for zero-downtime migration;
expiry alerting and skew monitoring; pin-rotation runbook including the
emergency re-pin path; revocation procedure; troubleshooting guide (mapping
"pin mismatch" / "expired" / "untrusted" / "clock skew" to operator actions);
`docs/SECURITY_MODEL.md`, `docs/FEATURE_0.5_L7_MESH_UPSTREAM.md` and
`AGENTS.md` updates.
*Exit:* an operator can, without reading source, diagnose a failed edge; a
tabletop rotation exercise is completed successfully.

**Phase 7 — Production enablement (gated).**
Production mesh enablement is gated on the roadmap's §13 mesh verdict, which
today requires F-02…F-08 to have passed and the HMAC limitation to be accepted.
TLS work must not be used to shortcut any of those gates.
*Exit:* the mesh verdict is recorded on a single frozen SHA.

**Deferred (separate approval) — Phase 8: remove the HMAC envelope (C2).**
Only after C1 has run in production, and only with explicit approval to change
the key-distribution model. Exit: the envelope carries no MAC; `peer_auth_keys`
and `auth_key` are deprecated and then removed under a versioned config change;
rollback is no longer a configuration-only operation, so this phase is
irreversible in practice and needs its own change record.

**Total indicative sizing:** roughly 17–28 engineer-days for Phases 0–6,
excluding review cycles, production soak time, and the optional CA path (which
adds offline signing-host work) — and, critically, excluding the F-02…F-08
correctness work that should land first.

---

## 11. Sources

**Repository (read-only):**
`crates/ramshield-mesh/src/transport.rs`; `crates/ramshield-mesh/src/lib.rs`;
`crates/ramshield-mesh/src/condition_sync.rs`;
`crates/ramshield-mesh/Cargo.toml`; `crates/ramshield-mesh/tests/`;
`crates/ramshield-config/src/sections/security.rs`;
`crates/ramshield-config/src/validate.rs`; `crates/ramshield-config/src/env.rs`;
`crates/ramshield-config/src/lib.rs`; `src/engine/boot.rs`; `Cargo.toml`;
`Cargo.lock`; `deny.toml`; `Containerfile`; `rust-toolchain.toml`;
`docs/SECURITY_MODEL.md`; `docs/FEATURE_0.5_L7_MESH_UPSTREAM.md`;
`docs/development/AUDIT_FINDINGS_BACKLOG.md`;
`docs/development/audit-2026-10-20/F01_REPORT.md`;
`docs/development/MESH_WORKING_ROADMAP.md`; `AGENTS.md`; `BACKLOG.md`.
Roadmap: `ramshield-remediation-roadmap-v1.2.md` (owner-controlled document).

**External:**
* rustls crate documentation (0.23.41) — https://docs.rs/rustls/0.23.41/rustls/index.html
* rustls `ServerConfig` / `ClientConfig` / `client::danger` / `WebPkiClientVerifier` — https://docs.rs/rustls/latest/
* rustls crate features and dependencies (crates.io API, rustls 0.23.40) — https://crates.io/api/v1/crates/rustls/0.23.40
* rustls performance report, 2026-03-07 (rustls 0.23.37 vs OpenSSL 3.6.1 vs BoringSSL, Xeon E-2386G) — https://rustls.dev/perf/2026-03-07-report/
* AWS-LC-Rust FAQ (build requirements, FIPS tooling) — https://aws.github.io/aws-lc-rs/faq.html
* RFC 8446, *The Transport Layer Security (TLS) Protocol Version 1.3* (§2.3 0-RTT, §4.2.1) — https://www.rfc-editor.org/rfc/rfc8446
* RFC 8470, *Using Early Data in HTTP* (replay exposure of 0-RTT) — https://httpwg.org/specs/rfc8470.html
* ShopperCove, *OpenSSL TLS Handshake Lab: Full vs Resume on Localhost*, 2026-09-30 — https://www.shoppercove.com/blog/openssl-tls-handshake-full-vs-resume-localhost-lab
* wolfSSL, *TLS 1.3 Performance Analysis — Full Handshake*, 2019-04-15 — https://www.wolfssl.com/tls-1-3-performance-part-2-full-handshake-2/
* rustls crate metadata and licences (crates.io API): `tokio-rustls` 0.26.4,
  `rustls-webpki` 0.103.13, `ring` 0.17.14, `rcgen` 0.13.2,
  `rustls-pki-types` 1.14.1 — https://crates.io/api/v1/crates/

---

*Prepared as a design proposal for repository-owner review. No production code
was written. No repository file was created, modified or deleted.*
