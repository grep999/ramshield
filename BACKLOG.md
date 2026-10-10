# RamShield Audit Backlog

**Target branch:** `audit-branch`  
**Baseline HEAD:** `70cd373f3c543ea796dd684197535956b49d6b0c`  
**Baseline master:** `821eab72e93e584cd6835d1f95171d5d380daaec`  
**Baseline CI:** [run 38039599076](https://github.com/grep999/ramshield/actions/runs/38039599076) — Check, Build, Clippy, Test all passed on the baseline SHA.  
**Operating rule:** one bounded fix per pass; every fix is revalidated on the exact resulting SHA. No merge, force-push, rebase, or changes to `kiddo_fix` / PR #3 / PR #5.  
**Meaning of 100/100:** every P0/P1 is closed with regression evidence, P2s are closed or explicitly justified as non-applicable, all acceptance gates pass on one SHA, and an independent reviewer approves that SHA. A green build alone is not 100/100.

## Priority definitions

- **P0 — release blocker:** security boundary, enforcement correctness, data corruption/loss, or unrecoverable failure.
- **P1 — required before production claim:** bounded resource use, lifecycle/recovery, correctness under concurrency/partial failure, or missing essential test coverage.
- **P2 — quality completion:** reproducibility, maintainability, docs, and coverage needed to make the score defensible.
- **Status vocabulary:** `OPEN`, `IN PROGRESS`, `PASS (exact SHA)`, `BLOCKED`, `NOT APPLICABLE (evidence)`. Do not mark PASS from source inspection alone.

## P0 — security and correctness blockers

| ID | Area | Required work / acceptance evidence | Status |
|---|---|---|---|
| A-001 | Mesh anti-entropy | Prove sync snapshots fit the wire frame and converge when the cluster has more state than one frame / `MAX_SYNC_ENTRIES`. Add deterministic chunking/pagination and tests for >1 frame, repeated sync, tombstones, and eventual convergence. Current `snapshot(4096)` may serialize beyond `MAX_FRAME`; `send_to` rejects oversize frames. | OPEN |
| A-002 | Mesh CRDT semantics | Property/table tests for add/remove/re-add, concurrent nodes, stale/delayed deltas, duplicate/reordered messages, expiry, tombstone GC, and restart. Verify unban cannot be undone by delayed pre-unban state and a later ban remains possible. Correct algorithm defects, not just tests. | OPEN |
| A-003 | Mesh input trust | Validate authenticated envelope schema and bounds after deserialization: node IDs, counter behavior, IP/tier/expiry values, sync vector lengths, key minimum length, timestamp skew, self-origin, and replay behavior. Add malformed/authentication/replay tests. | OPEN |
| A-004 | Shared-memory writer parity | Port the exclusive even→odd CAS writer claim and under-lock revalidation from `crates/ramshield-cgnat/src/shm.rs` to `rs/crates/ramshield-cgnat/src/shm.rs`, or formally retire the duplicate. Add concurrent-writer stress tests and Rust/C ABI validation against the same layout. The nested copy currently uses `fetch_add` to claim a seqlock writer. | OPEN |
| A-005 | Replay-store security | Prove global and per-key capacity behavior never evicts a still-live replay marker. Add collision, duplicate, TTL boundary, global pressure, per-key pressure, and concurrent atomicity tests. The isolated `kiddo_fix` / PR #5 is not to be copied or modified without reviewing its exact diff and obtaining a bounded audit pass. | OPEN |
| A-006 | Enforcement / mesh convergence | Exercise real service integration for remote block/unblock, manual local unblock, CIDR-vs-IP separation, XDP/store failures, expiry, and shutdown. Ensure failed enforcement is observable and state is retried/reconciled rather than silently lost. | OPEN |

## P1 — boundedness, failure handling, and operational correctness

| ID | Area | Required work / acceptance evidence | Status |
|---|---|---|---|
| A-010 | Mesh outbound resource limits | Bound concurrent peer sends and apply connect/write deadlines; verify a dead/slow peer cannot create unbounded Tokio tasks or hold resources indefinitely. Test concurrency ceiling, timeout release, and behavior when the bound is saturated. | OPEN |
| A-011 | Mesh task lifecycle | Retain/own cancellation handles for listener, anti-entropy, and outbound work; define graceful shutdown and join/cancel behavior. Test shutdown with idle, partial-frame, and blocked outbound peers. | OPEN |
| A-012 | Mesh queue overload | Replace undocumented drop-oldest behavior for authenticated Block/Unblock deltas with an explicit loss/recovery policy. Add queue saturation tests proving eventual state convergence and observable overload. | OPEN |
| A-013 | Mesh snapshot fairness | Ensure anti-entropy eventually visits all live entries and tombstones, rather than relying on an arbitrary first `limit` from concurrent maps. Test state larger than a page and verify every page is eventually transmitted. | OPEN |
| A-014 | Mesh integration boundary | Keep mesh optional and independently buildable; prove feature-off configuration fails clearly, feature-off core tests remain mesh-free, feature-on startup errors propagate, and shutdown does not leak background tasks. Current feature boundary exists; full lifecycle proof remains open. | IN PROGRESS |
| A-015 | Detection construction | Remove or deprecate the public `DetectionEngine::new` panic wrapper in favor of fallible `try_new`; update callers/tests and prove initialization failures reach controlled error handling. | OPEN |
| A-016 | Native packet ingest | Retain TPACKET_V3 range validation; add property/fuzz tests for malformed block lengths, packet offsets, descriptor chains, integer overflow, and mapped-ring edges. Exercise the Linux-only path in CI on a supported kernel where feasible. | OPEN |
| A-017 | XDP/SYNPROXY operational correctness | Test kernel map capacity, reconciliation after map loss, CIDR and IPv6 handling, partial install/rollback, privileges/unavailable interfaces, and shutdown cleanup. Make unsupported environment behavior explicit. | OPEN |
| A-018 | WAL / checkpoint recovery | Run crash/restart equivalence and fault injection around append/fsync/checkpoint/snapshot boundaries, TTL restoration, corrupt/truncated WAL, and disk-full errors. Keep recovery gates deterministic and exact-SHA recorded. | OPEN |
| A-019 | Dashboard / IPC auth | Test session expiry, login throttling, cookie flags, CSRF/origin boundaries, replay rejection, malformed requests, and failure-safe config. Validate actual deployment defaults, not only helper functions. | OPEN |
| A-020 | Resource bounds elsewhere | Audit all attacker-influenced queues, maps, buffers, task spawns, native event channels, analytics windows, and per-IP state for hard caps and overload signals. Each finding must include a saturation regression test. | OPEN |
| A-021 | Root vs nested workspace | Decide whether `rs/` is a maintained second product tree. If maintained, add independent pinned-toolchain CI, lockfile and parity tests; if not, document its status and stop treating untested duplicate code as release-ready. | OPEN |

## P2 — full quality and reproducibility

| ID | Area | Required work / acceptance evidence | Status |
|---|---|---|---|
| A-030 | Formatting and gates | Add `cargo fmt --all -- --check` to CI and run it on the candidate SHA. Current green CI does not include this gate. | OPEN |
| A-031 | Feature matrix | CI must exercise default/core-only, `full`, `mesh`, `full,mesh`, all-targets, unit/integration tests, and strict Clippy without feature-unification masking a broken configuration. | IN PROGRESS |
| A-032 | Mesh protocol tests | Cover exact max frame, one byte over, delimiter split across reads, clean EOF, partial EOF, malformed JSON, invalid HMAC, timestamp skew, self-origin, replay, bounded queue, and authenticated sync. Some frame-size/timeout tests already exist; remaining cases are open. | IN PROGRESS |
| A-033 | Shared-memory ABI | Verify C header size/alignment/offsets match Rust on every supported target; run concurrent readers/writers and sanitizer/stress tests where supported. | OPEN |
| A-034 | Dependency / supply chain | Run locked dependency audit, license/advisory checks, and review unsafe-code and transitive dependency changes. Record exceptions with rationale. | OPEN |
| A-035 | Public API / error semantics | Audit panics, swallowed errors, ambiguous return values, unchecked conversions, and inaccurate comments across crate boundaries. Prioritize production call paths and public APIs. | OPEN |
| A-036 | Docs and claims | Replace stale code-health notes; document the mesh threat model, key provisioning/rotation, clock assumptions, network exposure, failure/overload semantics, and exact supported feature commands. Add README files for analytics, CGNAT, and mesh; verify enforcement/detection docs against code. | OPEN |
| A-037 | Benchmarks and regression budgets | Establish reproducible baselines for detection, enforcement, WAL, native ingest, mesh merge/gossip and memory under load. Set only evidence-based regression thresholds. | OPEN |
| A-038 | Final independent audit | Two independent source-only reviewers assess the same exact candidate SHA and current diff against master; each records findings, severity, evidence, and APPROVE/BLOCK. Fix all P0/P1; resolve or justify P2s. | OPEN |
| A-039 | Final acceptance report | Attach exact SHA, compare-to-master, workflow URLs/job conclusions, fmt result, test counts, known limitations, module scores with evidence, and a prioritized residual-risk statement. No blanket 100/100 without this evidence. | OPEN |

## Mesh execution order

1. **M1 / A-010:** cap outbound concurrency and bound connect/write time. Small isolated transport change plus tests.
2. **M2 / A-001 + A-013:** make anti-entropy paginated/chunked and fair across all entries/tombstones; enforce encoded-frame limits before send.
3. **M3 / A-002:** add deterministic CRDT convergence/property tests and correct any counter/tombstone/expiry defects they expose.
4. **M4 / A-003 + A-012:** authenticate and validate all inbound state; define queue overload semantics and replay behavior.
5. **M5 / A-011 + A-014:** explicit lifecycle, bounded shutdown, service-level block/unblock integration tests.
6. Re-run mesh-only tests, core-only tests, full-feature integration, all-targets check/build, strict Clippy, rustfmt, and independent review on one exact SHA.

## 100/100 acceptance gate

A candidate is eligible only when all are true:

- Every P0 and P1 is `PASS` with a regression test or evidence-based `NOT APPLICABLE` decision.
- All P2 tasks are closed or have a reviewer-approved rationale.
- Core-only, mesh-only, full-feature, and all-targets checks/tests pass on the same SHA.
- `cargo fmt --all -- --check`, strict Clippy, and `git diff --check origin/master...HEAD` pass on that SHA.
- Security/recovery tests include malformed input, overload, concurrency, crash/restart, and failure injection where relevant.
- Two independent reviewers approve the exact same SHA; no unresolved P0/P1 finding remains.
- The final report links all run logs and states remaining environmental/coverage limits.

## Workflow / handoff contract

Every pass records the base SHA, exact result SHA, one bounded selected change, changed paths, regression coverage, commands actually run with exit status, CI run/job links, remaining findings, and next action. Work only on `audit-branch`. Never merge, force-push, rebase, or alter `kiddo_fix`, PR #3, or PR #5 as part of this workflow.
