# RamShield Production-Essential Audit Backlog



**Target branch:** `audit-branch`

**Baseline HEAD:** `70cd373f3c543ea796dd684197535956b49d6b0c`

**Baseline master:** `821eab72e93e584cd6835d1f95171d5d380daaec`

**Latest recorded CI:** [run 38081795011](https://github.com/grep999/ramshield/actions/runs/38081795011) — all CI jobs passed on exact HEAD `59d4bc6d68db132223e9817b61ac4c399b5e965e`, including core-only/full-feature/all-feature tests, build/check, strict Clippy, formatting, dependency audit, and C SHM ABI stress. [Backlog Gate run 38081795019](https://github.com/grep999/ramshield/actions/runs/38081795019) also passed on that SHA, including A-031 core matrix, A-033 SHM ABI/TSAN/AArch64, A-016, formatting, whitespace, and evidence manifest.

**Operating rule:** one bounded fix per pass; every fix is revalidated on the exact resulting SHA. No merge, force-push, rebase, or changes to `kiddo_fix` / PR #3 / PR #5.

**Scope:** this backlog tracks only production-essential fixes for the core RamShield product. Mesh feature design and in-development mesh implementation work live in [`docs/development/MESH_FEATURE_PLAN.md`](docs/development/MESH_FEATURE_PLAN.md) and are not core release blockers while mesh remains optional and disabled. Any mesh code enabled in a deployment must still meet its own acceptance gate before production use.

**Meaning of 100/100:** every applicable production-essential P0/P1 is closed with regression evidence, P2s are closed or explicitly justified as non-applicable, all core acceptance gates pass on one SHA, and an independent reviewer approves that SHA. A green build alone is not 100/100.

**State review (2026-10-12):** A-035 passed exact-SHA local and hosted validation on `0e48c3bc6ce7997d5e57421930715706b9627f71`; the final two-reviewer independent audit remains A-038. A-036 is partial (see its row), and A-037, A-038, and A-039 remain open. Mesh remains explicitly outside this core backlog and is tracked in `docs/development/MESH_FEATURE_PLAN.md`. The mesh prototype's green checks do not change core backlog status or constitute mesh production acceptance.



## Priority definitions



- **P0 — release blocker:** security boundary, enforcement correctness, data corruption/loss, or unrecoverable failure.

- **P1 — required before production claim:** bounded resource use, lifecycle/recovery, correctness under concurrency/partial failure, or missing essential test coverage.

- **P2 — quality completion:** reproducibility, maintainability, docs, and coverage needed to make the score defensible.

- **Status vocabulary:** `OPEN`, `IN PROGRESS`, `PASS (exact SHA)`, `BLOCKED`, `NOT APPLICABLE (evidence)`. Do not mark PASS from source inspection alone.



## P0 — security and correctness blockers



| ID | Area | Required work / acceptance evidence | Status |

|---|---|---|---|

| A-004 | Shared-memory writer parity | Root `publish_rule` uses exclusive even→odd CAS + under-lock revalidation. C `ramshield_shm_clear_slot`/`flush_all` mirrors CAS protocol. Concurrent writer and C reader/writer stress tests added. C/Rust layout checks cover size, alignment, and field offsets; ThreadSanitizer passed where supported. Nested `rs/` out of scope (A-021). Exact-SHA CI and Backlog Gate passed on `a9a3bf16c47680108993b75bf81b4b7e59bb31f8`. | PASS (exact SHA) |

| A-005 | Replay-store security | Live markers are never LRU-evicted under global/per-key pressure; saturation returns `capacity`. Time sampled under lock; poison fails closed. Tests: unit module + `crates/ramshield-protocol/tests/replay.rs`. Ported security model aligned with kiddo_fix review. Exact-SHA CI and Backlog Gate passed on `a9a3bf16c47680108993b75bf81b4b7e59bb31f8`. | PASS (exact SHA) |



## P1 — boundedness, failure handling, and operational correctness



| ID | Area | Required work / acceptance evidence | Status |

|---|---|---|---|

| A-015 | Detection construction | Public `new` remains a deprecated compatibility wrapper; production boot uses `try_new`, and `try_new_with_shm_path` supports deterministic initialization tests. `try_new_propagates_shm_open_failure` verifies the invalid SHM path returns `Err` without panic. Exact-SHA CI and Backlog Gate passed on `a9a3bf16c47680108993b75bf81b4b7e59bb31f8`. PR #6's `#[cfg(test)]` removal of the public wrapper is a downstream API break and requires an explicit breaking-release decision. | PASS (exact SHA) |

| A-016 | Native packet ingest | TPACKET_V3 validators and malformed-descriptor property sweeps cover block/header/packet bounds, ring edges, chain alignment/overflow, and deterministic malformed mixes. Exact-SHA CI and Backlog Gate A-016 TPACKET bounds passed on `a9a3bf16c47680108993b75bf81b4b7e59bb31f8`. | PASS (exact SHA) |

| A-017 | XDP/SYNPROXY operational correctness | Tests cover capacity rejection, reconcile-after-map-loss (v4/v6/CIDR), partial-install failure isolation, StubXdp no kernel effects, IPv4/IPv6 and CIDR map routing, native-mode no-downgrade, and SYNPROXY unsupported-environment / hostile-interface / cleanup behavior. Exact-SHA CI and Backlog Gate passed on `a9a3bf16c47680108993b75bf81b4b7e59bb31f8`. | PASS (exact SHA) |

| A-018 | WAL / checkpoint recovery | Crash/restart test truncates to the last explicit sync boundary and asserts the exact durable prefix; recovery tests prove torn-tail repair is idempotent and the repaired WAL accepts later appends. Added disk-full-after-checkpoint injection proving the checkpoint and durable prefix survive, failed append is excluded, and reopen accepts new writes. Exact-SHA CI and Backlog Gate passed on `a9a3bf16c47680108993b75bf81b4b7e59bb31f8`. | PASS (exact SHA) |

| A-019 | Dashboard / IPC auth | Session TTL expiry, cookie HttpOnly/SameSite/Secure, per-IP lockout, invalid PHC fail-closed, oversized password reject, deployment defaults (loopback, 8h TTL, lockout 50). Poisoned password-hash lock denies login and keeps auth enabled; explicit hash replacement clears poison after writing the replacement. Failed-login tracking is hard-capped at 10,000 IPs with serialized admission and a concurrent unique-source flood regression test. IPC: replay reject, malformed frames, parse_ipc_keys fail-closed. Exact-SHA CI and Backlog Gate passed on `a9a3bf16c47680108993b75bf81b4b7e59bb31f8`. | PASS (exact SHA) |

| A-020 | Resource bounds elsewhere | PulseTracker observations capped at 4,096. `pending_mitigations` enforces the 1,048,576 production / 64 test cap at admission with bounded eviction; batch-flood/concurrency tests added. `detection.pre_aggs_max_size` capped at 1,000,000; enabled forecasting seasonal allocation capped at 86,400 one-second slots; boundary and `usize::MAX` validation tests added. Existing: store capacity CAS, threat_sample≤1024, IPC conn semaphore, SHM/replay caps. Worker-local maps stop at 8,192 distinct IPs; production shared-map merges serialize with flushes and flush before admitting a new key at configured capacity. Regression test verifies the shared cap and exact event accounting. Exact-SHA CI and Backlog Gate passed on `a9a3bf16c47680108993b75bf81b4b7e59bb31f8`. | PASS (exact SHA) |

| A-021 | Root vs nested workspace | **DECIDED 2026-10-10:** `rs/` is archival/experimental only — not maintained, not shipped, not root-CI-gated. Production product is the root workspace. Evidence: `docs/WORKSPACE_SCOPE.md`, `rs/STATUS.md`. Reversal requires pinned CI + parity policy (see doc). Root workspace CI and Backlog Gate passed on `a9a3bf16c47680108993b75bf81b4b7e59bb31f8`. | PASS (exact SHA) |



## P2 — full quality and reproducibility



| ID | Area | Required work / acceptance evidence | Status |

|---|---|---|---|

| A-030 | Formatting and gates | `cargo fmt --all -- --check` is enforced in CI and passed on exact SHA `a9a3bf16c47680108993b75bf81b4b7e59bb31f8`. | PASS (exact SHA) |

| A-031 | Core feature matrix | CI and Backlog Gate separately run default/core-only and production `full` / all-feature configurations: locked all-targets check/build, workspace tests, and strict Clippy. Mesh-only and mesh-integrated configurations remain in the separate mesh feature plan. Exact-SHA CI and Backlog Gate passed on `a9a3bf16c47680108993b75bf81b4b7e59bb31f8`. | PASS (exact SHA) |

| A-033 | Shared-memory ABI | C header statically asserts 64-byte size/alignment and every `RamshieldShmRuleEntry` field offset; Rust layout is asserted at compile time and runtime. CI runs C pthread reader/writer stress, ThreadSanitizer, and AArch64 Rust/C cross-compilation checks. All passed on exact SHA `a9a3bf16c47680108993b75bf81b4b7e59bb31f8`. | PASS (exact SHA) |

| A-034 | Dependency / supply chain | CI runs `cargo audit --no-yanked` and `cargo deny check`; both passed on exact SHA `862ea51f5251d974de6d9f21f375c796282bddf0`. Workspace crates now declare MIT licenses; local path dependencies carry explicit package-version constraints; cargo-deny uses current v2 advisory policy and denies advisories, yanked crates, wildcard dependencies, and unknown sources. Reviewed production unsafe boundaries in TPACKET ring parsing, SHM mmap access, and XDP/BPF syscall helpers; `raw_get_next_key` now documents the requirement that kernel-written key bytes form a valid `K`. Remaining cargo-deny duplicate-version warnings are intentionally visible: `thiserror` v1 is used by `ramshield-types` while root/enforcement use v2; `getrandom`, `r-efi`, `rand_core`, and `syn` duplicates are transitive graph splits and are not force-unified without compatibility review. No advisory, license, or source-policy errors remain. | PASS (exact SHA) |

| A-035 | Public API / error semantics | `doctor` verifies `ip link show` exit status; CLI IPC enforces newline framing, a 1 MiB response cap, 5-second read/write timeouts, UTF-8, and valid JSON; CLI HMAC auth accepts a configurable key ID and enforces the server's 16-byte minimum key size; SYNPROXY ports reject malformed and non-UTF-8 environment values while an intentional blank list clears defaults. Regression tests cover these cases, CLI framing and parse errors, and doctor config/exit semantics. Dashboard Argon2 concurrency docs were corrected on `748feb2010e78a614b684b4d9cc1c4568a04fcb2`. Exact candidate `0e48c3bc6ce7997d5e57421930715706b9627f71`: full CI [run 38297139446](https://github.com/grep999/ramshield/actions/runs/38297139446) and Backlog Gate [run 38297139440](https://github.com/grep999/ramshield/actions/runs/38297139440) passed. Local core/full tests, check/build, strict Clippy, formatting, `cargo audit --no-yanked`, and `cargo deny check` also passed. The final independent audit is tracked under A-038. | PASS (exact SHA `0e48c3bc6ce7997d5e57421930715706b9627f71`) |

| A-036 | Core docs and claims | Replace stale code-health notes; document supported core deployment commands and operational assumptions. Add/update README files for analytics and CGNAT; verify enforcement and detection documentation against code. Mesh threat model, deployment, and module docs are tracked in the separate mesh feature plan. | PASS (exact SHA `9e06802abe371c14b3386fb1bce16cc3c1916e41`) — docs refreshed; CI run 39950403767 and Backlog Gate run 39950403772 passed on this SHA (fmt, whitespace, A-031, A-016, A-033). Independent review remains under A-038. |

| A-037 | Core benchmarks and regression budgets | Establish reproducible baselines for detection, enforcement, WAL, native ingest, and memory under load. Set only evidence-based regression thresholds. Mesh merge/gossip performance is tracked in the separate mesh feature plan. | IN PROGRESS — `docs/development/BENCHMARK_BASELINES.md` records detection/enforcement medians from five sequential runs, Criterion WAL, a synthetic native packet-to-event probe, and whole-process peak RSS. Kernel capture/TPACKET ring and isolated memory behavior are not measured; field-day variance on this host is too high to justify a threshold, so both remain open. |

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

## 2026-10-12 decomposition/hardening pass — audit-branch

Base SHA: `44dfdae`. Result SHA: `cc685f4`. One bounded pass.

Changed paths: `crates/ramshield-config/src/env.rs`, `crates/ramshield-config/src/validate.rs`, `crates/ramshield-enforcement/src/service/enforce.rs`, `src/engine/native.rs`, `src/ipc/server/connection.rs`, `src/ipc/server/handlers.rs` (decomposition, committed earlier via `e65408e`); `crates/ramshield-cgnat/README.md`, `docs/DEPLOYMENT_COMMANDS.md` (this commit).

Regression coverage: existing suite covers the touched paths. New native.rs misalignment assertion in `tpacket_tests`. Checkpoint-barrier scope in `enforce.rs` verified unchanged by diff review — not covered by a new test.

Commands actually run (all exit 0): `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo test --workspace --locked --features full` (all crate test suites pass, 0 failures).

Remaining findings: A-035 passed its recorded exact-SHA gates; its final audit is tracked by A-038. A-036 documentation is complete and awaits exact-SHA gates/review. A-037/A-038/A-039 remain open.

Next action: qualify A-036 on its exact candidate SHA, then continue with A-037 core benchmark baselines.

## 2026-10-13 Independent Audit Review Snapshot

- **Audit SHA**: `09d7af6` (current HEAD on audit-branch)
- **Local CI checks**: `cargo fmt --all -- --check` ✅, `cargo clippy --workspace --all-targets --locked -- -D warnings` ✅, `cargo test --workspace --locked --features full` ✅ (0 failures)
- **A-035**: PASS (exact SHA `0e48c3bc6ce7997d5e57421930715706b9627f71` — CI runs 38297139446/38297139440 passed, local checks verified)
- **A-036**: DOCS COMPLETE (CGNAT README, DEPLOYMENT_COMMANDS.md, INDEX.md). Exact‑SHA gates and independent review pending before final acceptance.
- **A-037**: IN PROGRESS (regression budget table added to BENCHMARKS.md; baselines recorded in `docs/development/BENCHMARK_BASELINES.md`. Kernel capture/TPACKET ring and isolated memory behavior not measured; field-day variance too high for threshold).
- **A-038**: OPEN (final independent audit — requires two independent reviewer assessments. My review in progress; second reviewer still needed).
- **A-039**: OPEN (final acceptance report — pending A‑038 closure. Will attach exact SHA, compare‑to‑master, workflow URLs/job conclusions, fmt result, test counts, known limitations, module scores with evidence, prioritized residual‑risk statement.)
- **A-022**: OPEN (WAL stress / heavy traffic testing task added).

**Next steps**: qualify A-036 exact‑SHA gates, finalize A-038 review, secure second reviewer, draft A-039 acceptance report.
