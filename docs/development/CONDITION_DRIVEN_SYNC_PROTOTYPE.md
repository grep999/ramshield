# Condition-driven mesh sync prototype

**Status:** integrated WAL-backed prototype; receiver-confirmed recovery completion, multi-process fault-injection acceptance, and independent review remain open. Not production-ready.

**Status discipline:** this document describes the implementation, not the live branch status. Before each run, fetch the current PR #19 HEAD and verify checks on that exact SHA; missing workflow runs/checks are **unverified**, not green. See [MESH_RECOVERY_ACCEPTANCE_MATRIX.md](MESH_RECOVERY_ACCEPTANCE_MATRIX.md) for the durable contract, acceptance matrix, ownership, and handoff process.

**Branch:** prototype/condition-driven-mesh-sync-20261011

## Goal

Use the existing WAL as durable mutation history and the CRDT as the convergence model. Condition-driven anti-entropy reduces unnecessary full-state work without introducing blockchain machinery, a global sequence, or changing CRDT merge semantics. A bounded periodic repair remains as a safety net.

## Scheduling conditions

| Condition | Scheduling behavior |
|---|---|
| Local mutation | Coalesce nearby changes for 50 ms, then schedule repair |
| Remote state applied | Mark state dirty and coalesce |
| Peer reconnect | Request immediate repair |
| Delivery failure | Retain dirty state and retry with bounded exponential backoff |
| Incoming queue overflow | Request repair because a delta may have been dropped |
| Periodic repair deadline | Run a safety-net sync even if no trigger was observed |

Prototype defaults are 50 ms coalescing, a 30 s repair interval, 250 ms initial retry, and 10 s maximum retry delay. The transport scheduler wakes every 25 ms with delayed missed-tick behavior; tests use short deterministic durations. Scheduling uses monotonic elapsed time, not wall-clock time.

## Durable mutation and replication contract

1. A local mesh mutation gets a durable WAL record with schema version, origin node/incarnation/counter, IP, action, creation/expiry, and tier.
2. WAL append and sync precede publication of the mutation to the CRDT.
3. Each peer has an independent acknowledged cursor over this node's WAL LSNs. These are sender-local offsets, not a cluster-global order; they are unrelated to any CRDT-state traversal.
4. WAL batches and snapshot transport frames are bounded. The receiver validates the authenticated envelope and mutation fields, persists/merges accepted mutations idempotently, and returns an authenticated application ACK for each frame.
5. The sender advances a WAL cursor only when the ACK's LSN exactly matches the batch end. A socket write alone is not acceptance.
6. Projection failure does not erase committed history; a bounded repair queue retries Store projection. XDP reconciliation remains a separate projection concern.
7. Startup restores checkpointed CRDT provenance/tombstones and replays the WAL tail. If required history was pruned and no provenance-bearing checkpoint exists, hard-WAL mode fails closed.
8. If a peer cursor falls behind WAL retention, the sender holds that cursor pending a fixed full CRDT snapshot. The recovery boundary and snapshot are captured once per attempt; repeated ticks cannot restart the attempt or move the boundary.

**Current recovery behavior is deliberately fail-closed:** frame ACKs may set a local transfer marker, but they are not whole-recovery completion. Enforcement gates WAL replay while `recovery_completion_pending(peer)` remains true; the local marker does not prove receiver-wide completion and must not release the cursor. The required receiver-confirmed handshake bound to generation + captured source-WAL boundary + canonical snapshot digest is still missing. Until it is implemented and verified, recovery remains pending rather than resuming unsafely.

## State-machine contract

Each configured peer owns one ConditionSyncPlanner.

1. The caller reports a condition with a monotonic timestamp.
2. take_due(now) claims at most one pass; overlapping passes are prevented.
3. The caller performs bounded snapshot/chunk/send work for that peer.
4. The caller invokes complete(now, success) once for each claimed pass.
5. Failure leaves the peer dirty and applies capped exponential backoff.
6. A condition arriving during an in-flight pass remains pending; completion of an older pass cannot erase it.
7. Periodic repair remains mandatory because notifications can be missed and ACKs cover individual frames, not an atomic snapshot transaction.

## Current integration

- Each configured peer has its own planner, delivery-failure flag, recovery generation, fixed snapshot (when recovering), and recovery boundary.
- Local broadcasts mark peers dirty; send failures request repair; success after a recorded failure triggers reconnect repair.
- Inbound queue overflow is surfaced to the scheduler after releasing the queue lock.
- Enforcement uses `drain_with_ack()` and acknowledges only after processing; WAL batches return the sender-local batch-end LSN only after all contained mutations are accepted.
- Full-state recovery exports one fixed, deterministically ordered snapshot under an exclusive CRDT snapshot barrier and splits it into 16-entry wire frames; every frame requires an authenticated application ACK. This avoids a moving cursor over a live map, but the snapshot currently consumes memory proportional to the full CRDT state. WAL batches are separately bounded and active sends use a shared writer semaphore and per-frame deadlines.
- Live-map snapshot pagination (`snapshot_page(cursor, limit)`) and rotating snapshot traversal are explicitly rejected and have been removed. Do not reintroduce or rename that design; fixed recovery snapshots plus the WAL tail are the accepted approach.
- CRDT state includes origin dots and tombstone high-water marks so concurrent origin mutations are not collapsed into one peer's aggregate event.

## Acceptance work still required

- Add and test an explicit receiver-confirmed recovery completion frame binding the recovery generation, captured WAL boundary, and a deterministic digest of the completed state rotation.
- Run real multi-node fault injection: disconnect/reconnect, process restart across checkpoint/retention, duplicate/reordered/lost frames and ACKs, queue pressure, concurrent mutations during fixed-snapshot capture/transfer, and eventual CRDT plus Store/XDP convergence.
- Prove eventual convergence of both CRDT state and Store/XDP projections, including tombstones and more than 4,096 live entries/tombstones.
- Test cancellation/shutdown during stalled sends, peer fairness, bounded task/memory behavior, and no hot retry loop for unreachable peers.
- Add origin signatures or per-node authenticated identity. Current shared-key HMAC authenticates a configured mesh member, not an individual origin dot.
- Keep independent review and exact-head CI green. Passing unit/workspace CI does not replace network-level convergence acceptance.

## Minimal disposable sandbox

From a checkout of this branch, run:

```bash
bash scripts/test-mesh-recovery-sandbox.sh
```

The script archives the current committed HEAD into a fresh temporary directory and runs three focused Rust gates: the captured recovery-boundary regression, real loopback TCP mesh acceptance tests, and enforcement tests with the mesh feature enabled. It requires Git, tar, Cargo/Rust, and Cargo.lock; it does not need Docker or production configuration. Use `--keep` to preserve the temporary tree for inspection. The sandbox validates these targeted integration paths; it is not a substitute for multi-process fault injection or the explicit recovery-completion handshake still listed above.

## Verified regression coverage

The integrated branch contains tests for bounded WAL-batch LSN behavior, checkpoint/tombstone roundtrip, checkpoint-seed plus WAL-tail folding, positive/negative application ACKs, enforcement-level WAL-batch TCP handling, and a recovery-boundary regression that proves repeated recovery requests do not move the captured boundary. These are existing test descriptions, not a claim that they have run successfully on the current HEAD.

## Acceptance bar

Do not merge or enable production mesh until all of the following are demonstrated on one immutable SHA:

- convergence with dropped, duplicated, delayed, and reordered messages;
- every entry and tombstone in the fixed recovery snapshot and subsequent WAL tail is applied for each peer independently;
- recovery does not advance past the captured boundary without the required completion proof;
- bounded recovery latency after reconnect/overflow/failure and no hot retry loop;
- no lost dirty signal when mutation races with sync completion;
- feature-off and mesh-enabled workspace gates, strict Clippy, formatting, dependency audit, and independent review are green.

Until those gates pass, keep mesh optional and disabled in production.


## Current implementation delta — 2026-10-12

The live-map keyset cursor and rotating snapshot APIs have been removed from the PR branch. Anti-entropy now freezes CRDT mutations during export, captures one immutable sorted `AworsetSnapshot` per repair attempt, and splits it into bounded `Sync` frames; each frame is sent through the authenticated application-ACK path. The retention-gap boundary is retained across retries. This is an implementation step, not final recovery acceptance: the receiver-confirmed generation/boundary/digest completion handshake and multi-process fault-injection tests remain to be implemented. Full snapshot export currently duplicates CRDT state in memory and must be measured under the target workload.
