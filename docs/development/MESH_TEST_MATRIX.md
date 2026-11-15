# Mesh Test and Security Acceptance Matrix

**Baseline reviewed:** `master` at `2c225d572a26f6f040b2dd8d5b987e0595e0fa87`.
**Purpose:** distinguish behavior present in source from acceptance evidence still required. A checked item means the described narrow regression coverage exists; it does not imply production readiness.

## Current authentication boundary — release-blocking

The current transport uses a per-node HMAC signing key, not pairwise edge secrets or asymmetric node signatures:

- `MeshHandle::bind` builds `trusted_keys` from each configured peer's `MeshPeer.auth_key`, keyed by peer node ID.
- `send_to_ack` signs outbound envelopes with the local node's `self.auth_key`.
- `read_stream` selects the verification key using the envelope's claimed `node_id`; it does not compare the TCP remote IP with the configured peer address.
- The configuration therefore distributes each node's signing key to its peers as a verification secret. A configured node that possesses another node's verification key can forge an envelope claiming that other node's ID to a receiver that trusts that key. The same trust issue applies to HMAC-authenticated ACKs. Unique key values in one node's config do **not** prevent this cross-peer impersonation when those keys are shared with verifiers.
- There is no source-IP binding and no F-01 pairwise edge-secret model in this baseline. Source-IP binding alone would be defense-in-depth, not a replacement for non-repudiable node identity; F-01's shared edge secret also cannot distinguish the two endpoints that possess the same edge secret.

**Do not describe the present design as cryptographically isolating peer identities.** Treat cross-peer impersonation by a compromised configured node as an open, release-blocking trust-boundary risk until the key model is redesigned and independently reviewed.

F-01 is not included in PR #97. Its review identified IPv4-mapped-IPv6 address normalization as a source-binding compatibility issue; the F-01 branch later reported a `same_ip()` normalization fix, but that implementation and its tests are not present in this PR. Any future source-IP binding must include IPv4, IPv6, dual-stack and mapped-address positive/negative tests. F-01's review also left a P2 weakness: negative tests need their own positive controls.

## Stage A — executable transport tests

- [x] Real TCP happy path: configured sender sends an authenticated block and receiver queues it.
- [x] Invalid-MAC frame is rejected before it enters the receive queue.
- [x] Narrow unit coverage exists for frame bounds/delimiter accounting, incomplete-frame timeout, sync chunk size, bind validation, send timeout, listener socket release, invalid future creation time, and oversized sync rejection.
- [ ] Positive control paired with every negative authentication/source rejection test.
- [ ] Cross-peer impersonation test proving the current per-node-key weakness, followed by regression tests for the replacement identity model.
- [ ] Source-address binding, if adopted, with ephemeral source-port handling and IPv4-mapped-IPv6 normalization tests.

## Stage B — CRDT correctness (release-blocking)

- [x] Same-process add/remove/re-add, concurrent-origin merge order, duplicate/reordered delta idempotency, and stale-ban rejection regressions.
- [x] Incarnation identity and missing/nil-incarnation rejection regressions.
- [x] High-water-mark regressions for explicit unban and expiry conversion.
- [ ] Offline-peer expiry/purge convergence across all delivery orderings.
- [ ] Causal-stability/epoch/repair argument for tombstone GC. Current safe fallback retains high-water marks indefinitely; bounded-memory GC remains unresolved.
- [ ] Property-style merge permutations and multi-process convergence.

## Stage C — anti-entropy and recovery completeness (release-blocking)

- [x] Stable-state snapshot pagination regressions above 4,096 entries (5,000 active; 2,500 active plus 2,500 tombstones).
- [x] Current transport implements fixed recovery snapshots, generation/recovery-ID/boundary/digest-bound frames, bounded frame assembly, receiver-side digest verification, and a receiver proof checked by the sender.
- [ ] Deterministic multi-process fault injection for loss, duplication, reordering, disconnect/reconnect, and concurrent mutation.
- [ ] Process restart behavior: recovery assembly and peer sync state are currently in-memory; prove restart recovery and cursor reconstruction from durable state.
- [ ] Fairness/progress under concurrent state mutation and multi-chunk repair after queue overflow.
- [ ] Stale tombstones and legitimate re-bans converge after offline periods.

## Stage D — protocol and trust boundary

- [ ] Replace or formally disposition the shared per-node HMAC trust model; prevent one configured peer from forging another peer's identity or ACK.
- [ ] Key provisioning, rotation, revocation, compromise recovery, and cluster membership-change tests.
- [ ] Replay semantics and envelope freshness boundaries under clock skew and restart.
- [ ] Self-origin, unknown node ID, counter extremes, invalid tier/expiry and timestamp boundaries.
- [ ] Frame exactly at limit, one byte over, delimiter split across reads, clean/partial EOF, CRLF and multiple frames per connection.
- [ ] Reader/peer caps, malformed-frame floods and queue saturation remain bounded.

## Stage E — overload, lifecycle and enforcement projection

- [ ] Queue saturation reports drops and demonstrates subsequent state repair.
- [ ] Outbound semaphore saturation, stalled peers, permit release and unreachable peers.
- [ ] Shutdown while idle, blocked on partial frame, broadcasting and processing recovery.
- [ ] All mesh-owned tasks terminate; listener/sockets release; startup failures propagate.
- [ ] Feature-off build proves mesh is not started; feature-on startup failures are surfaced.
- [ ] Store/WAL/XDP projection convergence across block, unblock, suppression, TTL, expiry, persistence failure and retry.
- [ ] Explicit IP/CIDR semantics; current wire deltas are IP-only.

## Stage F — performance and operations

- [ ] Reproducible 3+ node tests with loss/reconnect, large state and clock skew.
- [ ] Throughput, convergence latency, peak memory (including snapshot cloning), and slow-peer baselines.
- [ ] Threat model, key provisioning/rotation, network exposure, operational runbook and rollback reviewed.

## Acceptance rule

Mesh remains optional and production-disabled until release-blocking items are closed or explicitly dispositioned by an independent reviewer. Require feature-off, mesh, full and all-feature tests; formatting; strict Clippy; dependency audit; lifecycle/fault evidence; and security review on one exact SHA. A green unit-test run or a workflow qualification receipt alone is not production onboarding approval.
