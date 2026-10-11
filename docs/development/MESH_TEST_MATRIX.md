# Mesh Test Matrix

This matrix records executable acceptance evidence for the optional mesh feature. It does not replace the core production backlog.

## Stage A — transport boundary
- [ ] Real TCP authenticated block delivery and invalid-HMAC rejection (covered on the separate acceptance-test branch; must be included in the final integrated candidate).
- [x] Existing transport unit tests cover frame limits, read timeout, sync chunking, bind validation, send timeout, listener release, and invalid future timestamps.

## Stage B — CRDT correctness (release-blocking)
- [ ] Add/remove/re-add and stale-ban rejection across all delivery orders.
- [ ] Concurrent operations converge independent of merge order; duplicate and reordered messages are idempotent.
- [ ] Operation identity remains unique across HLC physical-time advances and process restarts.
- [ ] Expiry and unban state cannot resurrect after disconnection.
- [ ] Tombstone GC is based on causal stability/peer acknowledgement or epochs, not a wall-clock timer.

## Stage C — anti-entropy completeness (release-blocking)
- [x] Stable-state regression test visits all 5,000 active entries over repeated 1,024-item snapshots.
- [x] More than 4,096 mixed active entries and tombstones: repeated snapshots visit all 2,500 of each kind in stable-state regression coverage.
- [ ] Pagination progress under concurrent mutation is fair and deterministic.
- [ ] Multi-chunk state converges after packet loss, duplication, reordering, and reconnect.
- [ ] Sync repair remains effective after incoming queue overflow.

## Stage D — protocol and trust boundary
- [ ] Replay of a previously valid envelope; duplicate semantics are explicit.
- [ ] Unknown node IDs, self-origin, invalid tier/expiry, malformed frames, and timestamp boundaries.
- [ ] Node identity binding, shared-key impersonation assumptions, and key rotation are tested/documented.
- [ ] Reader/peer caps and malformed-frame floods remain bounded.

## Stage E — overload, lifecycle, and service projection
- [ ] Queue saturation reports drops and then demonstrably repairs state.
- [ ] Slow/unreachable peers, outbound saturation, permit release, and cancellation.
- [ ] Shutdown while idle, reading a partial frame, broadcasting, and syncing; all owned tasks terminate.
- [ ] Service-level block/unblock, TTL, WAL/storage failure, XDP failure, and reconciliation.
- [ ] Feature-off builds do not start mesh; mesh startup failures are surfaced.

## Stage F — performance and operations
- [ ] Reproducible 3+ node tests with loss/reconnect, large state, and clock skew.
- [ ] Throughput, convergence latency, memory at configured limits, and slow-peer behavior have baselines.
- [ ] Threat model, key provisioning/rotation, network exposure, rollback, and known limitations reviewed.

## Acceptance rule

Mesh remains optional and production-disabled until all release-blocking items are closed or explicitly dispositioned by independent review, and the feature-off/mesh/full/all-feature test matrix plus formatting, strict Clippy, and workspace checks pass on one exact SHA. A green unit-test run alone is not acceptance.
