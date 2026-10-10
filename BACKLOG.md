# RamShield Production-Essential Audit Backlog



**Target branch:** `audit-branch`

**Baseline HEAD:** `70cd373f3c543ea796dd684197535956b49d6b0c`

**Baseline master:** `821eab72e93e584cd6835d1f95171d5d380daaec`

**Latest recorded CI:** [run 38041528476](https://github.com/grep999/ramshield/actions/runs/38041528476) — Check, Build, Test, and Clippy passed on HEAD `808f97b8032230146c0012fb928059a0f77e229a`. Formatting is not covered by this workflow and remains unverified.

**Operating rule:** one bounded fix per pass; every fix is revalidated on the exact resulting SHA. No merge, force-push, rebase, or changes to `kiddo_fix` / PR #3 / PR #5.

**Scope:** this backlog tracks only production-essential fixes for the core RamShield product. Mesh feature design and in-development mesh implementation work live in [`docs/development/MESH_FEATURE_PLAN.md`](docs/development/MESH_FEATURE_PLAN.md) and are not core release blockers while mesh remains optional and disabled. Any mesh code enabled in a deployment must still meet its own acceptance gate before production use.

**Meaning of 100/100:** every applicable production-essential P0/P1 is closed with regression evidence, P2s are closed or explicitly justified as non-applicable, all core acceptance gates pass on one SHA, and an independent reviewer approves that SHA. A green build alone is not 100/100.



## Priority definitions



- **P0 — release blocker:** security boundary, enforcement correctness, data corruption/loss, or unrecoverable failure.

- **P1 — required before production claim:** bounded resource use, lifecycle/recovery, correctness under concurrency/partial failure, or missing essential test coverage.

- **P2 — quality completion:** reproducibility, maintainability, docs, and coverage needed to make the score defensible.

- **Status vocabulary:** `OPEN`, `IN PROGRESS`, `PASS (exact SHA)`, `BLOCKED`, `NOT APPLICABLE (evidence)`. Do not mark PASS from source inspection alone.



## P0 — security and correctness blockers



| ID | Area | Required work / acceptance evidence | Status |

|---|---|---|---|

| A-004 | Shared-memory writer parity | Root `publish_rule` uses exclusive even→odd CAS + under-lock revalidation. C `ramshield_shm_clear_slot`/`flush_all` mirrors CAS protocol. Concurrent writer stress tests added. `verify_shm_header.sh` asserts 64-byte ABI. Nested `rs/` out of scope (A-021). | PASS (exact SHA pending CI) |

| A-005 | Replay-store security | Live markers are never LRU-evicted under global/per-key pressure; saturation returns `capacity`. Time sampled under lock; poison fails closed. Tests: unit module + `crates/ramshield-protocol/tests/replay.rs`. Ported security model aligned with kiddo_fix review. | PASS (exact SHA pending CI) |



## P1 — boundedness, failure handling, and operational correctness



| ID | Area | Required work / acceptance evidence | Status |

|---|---|---|---|

| A-015 | Detection construction | Public `new` is `#[deprecated]` and panics only for legacy tests; production uses `try_new` / `try_new_with_shm_path`. Callers updated; unit test proves SHM open failure returns `Err` without panic. | PASS (exact SHA pending CI) |

| A-016 | Native packet ingest | TPACKET_V3 validators retained; property sweeps cover blk_len, header/packet edges, multi-block ring bounds, descriptor chain alignment/overflow, and 2k deterministic malformed mixes (no panic). Linux-only unit path exercised by `cargo test --lib tpacket`. | PASS (exact SHA pending CI) |

| A-017 | XDP/SYNPROXY operational correctness | Capacity rejection, reconcile-after-map-loss (v4/v6/CIDR), partial-install failure isolation, StubXdp no kernel effects, CIDR map routing, synproxy unsupported-env / hostile iface / uninstall cleanup tests. | PASS (exact SHA pending CI) |

| A-018 | WAL / checkpoint recovery | Existing golden/crash/fuzz suite plus boundary tests: crash/restart keeps checkpoint+durable prefix, disk-full after checkpoint preserves prior state, truncated tail repair is idempotent. Fault seams: fail_next_write/diskfull/dir_fsync. | PASS (exact SHA pending CI) |

| A-019 | Dashboard / IPC auth | Test session expiry, login throttling, cookie flags, CSRF/origin boundaries, replay rejection, malformed requests, and failure-safe config. Validate actual deployment defaults, not only helper functions. | OPEN |

| A-020 | Resource bounds elsewhere | Audit all attacker-influenced queues, maps, buffers, task spawns, native event channels, analytics windows, and per-IP state for hard caps and overload signals. Each finding must include a saturation regression test. | OPEN |

| A-021 | Root vs nested workspace | **DECIDED 2026-10-10:** `rs/` is archival/experimental only — not maintained, not shipped, not root-CI-gated. Production product is the root workspace. Evidence: `docs/WORKSPACE_SCOPE.md`, `rs/STATUS.md`. Reversal requires pinned CI + parity policy (see doc). | PASS (exact SHA pending push) |



## P2 — full quality and reproducibility



| ID | Area | Required work / acceptance evidence | Status |

|---|---|---|---|

| A-030 | Formatting and gates | Add `cargo fmt --all -- --check` to CI and run it on the candidate SHA. Current green CI does not include this gate. | OPEN |

| A-031 | Core feature matrix | CI must exercise default/core-only and the production `full` configuration, all-targets check/build, unit/integration tests, and strict Clippy without feature-unification masking a broken core configuration. Mesh-only and mesh-integrated configurations are tracked in the separate mesh feature plan. | IN PROGRESS |

| A-033 | Shared-memory ABI | Verify C header size/alignment/offsets match Rust on every supported target; run concurrent readers/writers and sanitizer/stress tests where supported. | OPEN |

| A-034 | Dependency / supply chain | Run locked dependency audit, license/advisory checks, and review unsafe-code and transitive dependency changes. Record exceptions with rationale. | OPEN |

| A-035 | Public API / error semantics | Audit panics, swallowed errors, ambiguous return values, unchecked conversions, and inaccurate comments across crate boundaries. Prioritize production call paths and public APIs. | OPEN |

| A-036 | Core docs and claims | Replace stale code-health notes; document supported core deployment commands and operational assumptions. Add/update README files for analytics and CGNAT; verify enforcement and detection documentation against code. Mesh threat model, deployment, and module docs are tracked in the separate mesh feature plan. | OPEN |

| A-037 | Core benchmarks and regression budgets | Establish reproducible baselines for detection, enforcement, WAL, native ingest, and memory under load. Set only evidence-based regression thresholds. Mesh merge/gossip performance is tracked in the separate mesh feature plan. | OPEN |

| A-038 | Final independent audit | Two independent source-only reviewers assess the same exact candidate SHA and current diff against master; each records findings, severity, evidence, and APPROVE/BLOCK. Fix all P0/P1; resolve or justify P2s. | OPEN |

| A-039 | Final acceptance report | Attach exact SHA, compare-to-master, workflow URLs/job conclusions, fmt result, test counts, known limitations, module scores with evidence, and a prioritized residual-risk statement. No blanket 100/100 without this evidence. | OPEN |



## Separate in-development feature



Mesh is intentionally excluded from this production-essential remediation backlog. Track its feature requirements, open correctness work, development sequence, and mesh-specific acceptance criteria in [`docs/development/MESH_FEATURE_PLAN.md`](docs/development/MESH_FEATURE_PLAN.md). Do not use incomplete mesh feature work to inflate or block the core remediation score while mesh remains optional and disabled; do not enable mesh in production before its own acceptance gate passes.



## 100/100 acceptance gate



A candidate is eligible only when all are true:



- Every P0 and P1 is `PASS` with a regression test or evidence-based `NOT APPLICABLE` decision.

- All P2 tasks are closed or have a reviewer-approved rationale.

- Core-only and production full-feature checks/tests, all-targets check/build, and applicable ABI/recovery gates pass on the same SHA. Mesh-specific gates are owned by the separate mesh feature plan.

- `cargo fmt --all -- --check`, strict Clippy, and `git diff --check origin/master...HEAD` pass on that SHA.

- Core security/recovery tests include malformed input, overload, concurrency, crash/restart, and failure injection where relevant.

- Two independent reviewers approve the exact same SHA; no unresolved P0/P1 finding remains.

- The final report links all run logs and states remaining environmental/coverage limits.



## Workflow / handoff contract



Every pass records the base SHA, exact result SHA, one bounded selected change, changed paths, regression coverage, commands actually run with exit status, CI run/job links, remaining findings, and next action. Work only on `audit-branch`. Never merge, force-push, rebase, or alter `kiddo_fix`, PR #3, or PR #5 as part of this workflow.
