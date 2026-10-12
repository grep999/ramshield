# Condition-driven mesh sync prototype

**Status:** integrated WAL-backed prototype; exact-head CI is green, but multi-node fault-injection acceptance and independent review remain open. Not production-ready.

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
3. Each peer has an independent cursor over this node's WAL LSNs. These are sender-local offsets, not a cluster-global order.
4. Replication pages are bounded. The receiver validates the authenticated envelope and mutation fields, persists/merges accepted mutations idempotently, and returns an authenticated application ACK.
5. The sender advances a WAL cursor only when the ACK's LSN exactly matches the page end. A socket write alone is not acceptance.
6. Projection failure does not erase committed history; a bounded repair queue retries Store projection. XDP reconciliation remains a separate projection concern.
7. Startup restores checkpointed CRDT provenance/tombstones and replays the WAL tail. If required history was pruned and no provenance-bearing checkpoint exists, hard-WAL mode fails closed.
8. If a peer cursor falls behind WAL retention, the sender holds that cursor pending a full CRDT rotation. The recovery boundary is captured once per attempt; repeated ticks cannot restart the attempt or move the boundary.

The transport's full-sync rotation currently considers completion after every frame/page in the current generation has received an accepted application ACK. Enforcement then consumes a local completion marker and resumes WAL replay after the captured boundary. **This is not yet a separate receiver-level RecoveryComplete handshake that echoes the generation, boundary, and a digest of the snapshot.** Treat that as remaining protocol-hardening work; do not describe the current marker as a receiver-confirmed snapshot transaction.

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

- Each configured peer has its own planner, delivery-failure flag, snapshot cursor, generation, and recovery boundary.
- Local broadcasts mark peers dirty; send failures request repair; success after a recorded failure triggers reconnect repair.
- Inbound queue overflow is surfaced to the scheduler after releasing the queue lock.
- Enforcement uses drain_with_ack() and acknowledges only after processing; WAL batches return the sender's page-end LSN only after all contained mutations are accepted.
- Full-state pages are bounded to 4,096 entries per page and 16 entries per wire frame. WAL pages are also bounded, and active page sends use a shared writer semaphore and per-frame deadlines.
- Caller-owned snapshot_page(cursor, limit) cursors keep one peer from advancing another peer's pagination position.
- CRDT state includes origin dots and tombstone high-water marks so concurrent origin mutations are not collapsed into one peer's aggregate event.

## Acceptance work still required

- Add and test an explicit receiver-confirmed recovery completion frame binding the recovery generation, captured WAL boundary, and a deterministic digest of the completed state rotation.
- Run real multi-node fault injection: disconnect/reconnect, process restart across checkpoint/retention, duplicate and reordered pages, queue pressure, concurrent mutation during pagination, and repeated ACK loss.
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

The integrated branch contains tests for WAL LSN paging, checkpoint/tombstone roundtrip, checkpoint-seed plus WAL-tail folding, positive/negative application ACKs, enforcement-level WAL-batch TCP handling, and a recovery-boundary regression that proves repeated recovery requests do not move the captured boundary.

## Acceptance bar

Do not merge or enable production mesh until all of the following are demonstrated on one immutable SHA:

- convergence with dropped, duplicated, delayed, and reordered messages;
- every page and tombstone eventually visited for each peer independently;
- recovery does not advance past the captured boundary without the required completion proof;
- bounded recovery latency after reconnect/overflow/failure and no hot retry loop;
- no lost dirty signal when mutation races with sync completion;
- feature-off and mesh-enabled workspace gates, strict Clippy, formatting, dependency audit, and independent review are green.

Until those gates pass, keep mesh optional and disabled in production.
