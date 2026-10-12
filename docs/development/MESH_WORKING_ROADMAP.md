# Mesh Working Roadmap — Metrics-Driven Acceptance

**Status:** roadmap for PR #19; prototype only, production mesh stays disabled.
**Branch:** prototype/condition-driven-mesh-sync-20261011
**Decision rule:** use WAL state and measured convergence/failure behavior as evidence. Do not declare readiness from green compilation or a single-node test.

## Target architecture

- **Authority:** the WAL is the durable mutation history; append + required sync happens before publishing a local mutation.
- **Convergence:** the AWORSet CRDT carries origin dots and tombstones; local WAL LSNs are per-sender transport cursors, never a cluster-global sequence.
- **Repair:** condition-driven scheduling coalesces ordinary changes; reconnect, delivery failure, queue overflow, and periodic repair trigger anti-entropy.
- **Projection:** Store/XDP are projections of committed state. Projection failure must remain observable and retryable; it must not roll back or erase accepted WAL history.
- **Safety:** no blockchain, global ordering, or new telemetry framework by default. Start with structured tracing and counters already available in the service; add a metrics dependency only if the deployment already standardizes on one.

## What is measurable today, and what must be exposed

Existing code gives us useful signals/state: MeshApplyReport counts received/applied/ignored/failed mutations; peer WAL cursors and in-flight guards; bounded queues and configured capacity; per-peer planner reasons, dirty/in-flight state, and retry policy; the projection-pending set; page/ACK outcomes; and WAL head/LSN boundaries. These are not yet a complete exported operational metrics contract.

Add a small, stable telemetry surface before tuning behavior:

| Signal | Derivation | Why it matters |
|---|---|---|
| WAL lag per peer | local WAL head LSN minus acknowledged peer cursor, only for the same source WAL | Detect a peer that is connected but not catching up |
| ACK outcome | accepted, rejected, timeout, disconnect, malformed/mismatched LSN | Distinguish transport reachability from application acceptance |
| Repair outcome/latency | reason, start/end monotonic time, pages and entries visited | Verify condition-driven sync reduces work without delaying repair |
| Queue pressure | queue depth/capacity and overflow/drop count | Make loss/recovery triggers visible before convergence fails |
| Projection debt | pending Store projections; XDP reconcile failures separately | Detect durable-but-not-yet-enforced state |
| Recovery state | generation, captured boundary, state, age, retries | Detect stuck or incorrectly restarted full-sync generations |
| State/page progress | per-peer cursor, pages completed, last successful repair age | Verify fairness and no starvation beyond 4,096 entries |
| Mutation outcomes | existing received/applied/ignored/failed counts, with bounded reason labels | Separate idempotent ignores from malformed or failed mutations |

Do not use peer IPs, mutation IPs, origin IDs, or arbitrary error strings as unbounded metric labels. Put high-cardinality context in sampled/structured logs. Never log shared keys or secrets.

## Phases and exit gates

### Phase 0 — Make the baseline observable

1. Keep the current defaults as the first test baseline: 50 ms coalescing, 30 s periodic repair, 250 ms initial retry capped at 10 s, 25 ms scheduler tick, 2 s application-ACK wait, 16 entries per wire frame, 4,096 entries per state page, 2,048 inbound queue capacity, 64 outbound writer permits, and existing read/write deadlines.
2. Expose the signals above through structured tracing and small counters/gauges without adding a metrics framework.
3. Add one repeatable harness report with initial/final state digest, per-peer acknowledged WAL cursor/head, repair duration, page count, ACK failures, queue overflow, projection debt, and memory/task bounds.
4. Capture a quiet baseline, steady mutation baseline, and overloaded/unreachable-peer baseline. Record the exact commit SHA and commands.

**Exit:** a failed or stalled test explains where progress stopped (WAL, ACK, snapshot rotation, queue, or projection), rather than only timing out.

### Phase 1 — Close the recovery protocol gap (highest priority)

1. Add an explicit receiver-confirmed RecoveryComplete message binding protocol version, peer/session identity, recovery generation, captured WAL boundary, and deterministic digest of the completed state rotation.
2. Receiver emits it only after every required state page is validated and applied; sender accepts only a matching generation/boundary/digest. Duplicate completion is idempotent; stale or mismatched completion is rejected.
3. Keep the sender WAL cursor at/before the captured boundary until this proof is accepted; resume WAL replay at boundary + 1 only afterward.
4. Define digest canonicalization explicitly (stable sort, normalized fields, tombstones included); never hash map iteration order.
5. Test lost, duplicated, delayed, stale-generation, wrong-boundary, and wrong-digest completion messages.

**Exit:** there is no path that skips WAL history because the sender merely finished writing/ACKing individual frames or consumed a local-only completion marker.

### Phase 2 — Prove convergence under adversarial delivery

Build a deterministic two-process test first, then three peers. Inject disconnect/reconnect, dropped and duplicated frames/ACKs, reordering, delayed pages, queue saturation, slow readers, and shutdown during a stalled send. Mutate and unblock/re-block keys while a snapshot page walk is active. Include expiry, tombstones, concurrent origin updates, and more than 4,096 live entries/tombstones.

For each run compare canonical digests of:
1. CRDT blocks + tombstones;
2. effective Store projection;
3. expected XDP/applier state (or a test double with the same observable contract).

**Exit:** all peers eventually converge after faults stop; no tombstone resurrection; no missing later mutation; projection debt returns to zero; dirty signals raised during an in-flight pass are not lost; no peer starves.

### Phase 3 — Prove checkpoint/WAL retention recovery

Test restart with (a) a valid provenance-bearing checkpoint and retained WAL tail, (b) WAL replay after checkpoint, (c) peer cursor inside retained history, (d) peer cursor older than retention, and (e) legacy checkpoint lacking mesh provenance. Exercise crashes between WAL append, sync, CRDT merge, projection, ACK, checkpoint write, and recovery completion.

**Exit:** replay is idempotent, hard-WAL mode fails closed when required provenance/history is absent, and no cursor advances beyond verified durable state.

### Phase 4 — Bound overload, lifecycle, and trust

- Prove queue overflow is counted and schedules repair; establish whether dropping deltas is safe under all message types.
- Test max writer/readers, ACK timeout, unreachable peers, permit release, retry cap, peer fairness, cancellation, startup failure, task join, and clean shutdown.
- Validate every deserialized field and replay case.
- Decide whether the deployment threat model requires per-node identity/signatures. Current shared-key HMAC authenticates a configured mesh member, not the individual origin dot; do not claim origin authenticity until that is implemented and tested.
- Verify feature-off core builds remain independent and mesh-enabled service startup/failure behavior is tested.

**Exit:** bounded resource use and no hot retry loops under a prolonged outage; trust guarantees are explicit and match the implementation.

### Phase 5 — Tune from evidence, then canary

Only tune scheduling, page sizes, timeouts, or concurrency after Phase 2–4 tests emit comparable measurements. Change one parameter at a time and compare:
- p50/p95/p99 repair duration;
- time to convergence after reconnect or overflow;
- WAL lag trajectory and time to drain backlog;
- ACK acceptance/timeout rate;
- queue high-water mark/overflow count;
- projection-debt peak and drain time;
- CPU, resident memory, task count, and outbound concurrency.

Prefer the smallest change that improves a measured bottleneck without violating correctness. Keep 30 s periodic repair as a safety net until fault-injection data proves a safe alternative. Do not pick arbitrary SLOs from intuition: define provisional budgets from the workload and baseline, then tighten them with reproducible results.

**Exit:** reproducible benchmark report, independent review of the exact candidate SHA, all required CI/review/backlog gates green on that same SHA, and a documented rollback path. Only then consider a controlled canary; production enablement is a separate explicit decision.

## Blockers and workarounds

| Blocker | Safe workaround until fixed | Do not do |
|---|---|---|
| No receiver-confirmed recovery transaction | Keep mesh disabled in production; keep cursor pinned at captured boundary during recovery | Treat page ACKs or local marker as proof of whole-snapshot completion |
| Concurrent mutations during keyset pagination | Periodic full repair plus generation-aware restart; stress-test mutation races | Claim a page walk is an atomic snapshot |
| Queue overflow / dropped delta | Mark peer dirty and run repair; expose overflow and backlog | Silently discard and report healthy |
| Projection failure after durable acceptance | Keep WAL/CRDT truth, record projection debt, retry reconciliation | Roll back committed history or ACK before durable acceptance |
| Cursor older than WAL retention | Run full-state repair with explicit boundary proof, then replay tail | Jump cursor to current head before repair completes |
| Shared-key HMAC lacks origin attribution | Restrict membership to trusted nodes and document trust boundary | Claim signatures or per-origin authentication exist |
| Green CI without network faults | Keep PR draft; use a multi-process fault-injection gate | Merge based on compile/unit success alone |

## Execution order for the next pass

1. Add telemetry needed to observe progress and failures (Phase 0).
2. Implement the receiver-confirmed recovery completion protocol (Phase 1).
3. Build a deterministic multi-process fault harness and convergence digest checks (Phase 2).
4. Cover checkpoint/retention crash windows (Phase 3).
5. Finish overload, lifecycle, and origin-trust decisions (Phase 4).
6. Benchmark and tune only after correctness gates (Phase 5).

**Current gate:** PR #19 remains draft. Exact-head CI, Review Pipeline, and Backlog Gate are green on da877ec5a6a53a744a3376f4d590783df7f179f9, but multi-node fault-injection acceptance, explicit recovery completion, origin authentication decision, and independent review remain open. Keep production mesh disabled.
