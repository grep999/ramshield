# Mesh Test Matrix

**Purpose:** make mesh feature acceptance evidence explicit. This matrix supplements, rather than replaces, the core production backlog.

**Current candidate baseline:** `audit-branch` at `967f388513c4feff71db225207efff608c4f47d9`.

## Stage A — executable transport tests

- [x] Real TCP happy path: a configured sender broadcasts an authenticated block to a receiver and the receiver queues it.
- [x] Invalid-MAC frame is rejected before it enters the receive queue.
- [x] Existing unit coverage: frame cap and delimiter accounting, incomplete-frame timeout, sync chunk size, bind-argument validation, send timeout, listener socket release, invalid future creation time, and oversized sync rejection.

## Stage B — CRDT correctness (release-blocking)

- [x] Add/remove/re-add for the same IP and node (same-process operation sequence).
- [x] Concurrent bans from different nodes; merge order independence.
- [x] Duplicate and reordered deltas are idempotent.
- [x] Old ban after unban is rejected; a genuinely newer ban after unban is accepted (same process).
- [x] HLC physical-time advance cannot reuse a dot counter (64-bit process-local operation counter).
- [x] Restart/incarnation changes cannot collide with old dots in the same configured node ID: each blocklist instance gets a random UUID incarnation, and nil/missing incarnations are rejected.
- [x] Regression: explicit-unban high-water mark survives the former GC horizon and rejects a replay.
- [x] Regression: an expired active entry is converted to a high-water mark before pruning.
- [ ] Expiry and purge cannot resurrect stale state after a peer is offline in every delivery ordering; requires multi-peer/offline convergence tests.
- [ ] Tombstone GC has a causal-stability/epoch/repair argument, not only a timer. **Current safe fallback:** timer-only deletion is removed; high-water marks are retained indefinitely, so bounded-memory GC is still blocked.
- [ ] Property-style merge permutations converge to the same state.

## Stage C — anti-entropy completeness (release-blocking)

- [x] More than 4,096 active entries: repeated snapshots visit all 5,000 entries in stable-state regression coverage.
- [x] More than 4,096 mixed active entries and tombstones: repeated snapshots visit all 2,500 of each kind in stable-state regression coverage.
- [ ] Pagination progress is fair and deterministic, even while the map changes.
- [ ] Multi-chunk state converges after loss, duplicate delivery, reordering, and reconnect.
- [ ] Stale tombstones and later legitimate re-bans converge correctly.
- [ ] Sync repair remains effective after queue overflow.

## Stage D — protocol and trust boundary

- [ ] Invalid HMAC, truncated/invalid HMAC encoding, modified body, malformed JSON.
- [ ] Replay of a previously valid envelope; duplicate semantics are explicit.
- [x] Nil/missing incarnation is rejected for block, unblock, and sync deltas.
- [ ] Self-origin, unknown node ID, counter extremes, invalid tier, invalid expiry, and timestamp boundaries.
- [x] Unknown/unconfigured node IDs and mismatched peer keys are rejected by transport tests.
- [ ] Asymmetric per-origin signatures prevent a compromised configured peer from forging another node's CRDT dots; per-peer HMAC keys alone do not provide this guarantee.
- [ ] Key rotation is exercised across a coordinated rollout.
- [ ] Frame exactly at limit, one byte over, delimiter split across reads, clean EOF, partial EOF, CRLF, and multiple frames on one connection.
- [ ] Reader/peer caps and malformed-frame floods remain bounded.

## Stage E — overload, lifecycle, and service projection

- [ ] Incoming queue saturation reports drops and then demonstrably repairs state.
- [ ] Outbound semaphore saturation, stalled peers, permit release, and unreachable peers.
- [ ] Shutdown while idle, blocked on a partial frame, broadcasting, and processing sync.
- [ ] All mesh-owned tasks terminate; listener and sockets release; startup errors propagate.
- [ ] Feature-off build and tests do not start mesh; mesh-feature startup failures are surfaced.
- [ ] Service-level block/unblock, operator suppression, TTL, expiry, WAL/storage failure, XDP failure, and retry/reconciliation.
- [ ] IP and CIDR semantics are explicit; current wire deltas are IP-only.

## Stage F — performance and operations

- [ ] Reproducible multi-node tests with 3+ peers, loss/reconnect, large state, and skew.
- [ ] Throughput, convergence latency, memory at configured limits, and slow-peer behavior have baselines.
- [ ] Threat model, key provisioning/rotation, network exposure, operational runbook, rollback, and known limitations reviewed.

## Acceptance rule

The feature stays optional and disabled in production until Stages B–F are closed or explicitly dispositioned by an independent reviewer, and the full feature-off/mesh/full/all-feature test matrix, formatting, strict Clippy, and relevant workspace checks pass on one exact SHA. A green unit-test run alone is not acceptance.
