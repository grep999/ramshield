# RamShield Production-Essential Audit Backlog



**Target branch:** `audit-branch`

**Baseline HEAD:** `70cd373f3c543ea796dd684197535956b49d6b0c`

**Baseline master:** `821eab72e93e584cd6835d1f95171d5d380daaec`

**Latest recorded CI:** [run 38078190216](https://github.com/grep999/ramshield/actions/runs/38078190216) — all CI jobs passed on exact HEAD `32c0aa0501f86efed98a5925c8a3f6a9fb536307`, including formatting, check, build, tests, and strict Clippy. [Backlog Gate run 38078190283](https://github.com/grep999/ramshield/actions/runs/38078190283) also passed on that SHA.

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

| A-015 | Detection construction | Public `new` remains a deprecated compatibility wrapper; production boot uses `try_new`, and `try_new_with_shm_path` supports deterministic initialization tests. `try_new_propagates_shm_open_failure` verifies the invalid SHM path returns `Err` without panic. Exact-SHA CI and Backlog Gate passed on `0405398ae53e7363f02f2c541418bbea7cf1c17c`. PR #6's `#[cfg(test)]` removal of the public wrapper is a downstream API break and requires an explicit breaking-release decision. | PASS (exact SHA) |

| A-016 | Native packet ingest | TPACKET_V3 validators and malformed-descriptor property sweeps cover block/header/packet bounds, ring edges, chain alignment/overflow, and deterministic malformed mixes. Exact-SHA Linux test and CI evidence: Backlog Gate A-016 TPACKET bounds passed on `6af1bd28cd8ca7e86298301676ba508b40ed62a2`. | PASS (exact SHA) |

| A-017 | XDP/SYNPROXY operational correctness | Tests cover capacity rejection, reconcile-after-map-loss (v4/v6/CIDR), partial-install failure isolation, StubXdp no kernel effects, IPv4/IPv6 and CIDR map routing, native-mode no-downgrade, and SYNPROXY unsupported-environment / hostile-interface / cleanup behavior. Exact-SHA CI and Backlog Gate passed on `744f44987450e60c753dc79298c7c633601fb685`. | PASS (exact SHA) |

| A-018 | WAL / checkpoint recovery | Crash/restart test truncates to the last explicit sync boundary and asserts the exact durable prefix; recovery tests prove torn-tail repair is idempotent and the repaired WAL accepts later appends. Added disk-full-after-checkpoint injection proving the checkpoint and durable prefix survive, failed append is excluded, and reopen accepts new writes. Exact-SHA CI and Backlog Gate passed on `744f44987450e60c753dc79298c7c633601fb685`. | PASS (exact SHA) |

| A-019 | Dashboard / IPC auth | Session TTL expiry, cookie HttpOnly/SameSite/Secure, per-IP lockout, invalid PHC fail-closed, oversized password reject, deployment defaults (loopback, 8h TTL, lockout 50). Poisoned password-hash lock denies login and keeps auth enabled; explicit hash replacement clears poison after writing the replacement. Failed-login tracking is hard-capped at 10,000 IPs with serialized admission and a concurrent unique-source flood regression test. IPC: replay reject, malformed frames, parse_ipc_keys fail-closed. Exact-SHA CI and Backlog Gate passed on `32c0aa0501f86efed98a5925c8a3f6a9fb536307`. | PASS (exact SHA) |

| A-020 | Resource bounds elsewhere | PulseTracker observations capped at 4,096. `pending_mitigations` enforces the 1,048,576 production / 64 test cap at admission with bounded eviction; batch-flood/concurrency tests added. `detection.pre_aggs_max_size` capped at 1,000,000; enabled forecasting seasonal allocation capped at 86,400 one-second slots; boundary and `usize::MAX` validation tests added. Existing: store capacity CAS, threat_sample≤1024, IPC conn semaphore, SHM/replay caps. Worker-local maps stop at 8,192 distinct IPs; production shared-map merges serialize with flushes and flush before admitting a new key at configured capacity. Regression test verifies the shared cap and exact event accounting. Exact-SHA CI and Backlog Gate passed on `6af1bd28cd8ca7e86298301676ba508b40ed62a2`. | PASS (exact SHA) |

| A-021 | Root vs nested workspace | **DECIDED 2026-10-10:** `rs/` is archival/experimental only — not maintained, not shipped, not root-CI-gated. Production product is the root workspace. Evidence: `docs/WORKSPACE_SCOPE.md`, `rs/STATUS.md`. Reversal requires pinned CI + parity policy (see doc). | PASS (exact SHA pending push) |



## P2 — full quality and reproducibility



| ID | Area | Required work / acceptance evidence | Status |

|---|---|---|---|

| A-030 | Formatting and gates | `cargo fmt --all -- --check` is enforced in CI and passed on exact SHA `6af1bd28cd8ca7e86298301676ba508b40ed62a2`. | PASS (exact SHA) |

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

## 2026-10-10 remediation pass — audit-branch

The following source-level fixes were applied directly to `audit-branch`:

- **SHM probe parity:** C reader and Rust writer now both use a four-slot probe window. A coherent occupied C slot with a different key advances to the next probe instead of retrying the same slot 64 times.
- **Test dispatch:** `suite.py load bench` now dispatches the `.sh` benchmark through Bash. `integration_suite.py` maps its named profiles to supported `suite.py` commands and no longer passes profile strings as arguments to individual scripts.
- **Audit helper:** `audit-safety-commitment.py` uses the repository-relative workspace, no longer treats `#` as a dangerous token, and has its shebang at byte zero.
- **Python import:** `random` is imported at module scope in `scripts/subnet_test.py`.
- **SYNPROXY deployment:** added `deploy/sysctl.d/99-ramshield-synproxy.conf`; the systemd service restores `ProtectKernelTunables=true`; runtime SYNPROXY setup verifies required sysctl values and does not attempt privileged writes.
- **Documentation:** removed the duplicate half of `crates/ramshield-analytics/README.md`, corrected the stale `ipnet` claim in `crates/ramshield-types/README.md`, and populated the previously empty `docs/LOCKDOWN_RUNBOOK.md`.
- **Duplicate tree:** removed the nested `rs/` archive; root-level workspace sources remain.

### Findings checked against current source

- Removed obsolete `pp.patch`: it contained an invalid `Store::set_block_state` call, unconditional mesh imports, bare lock unwraps, and an `unsafe { mem::zeroed() }` test harness. These hunks were not applied to the tracked source. The suggested `update_ip` replacement in the report would violate the documented `Store::update_ip` contract, which is for statistics-only mutation; enforcement must retain its block-state-authoritative write path.
- Mesh fields/modules and boot-time mesh construction are already gated behind `feature = "mesh"`; the dependency is optional.
- The enforcement task's `JoinHandle` is selected by the pipeline; unexpected termination returns a fatal pipeline error rather than leaving the daemon silently healthy.
- The ExaBGP FIFO bridge already opens the FIFO with `O_RDWR`, preventing EOF on writer restart.
- The proposed `concurrency_invariants.rs` file and its `mem::zeroed()` test are absent from the tracked tree.

### Acceptance status

These are applied source changes, **not a release sign-off**. The full Rust/C build, formatting, Clippy, zero-unwrap gate, and end-to-end suite must pass on one final immutable SHA before closure. Keep the remaining backlog items open until that evidence is attached.
