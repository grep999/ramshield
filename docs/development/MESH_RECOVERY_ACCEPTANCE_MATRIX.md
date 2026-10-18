# Mesh Recovery Acceptance Matrix

**Status:** active work ledger for PR #19; acceptance is not yet complete.
**Target branch:** `prototype/condition-driven-mesh-sync-20261011`
**Baseline observed before this document commit:** `9f24516c61f40df79d25536988b67251dfcba933`
**Safety:** keep PR #19 Draft; keep production mesh disabled. No merge, deploy, force-push, or rebase.

This document is the durable handoff contract for the recovery-protocol coder, test coder, coordinator, and independent reviewer. It records required behavior; it does not claim that unimplemented behavior or unrun tests have passed.

## Fixed design decisions

- WAL is durable mutation history. Append and required sync precede publishing a local mutation.
- CRDT state, including origin dots and tombstones, is the convergence model.
- WAL LSNs are local to each sender/source WAL. They are not a global sequence.
- While retained history is available, catch up from the peer's acknowledged source-WAL cursor.
- On a retention gap, capture one immutable deterministic snapshot and its source-WAL boundary under a new recovery generation. Send that fixed snapshot in bounded frames.
- **Live-map cursor/keyset pagination is dropped. Do not reintroduce it.**
- Individual frame ACKs prove only acceptance of those frames. They do not prove whole-recovery completion.
- Keep the peer WAL cursor pinned and WAL replay gated until a receiver-confirmed completion proof for the same generation, captured boundary, and canonical snapshot digest has been accepted. Then resume at boundary + 1.
- Keep the existing periodic repair safety net. No blockchain or global ordering layer is needed.

## Protocol contract to implement

1. Each recovery attempt has an unambiguous generation/session identity, source-WAL boundary, canonical snapshot digest, and fixed frame count.
2. Every frame carries enough authenticated context to reject a frame belonging to another generation/boundary/digest. Frame identity/order and duplicate semantics must be explicit.
3. The receiver records successful frame application, not merely socket receipt. It may issue whole-recovery completion only after it has accepted the complete frame set for one consistent recovery identity and independently verifies the canonical digest of the resulting snapshot.
4. Canonical digest input must be specified and deterministic: stable ordering, normalized fields, and both live entries and tombstones. Never hash map iteration order.
5. Completion must be authenticated and idempotent. Exact duplicate completion is safe; conflicting duplicate, stale generation, wrong boundary, wrong digest, incomplete frame set, malformed proof, and replay from an obsolete session must be rejected.
6. Sender accepts only a completion proof matching the active generation, boundary, and digest. Frame ACKs, TCP writes, and local transfer markers cannot release the cursor.
7. On timeout, disconnect, partial transfer, invalid proof, or restart with missing receiver recovery state, fail closed: preserve the pending recovery and cursor pin; safely retry/restart the fixed recovery transaction rather than skipping history.
8. Mutations after snapshot capture belong to the WAL tail. After completion, replay from boundary + 1 and prove those mutations are not lost.
9. Document the actual wire/API contract in the protocol coder's handoff before the test coder binds tests to field names. Do not invent API names to make a test compile.

## Acceptance matrix

| Case | Required observable result |
|---|---|
| All frames ACKed, no completion proof | Recovery remains pending; WAL cursor stays pinned |
| Complete valid frame set and matching proof | Recovery clears only for matching generation/boundary/digest; replay resumes at boundary + 1 |
| Missing, duplicate, reordered, or delayed frame | No premature completion; duplicates are idempotent or explicitly rejected |
| Incomplete frame set or digest mismatch | Completion rejected; cursor remains pinned |
| Stale generation, wrong boundary, wrong digest, conflicting proof | Rejected; current recovery state is not overwritten |
| Lost completion response / duplicate completion request | Safe retry; no duplicate side effect or cursor jump |
| Empty snapshot | Valid deterministic digest and completion path; no special-case cursor skip |
| Mutation after capture | Fixed snapshot remains unchanged; WAL tail applies after boundary |
| Disconnect/reconnect or process restart mid-transfer | Partial state cannot masquerade as complete; retry/recovery converges |
| Tombstones, unblock/re-ban, concurrent origins | No resurrection or deletion of a concurrent origin's live dot |
| Multi-frame and large state | Bounded frames; eventual CRDT and Store/XDP projection convergence |
| Queue pressure, slow/unreachable peer, shutdown | Bounded resources; retry/repair visible; no hot loop or leaked task |

At minimum, a deterministic two-process fault-injection harness must exercise frame loss/duplication/reordering, completion loss/replay, reconnect/restart, concurrent mutation, and convergence digests for CRDT blocks + tombstones and the effective Store/XDP projection. A single-node unit test or green compile is not a substitute.

## File ownership and handoff sequence

1. **Protocol coder** owns only `crates/ramshield-mesh/src/transport.rs` and `crates/ramshield-enforcement/src/service/mesh.rs`.
2. **Test coder** owns `crates/ramshield-mesh/tests/mesh_acceptance.rs`, `scripts/test-mesh-recovery-sandbox.sh`, and test-only support. Do not overlap production protocol files.
3. **Coordinator** tracks exact SHA, handoffs, and gates; does not patch either coder's files while work is in flight.
4. **Independent reviewer** is read-only and reviews the exact same candidate SHA named by both coder handoffs. A SHA mismatch or missing handoff means BLOCKED, not approval.
5. Each handoff is a durable PR #19 comment containing role, exact old/new SHA, changed files, contract/invariants, commands actually run and outcomes, known gaps, and one next owner/action.
6. Commit bounded, reviewable changes with truthful messages. Before each write, fetch current branch head and target blob SHA. Do not overwrite concurrent changes.
7. Run the focused sandbox and relevant Rust tests where Cargo is available; record actual exit status. Then verify CI, Review Pipeline, and Backlog Gate on the exact candidate SHA. Missing, queued, skipped, cancelled, inaccessible, or stale-SHA results are not PASS.
8. Reviewer reports `APPROVE`, `REQUEST CHANGES`, or `BLOCKED` with file/line evidence and exact-SHA check evidence. Do not self-approve.

## Current known state at baseline

- The prototype already uses a fixed deterministic snapshot protected during capture, bounded frames with application ACKs, and deliberately pins the WAL cursor while recovery completion is pending.
- The explicit receiver-confirmed generation/boundary/digest completion handshake remains the top protocol blocker.
- Multi-process fault injection, memory qualification for full-snapshot cloning, exact-SHA required gates, and independent review remain open.
- The environment used for the previous coordination pass could not run Cargo locally. No Rust test result is claimed here.
- This matrix is a committed execution contract, not evidence that any acceptance case passed.

## Per-commit record template

Append a short PR #19 handoff for every meaningful change with:

- **Role / result:** implementation, tests, docs, or independent review.
- **Base SHA → resulting SHA:** full immutable hashes.
- **Changed paths:** only paths actually changed.
- **Checks:** exact commands and observed exit statuses; link exact-SHA workflow runs/jobs.
- **Acceptance rows covered:** refer to the matrix above.
- **Open risks:** what remains unproved.
- **Next owner/action:** one bounded task.
