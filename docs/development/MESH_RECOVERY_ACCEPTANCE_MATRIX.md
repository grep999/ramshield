# Mesh Recovery Implementation and Acceptance Ledger

**Code baseline reviewed:** `master` at `2c225d572a26f6f040b2dd8d5b987e0595e0fa87`.
**Document change:** consolidated in PR #97. This ledger distinguishes implemented source behavior from operational properties that still lack acceptance evidence.
**Safety invariant:** mesh remains optional and production-disabled. This workflow/documentation PR is not production onboarding approval.

## 1. Current recovery implementation (source-verified)

The implementation is in `crates/ramshield-mesh/src/transport.rs` and `crates/ramshield-enforcement/src/service/mesh.rs`.

1. **Snapshot capture:** `capture_mesh_recovery_snapshot` holds the checkpoint barrier while reading the durable WAL LSN and exporting the CRDT snapshot.
2. **Immutable recovery identity:** `request_full_sync` canonicalizes and validates the snapshot, increments a per-peer generation, creates a UUID recovery ID, and stores the snapshot behind an `Arc` with its WAL boundary.
3. **Canonical representation:** `canonical_snapshot` sorts block and unblock deltas by IP and dot identity. `canonical_snapshot_digest` hashes the canonical JSON using SHA-256 with a domain separator and encoded-length prefix.
4. **Bounded transfer:** recovery frames carry generation, recovery ID, WAL boundary, snapshot digest, frame index/count and bounded block/unblock payloads. Frame/entry limits are enforced.
5. **Receiver assembly:** `record_recovery_frame` keys sessions by authenticated sender node ID; it checks recovery identity, boundary, digest, frame count, index and aggregate limits. An exact duplicate frame is idempotent; a conflicting duplicate is rejected.
6. **Receiver completion:** `complete_recovery` requires every frame, reconstructs and canonicalizes the snapshot, recomputes the digest, and returns a proof bound to generation, recovery ID, boundary, digest and frame count. An exact repeat returns the stored proof.
7. **Application ACK boundary:** `ReceivedMeshMessage::acknowledge_with_lsn` only records recovery frames after the caller reports acceptance and the CRDT no longer needs the deltas. The enforcement service applies incoming mutations through its WAL/CRDT/enforcement path before acknowledging; the API documentation makes persistence-before-ACK an explicit caller contract.
8. **Sender verification:** the sender accepts only a receiver proof exactly equal to its expected generation/recovery-ID/boundary/digest/frame-count tuple. Missing or mismatched proof leaves recovery pending and the cursor pinned.
9. **WAL continuation:** after the matching completion is recorded, the enforcement service advances the peer cursor to at least the recovery boundary, acknowledges that completed recovery in the handle, and resumes WAL replication from the next cursor.

These are source-level implementation facts, not evidence of multi-process reliability, durable recovery-session state, or production operational readiness.

## 2. Authentication boundary relevant to recovery

The recovery frames and completion proofs are carried inside the existing HMAC envelope. The current baseline uses per-node signing keys distributed to configured peers as verification keys. It has no source-IP binding and no pairwise edge-secret model. Because peers possess other nodes' verification secrets, a compromised configured peer may be able to forge another peer's claimed node ID and corresponding ACK. See `MESH_TEST_MATRIX.md` for the security acceptance gate.

PR #97 does not transplant F-01 from PR #85. F-01's review identified IPv4-mapped-IPv6 source-address normalization and negative-test positive-control issues; its later branch report says the address normalization was fixed there, but that code is not in this PR. F-01's shared edge-secret design also cannot distinguish the two endpoints sharing one edge secret.

## 3. Recovery acceptance matrix

| Scenario | Source behavior / evidence | Remaining acceptance |
|---|---|---|
| Fixed snapshot and boundary | Snapshot captured under checkpoint barrier with durable WAL LSN | [ ] Multi-process concurrent-mutation test proves boundary/tail relationship |
| Canonical digest | Stable sort, validated snapshot, domain-separated SHA-256 over canonical JSON | [ ] Cross-process canonicalization vectors and version-compatibility tests |
| Bounded frame transfer | Frame, entry, and total-session bounds exist | [ ] Resource/peak-memory qualification at configured maxima |
| Duplicate/reordered frames | Indexed assembly; exact duplicate payload accepted; conflicting duplicate rejected | [ ] Deterministic network fault injection for loss, duplication, reordering and delay |
| Completion proof | Requires all frames and recomputed matching digest; proof binds recovery identity | [ ] Lost completion response/retry/replay tests across real processes |
| Sender cursor pin | Missing/mismatched proof keeps recovery pending; matching proof is required before cursor advance | [ ] WAL retention-gap and cursor-reconstruction tests under restart |
| WAL durability before ACK | Enforcement service uses WAL append/sync and CRDT application before ACKable result | [ ] Crash injection between append, sync, CRDT merge, projection and ACK |
| Receiver restart mid-recovery | Receiver assembly is held in an in-memory map; it is not durable across process restart | [ ] Prove restart triggers safe fresh recovery and cannot falsely complete or skip state |
| Sender restart mid-recovery | Per-peer sync/recovery state is held in the in-memory mesh handle | [ ] Prove generation/session renewal and durable WAL replay/cursor reconstruction |
| Tombstone acknowledgments | Completion carries ordered tombstone ACK epochs; peer acknowledgment is recorded after matching proof | [ ] Offline-peer causal stability, GC safety and multi-peer convergence |
| Store/XDP projection | Enforcement service applies accepted mutations through persistence/enforcement path | [ ] Two-process convergence digest for CRDT, Store and effective XDP projection |
| Authentication of recovery messages | HMAC envelope and ACK authentication are implemented | [ ] Resolve cross-peer impersonation risk in the shared per-node key distribution model |

## 4. Required fault-injection harness

Before production onboarding, a deterministic multi-process harness must cover:

- Frame loss, duplicate, reorder, delay, malformed and oversized payloads.
- Lost completion response, duplicate completion, stale generation, wrong boundary and wrong digest.
- Disconnect/reconnect and process restart on either side at every recovery phase.
- Concurrent mutation after snapshot capture and exact WAL-tail replay from boundary + 1.
- WAL append/sync failures and crashes before/after ACK.
- Offline peers, tombstone retention, legitimate re-ban and convergence across at least three nodes.
- Queue saturation, slow/unreachable peers, bounded memory and clean shutdown.
- Final convergence digests for CRDT blocks/tombstones and effective Store/XDP state.

A single-process unit test, successful compile, or workflow qualification receipt is not a substitute.

## 5. Ownership and review protocol

1. **Protocol implementation:** changes to transport, key distribution, or enforcement semantics require a dedicated, narrowly scoped implementation change with a complete config/boot/test/operator-documentation contract.
2. **Test implementation:** the fault-injection harness must be independently runnable and must not rely on unverified internal API assumptions.
3. **Coordinator:** freezes one exact candidate SHA and records changed paths, commands, exit codes, workflow URLs and remaining risks.
4. **Independent reviewers:** security, crash/concurrency/fault behavior, and exact-SHA CI/diff review are separate passes. Reviewers are read-only and must inspect the same frozen SHA.
5. **Gate:** missing, queued, skipped, cancelled, stale-SHA or inaccessible evidence is not PASS. No merge or production enablement while any release-blocking gate remains open.

## 6. Out-of-scope for PR #97

- Replace the shared per-node HMAC key distribution with a design that prevents one configured node from impersonating another; evaluate asymmetric signatures or an equivalently sound identity model.
- Decide whether source-IP binding is an additional control and implement IPv4/IPv6/mapped-address handling with positive and negative controls.
- Do not adopt F-01's pairwise edge-secret model without coordinated config validation, boot-time decoding, peer provisioning, transport, tests and operator documentation; explicitly account for the within-edge attribution limit.
- Persist or safely reconstruct recovery session and replication cursor state across process restart.
- Complete the deterministic multi-process fault-injection and Store/XDP projection convergence suite.
- Obtain independent security, fault/concurrency and exact-SHA CI review before production onboarding.

## 7. Change record

PR #97 changes workflow routing and these acceptance documents only. It does not change transport code, the wire protocol, key provisioning, runtime defaults, feature flags, deployment behavior or production mesh enablement.
