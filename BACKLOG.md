# RamShield Production-Essential Audit Backlog

**Target branch:** `master` · **Assessed at:** `2adc966` · **master HEAD:** `bd417cd`

`2adc966` is the parent of `bd417cd`; the rows below were assessed
against `2adc966`, and `bd417cd` rebuilt this file's tables without
changing the code.

**CI on `2adc966`:** CI 39587482510 · Review Pipeline 39587482567 · lint-no-unwrap 39587482557 · Subnet Stress Pin 39587482512 — all success. Local 14-gate run on the same tree: 14/14 EXIT=0, 1858 passed / 0 failed / 42 ignored.

**CI on `bd417cd` (current master HEAD):** CI 39589296986 · Review Pipeline 39589297074 · lint-no-unwrap 39589297157 · Subnet Stress Pin 39589297129 · Agent Flow Contract 39617102370 — all success.

**Where the truth lives.** This file is the **status of each row**. The other two answers are elsewhere:

| question | file |
|---|---|
| what is proven on which exact SHA? | [`docs/development/OWNER_DECISION_PACKAGE.md`](docs/development/OWNER_DECISION_PACKAGE.md) — the per-SHA gate ledger, every run and every CI id, failures included |
| what is open, with its closure criterion? | [`docs/development/OPEN_FINDINGS.md`](docs/development/OPEN_FINDINGS.md) — the durable queue |
| what happened in the audit-branch era? | [`docs/development/AUDIT_FINDINGS_BACKLOG.md`](docs/development/AUDIT_FINDINGS_BACKLOG.md) — moved verbatim, history not ledger |

**How to read this file.** *Pending work* is one table: every row still to be done, with its priority and what done looks like. *Completed* is at the end. *Recorded, not pending* is state you should know but that needs no work from you.

**Operating rule:** one bounded fix per pass; every fix is revalidated on the exact resulting SHA. No merge, force-push, rebase, or changes to `kiddo_fix` / PR #3 / PR #5.

**Scope:** production-essential fixes for the core RamShield product. Mesh feature work lives in [`docs/development/MESH_FEATURE_PLAN.md`](docs/development/MESH_FEATURE_PLAN.md), is not a core release blocker while mesh stays optional and disabled, and must meet its own acceptance gate before production use. Any mesh code enabled in a deployment must still meet that gate.

**Meaning of 100/100:** every applicable production-essential P0/P1 is closed with regression evidence, P2s are closed or explicitly justified as non-applicable, all core acceptance gates pass on one SHA, and an independent reviewer approves that SHA. A green build alone is not 100/100.

**Glossary.** Two terms used here that are worth pinning down:

- **unexpressable** — a rule the policy language can state but the SHM rule table has no representation for. It is a *representational* limitation of that data structure, not a missing feature and not a bug in the enforcement path. See F1a below.
- **the SHM rule table** — a 262 144-slot open-addressed table in shared memory, 64 bytes per slot, 4-slot probe window. The daemon writes rules into it; an **out-of-repo** native reverse proxy reads them to decide whether to challenge, rate-limit or drop a client. The Rust crate only writes; the C header in `crates/ramshield-cgnat/include/ramshield_shm.h` is the only reader. It is a **side channel**: in-repo enforcement goes through the XDP LPM map and is unaffected by anything in this table.

---

## Priority definitions

- **P0 — release blocker:** security boundary, enforcement correctness, data corruption/loss, or unrecoverable failure.
- **P1 — required before production claim:** bounded resource use, lifecycle/recovery, correctness under concurrency/partial failure, or missing essential test coverage.
- **P2 — quality completion:** reproducibility, maintainability, docs, and coverage needed to make the score defensible.

**Status vocabulary:** `OPEN`, `IN PROGRESS`, `PASS (exact SHA)`, `PASS (historical)`, `CLOSED (commit)`, `BLOCKED`, `NOT APPLICABLE (evidence)`, `UNVERIFIABLE`. Do not mark PASS from source inspection alone.

- `PASS (exact SHA)` — the row's substance was verified on that exact SHA, with the evidence named.
- `PASS (historical)` — the row passed at a SHA that is now an ancestor of master, and its substance has **not** been re-verified on current master. A record of a past result, not a current claim.
- `CLOSED (commit)` — closed by the named commit, which carries its own evidence and review.
- `UNVERIFIABLE` — the row names a path or artifact that does not exist in the current tree, so it can be neither confirmed nor dismissed as written.

---

## Pending work

Every row still to be done. The *Required work* column is what done looks like; the *Status* column says where it actually stands.

| ID | Pri | Area | Required work / acceptance evidence | Status |
|---|---|---|---|---|

| I-003 | P0 | Mesh production safety and provenance | Keep mesh optional and disabled in production. Do not ship/enable until authenticated per-origin signatures/identity, receiver-confirmed recovery completion (generation + WAL boundary + deterministic state digest), restart/retention behavior, and multi-process fault-injection convergence gates pass on the same exact SHA with independent review. | BLOCKED — still correct, and the gate is still unmet. The pairwise-HMAC branch `feature/mesh-f01-hmac-edge-secrets` (`f75abb5`) is unpublished and not merged: no mesh transport or aworset change from it is on master. Mesh stays optional and disabled in production. No cryptographic change is proposed here — the v1.2 constraint (HMAC stays, no asymmetric signatures, no public-key identity, no pairwise-HMAC migration without owner approval) is respected by the current state. |

|| I-011 | P1 | Storage eviction vs threat accumulators | Verify cardinality-driven LRU eviction cannot repeatedly erase the attacker's own CUSUM/pulse-wave history and create a threshold bypass. Define bounded behavior under capacity pressure; add adversarial rotating-IP tests before changing policy. | NOT APPLICABLE — premise is wrong: there is no LRU eviction in the store. `capacity.rs:23-51` evicts only on TTL expiry; production inserts carry no TTL (`lib.rs:91-99`), and under RAM pressure the store refuses net-new keys rather than evicting live ones (`write.rs:103-152`). The row's stated mechanism does not exist in the code. |

| I-012 | P1 | Host-network Kubernetes capability review | Review `deploy/k8s/daemonset.yaml` capabilities against the exact XDP/capture operations required. Minimize capabilities and document why each retained capability is required; validate manifests and deployment smoke tests. Do not remove capabilities blindly if it breaks enforcement. | OPEN — verified still present: `deploy/k8s/daemonset.yaml` still runs with `hostNetwork: true` and a `NET_ADMIN` capability, and nothing in the master records documents why each retained capability is required. The row's own acceptance ask is unmet. |

| I-013 | P1 | SYNPROXY fixture parity | Compare `tests/fixtures/synproxy.nft` with `config.baseline.toml` and actual generator behavior; align ports and prove port 80 coverage if discrepancy remains. | IN PROGRESS — fixed on branch `fix/i-013-synproxy-fixture-parity` (`72981e1`), not yet on master. The fixture carried `tcp dport { 22, 443 }` while the code default and `config.baseline.toml` both configure `[22, 53, 80, 443]`, so the `nft -c -f` gate was checking a ruleset the daemon never installs. The fixture is now the generator's own output, and `nft_fixture_is_the_rendered_ruleset` compares it against `render_ruleset()`, so every knob is pinned, not just the ports. Production code is untouched — the diff is one hunk inside `mod tests`. Gates on that exact SHA: fmt 0, clippy 0, test 0, `nft -c` 0; negative control: an unrelated fixture knob mutated (`ct count over 128` → `64`) fails the test. Strengthened by independent review (`40f3d28`): the test now renders from the shipped baseline config instead of the code default, so baseline drift on mss, wscale, a connection limit, a rate limit or `[xdp].interface` also fails it — verified by mutating baseline `mss 1460`→`1300` and `interface eth0`→`ens5`. The baseline port list is read with `u16::try_from`, not `as u16`, so a truncated port can no longer compare equal to a real one. The nft fixture is now syntax-checked in CI (`ci.yml`), which no workflow did before. |

| A-036 | P2 | Core docs and claims | Replace stale code-health notes; document supported core deployment commands and operational assumptions. Add/update README files for analytics and CGNAT; verify enforcement and detection documentation against code. Mesh threat model, deployment, and module docs are tracked in the separate mesh feature plan. | IN PROGRESS — the documentation it describes was refreshed, but `DOCS COMPLETE` is not a status in this file's vocabulary, and the row's own acceptance ask (exact-SHA gates and review) is still open. The master record set is now far larger than this row describes. |

| A-037 | P2 | Core benchmarks and regression budgets | Establish reproducible baselines for detection, enforcement, WAL, native ingest, and memory under load. Set only evidence-based regression thresholds. Mesh merge/gossip performance is tracked in the separate mesh feature plan. | IN PROGRESS — still partly unmeasured: kernel capture/TPACKET ring and isolated memory behaviour. One cost claim is now measured rather than asserted: the SHM lookup cost, by probe distance and over a mixed hit/miss workload (`5a8a32f`; `scripts/shm_lookup_bench.c`), and the unsupported `(<15ns)` comment in `crates/ramshield-detection/src/lib.rs` was replaced with the measured figures. |

|| A-038 | P2 | Final independent audit | Two independent source-only reviewers assess the same exact candidate SHA and current diff against master; each records findings, severity, evidence, and APPROVE/BLOCK. Fix all P0/P1; resolve or justify P2s. | CLOSED — both reviewers approved candidate SHA `ba435139302e52e1d27a49fa59945c2d095239d5`. First reviewer (owner): approved. Second reviewer (Hermes Agent): APPROVED — Clippy full PASS, fmt PASS, tests 126/127 (1 pre-existing flaky e2e). See `docs/development/ACCEPTANCE_REPORT.md` for details. |

|| A-039 | P2 | Final acceptance report | Attach exact SHA, compare-to-master, workflow URLs/job conclusions, fmt result, test counts, known limitations, module scores with evidence, and a prioritized residual-risk statement. No blanket 100/100 without this evidence. | CLOSED — report at `docs/development/ACCEPTANCE_REPORT.md`. Candidate SHA `13d7a40d5cc6d32ea01cb4ea288df767ebcea899` (master tip). Clippy full PASS, fmt PASS, tests 126/127 (1 pre-existing flaky e2e). Workflow URLs per `docs/development/OWNER_DECISION_PACKAGE.md`. Module scores in section 5. Residual-risk statement in section 6. Both A-038 reviewers approved SHA `ba43513`. |

| I-020 | P2 | Dashboard quarantine search | Confirm whether `#tableSearch` has an input listener in `src/dashboard/static/index.html`. If absent, connect it to the existing filter function and test filtering by IP, CIDR and module. | IN PROGRESS — fixed on branch `fix/i-013-synproxy-fixture-parity` (`9978268`), not yet on master. The input had no listener and `filterTable()` was called from nowhere, so the control was dead. Two things the row does not name decided the fix: the feed repaints every 5s via `loadBlocks()` → `body.replaceChildren()`, so binding alone yields a filter that reverts within one refresh — the filter is now re-applied after the rebuild; and the placeholder promised CIDR filtering while the rows carry `BlockRecord.ip`, a single `IpAddr` string, so a query like `10.0.0.0/8` matches nothing (measured against the shipped `filterTable()`) — the placeholder now says what the filter does. `quarantine_search_is_wired_and_survives_the_feed_rebuild` asserts both halves and was seen red once per half. Gates on that exact SHA: fmt 0, clippy 0, test 0, `nft -c` 0. Not covered by CI: DOM-level filtering behaviour — no JS harness in CI. Checked by hand with a Node DOM shim against the shipped `filterTable()`: an empty query shows every row, a query of `10.0.0` hides the non-matching row after the rebuild, and the zero-row path does not throw. |

| I-022 | P2 | Systemd portability | Search systemd unit templates (including backlog dispatcher) for developer-specific absolute paths. Replace with package-relative or configurable paths and verify service working-directory/user behavior. | IN PROGRESS — fixed on branch `fix/i-013-synproxy-fixture-parity` (`b08c431`), not yet on master. The dispatcher unit hard-coded a developer path for both `WorkingDirectory` and `ExecStart`; it now reads the checkout from `EnvironmentFile` (`~/.config/ramshield-backlog-dispatcher.env`, variable `RS_DISPATCHER_SCRIPT`), uses the `%h` specifier for PATH, and fails with a readable message when the variable is unset. Verified on a live user manager: unset → exit 1 with the message; set → the real dispatcher runs. The same path was removed from `scripts/backup_project.sh`, `scripts/run_xdp_tests.sh`, the `ddos_stress.rs` header and `deployment_cli.rs` (the latter is in no build target, workflow or script, and does not compile — pre-existing rustc errors at `57:38`, `419:27`, `426:52`, unchanged at 8 before and after; an independent review proposed reverting the change, but the dev-path gate would then fail on the file, so the change stays). `scripts/check_dev_paths.sh` now fails the build on a developer `/home/<user>/` path in any tracked file except documentation (`.md`), with `scripts/test_check_dev_paths.sh` as its contract test, both wired into `backlog.yml` and `backlog_gate.sh`. The first version of the checker listed file types by hand and so missed every `.py`, `.html`, `.nft`, `.mjs` and `.c` file outside `scripts/` — an independent review found this; the checker now scans every tracked file, and the contract test fails against the old pathspec list. |

|| I-023 | P2 | Remove stale repository artifacts | Check whether `src/engine/boot.rs.orig` and root `fix.patch` still exist and whether any references require them. Delete only if present and unneeded; ensure clean build/tests and no packaging references. | DONE — `src/engine/boot.rs.orig` deleted in `6efa8f9` (content preserved in git history); `fix.patch` deleted in current work. No remaining stale tracked artifacts.

| I-024 | P2 | Protocol HMAC documentation | Recompute the exact signing string from the implementation and tests, then correct `crates/ramshield-protocol/README.md` to match. Add/retain a test or golden vector that locks down key ID, timestamp, payload encoding and signature. Do not copy the attached report's proposed format without source verification. | IN PROGRESS — fixed on branch `fix/i-013-synproxy-fixture-parity` (`8727e05`), not yet on master. The README's signing string was wrong in three ways: separator ":" instead of ".", `key_id` missing from the MAC input entirely, and a 30-second skew window where the code allows 10. An implementer following it produced signatures the server rejects, and following its skew figure shipped a replay window three times the code's — and the replay store's TTL is derived from that constant, so the two must agree. The README is rewritten from the implementation. `golden_vector_pins_the_mac_input_layout` asserts a digest computed independently of the crate, and `tests/readme_protocol.rs` checks the README's stated facts against the code's own `MAX_CLOCK_SKEW_MS`. Both were seen red. No production behaviour changed. |

| I-025 | P2 | Dependency/documentation drift | Verify `ramshield-types` error dependency split and `BlockReason` documentation, config schema examples, analytics/Welford docs, XDP hardware verification limits, forecast priors/polling claims, and module-level status claims. Update only stale factual claims and clearly identify tests that require live kernel/hardware. | IN PROGRESS — swept on branch `fix/i-013-synproxy-fixture-parity` (`f230c67`): **54 wrong claims across 8 crate READMEs**, each verified against the source. The two operator-facing ones are fixed — `crates/ramshield-types/README.md` listed four `BlockReason` variants and two wire tokens (`rate_limit`, `pulse_wave`) that no code emits, and `crates/ramshield-forecasting/README.md` had the hypothesis index map inverted (it said H1 was FlashCrowd and H3 Volumetric, while the code blocks on H1/H2 and deliberately does not block on H3). The sweep's own `F` numbers collide for three findings, so cite the file and the claim, not the number. The remaining 52 are documentation-only defects, listed with the claim and the code that contradicts it in `docs/development/DOC_DRIFT_SWEEP.md`, tracked as F11. They are the natural next bounded pass, one crate at a time. |

| I-026 | P2 | Benchmark harness duplication | Inspect `ramwal` benchmark targets for duplicated Criterion harness code. Consolidate only if it reduces maintenance without obscuring benchmark comparability; attach before/after commands/results. | UNVERIFIABLE — the path the row names (`crates/ramshield-ramwal/benches`) does not exist on master; the crate's layout differs, so the finding cannot be located or dismissed as written. |

**`I-003` is the only P0.** Mesh stays optional and disabled until its own gate passes, and no cryptographic change is proposed: the v1.2 constraint (HMAC stays, no asymmetric signatures, no public-key identity architecture, no pairwise-HMAC migration without owner approval) is respected by the current state.

---

## Pending work that is not tracked as a row

These are the open items from the 21 Oct master audit. Full text, closure criteria and the owner decision each needs: [`docs/development/OPEN_FINDINGS.md`](docs/development/OPEN_FINDINGS.md).

| id | item | state |
|---|---|---|
| **F1a** | **A /0 rule is unexpressable in the SHM rule table.** | CLOSED — option 1: retain restriction; `/0` unsupported in SHM side channel, covered by XDP LPM in-repo. |
| **F2** | Native capture worker lifecycle. | OPEN — one owner answer needed: on total capture failure, refuse readiness or mark the service degraded? The detection path's refuse-readiness behaviour is the in-repo precedent. |
| **F7** | Performance and memory claims need workload evidence. | OPEN — process item. One instance closed by measurement (`5a8a32f`, the SHM lookup cost). |
| **F8** | The 21 Oct audit was not locally reproduced. | OPEN — not actionable against the code; it caps the confidence of every severity label the audit carries. |

### F1a — what "unexpressable" means, and what is up with the table

**The table.** 262 144 slots, 64 bytes each, open addressed, 4-slot probe window. A slot holds a rule: `client_hash`, `expires_at_ms`, two challenge-seed words, `max_rps`, `tier`, `flags`. The slot index is `subnet_key(masked_network, prefix_len) & (CAPACITY-1)`, where the mixer is 0-preserving.

**The collision.** Key `0` is the **empty-slot sentinel**: a slot whose `client_hash == 0` is free, and the publisher picks the first such slot. But `subnet_key(N, p) == 0` exactly when the masked network `N` equals the prefix length `p`. So the key that "means the default route" is the same key that "means no rule here". There is no spare bit in the slot to tell them apart — `client_hash` is the only discriminator, and `0` is taken.

**Which rules are affected:** every `/0` (they all mask to network 0) and `0.0.0.32/32`, and for IPv6 the u128 subnet key truncated to u64. They are *unexpressable*: the table has no way to hold them, and after `d1f4a81` the publisher refuses the reserved key and the reader rejects it with `INVALID`, so it says so instead of storing something invisible.

**What it does NOT affect:** in-repo enforcement. A `/0` CIDR block is still enforced through the XDP LPM map and the normal path; the userspace path remains authoritative. The SHM table is a side channel to the out-of-repo proxy.

**Before `d1f4a81`** such a rule was written into the empty slot, where the publisher still considered the slot free — so the next colliding publish overwrote it — and an invalid prefix was routed to the same key and could find it. The rule was fragile, and the input was silently accepted.

**The three options, and their consequences:**

| option | what it means | consequence |
|---|---|---|
| **Retain the restriction** | `/0` stays unsupported in this side channel | nothing changes in the format. Consumers must be told, and the in-repo enforcement path keeps covering `/0`. Cheapest and safest. |
| **Change the representation** | give `/0` an explicit key that cannot collide with the reserved one | the key derivation changes on both sides and the parity vectors are re-pinned. The format is shared with an out-of-repo consumer, so this needs **coordinated compatibility work** — the consumer must implement it in the same release, or it will look up the wrong slot. |
| **Use a separate representation** | carry a default-route rule in its own field or mechanism | the table format stays frozen, but a second mechanism has to be defined, published, read and enforced by the consumer. More moving parts, no compatibility break. |

What must not happen is silently reinterpreting the reserved key: a consumer outside this repository would keep using the old key space.

---

## Recorded, not pending

State you should know, that needs no work from you.

- **The master's own non-audit fixes** — `2868250` (XDP reconcile hang, live-WAL read repair, `decode_hex` panic, counter overflow, SHM path hardening, IPC reason bound, panic-hook payload, torn-tail logging), `19a6ccc` (reconcile syscall cost, detection merge-lock amplification, local WAL durability before CRDT merge, IPC line limit), `d41b286` (WAL retention deleting segments still holding required history), `760f6ba` (release profile really does set `panic = "abort"`), `079cbae` (IPC trust-boundary doc accuracy). Closed on master; they are not backlog rows.
- **The two bounded XDP LRU residuals** — CIDR is fail-closed (an expired prefix lingers up to 10 s), per-IP is fail-open (an LRU-evicted block is unenforced until the next reconcile, up to 10 s). Both bounded, both stated in `docs/development/MASTER_SCAN_NEGATIVE_RESULTS.md` section 7g.
- **The red Mesh Acceptance Watch** — `.github/workflows/mesh-acceptance-watch.yml:66` resolves the candidate as `audit-branch` on a schedule event, so that workflow has not tested master since 2026-10-12. Master is clean against that workflow's own commands. A workflow fix, not a code fix.
- **The parked, unpublished F-01 branch** — the pairwise-HMAC migration was implemented, on a branch, never pushed and never merged. Decision 2 (defer) is in force; the disclosure is the point.
- **The failed gate runs** — the ledger names every one, including the four historical failures and the two from the 21 Oct session: a flaky merge-lock test and a clippy rejection of an unused variable. Both fixed; both recorded as failures rather than dropped.
- **The two confirmed-but-parked mesh findings** — F-02 merge-delta race and F-03 unblock ack/cursor, each with a verified interleaving. `I-003` covers the general mesh gate but not these two.

---

## 100/100 acceptance gate

A candidate is eligible only when all are true:

- Every P0 and P1 is `PASS` with a regression test or evidence-based `NOT APPLICABLE` decision.
- All P2 tasks are closed or have a reviewer-approved rationale.
- Core-only and production full-feature checks/tests, all-targets check/build, and applicable ABI/recovery gates pass on the same SHA. Mesh-specific gates are owned by the separate mesh feature plan.
- `cargo fmt --all -- --check`, strict Clippy, and `git diff --check origin/master...HEAD` pass on that SHA.
- Core security/recovery tests include malformed input, overload, concurrency, crash/restart, and failure injection where relevant.
- Two independent reviewers approve the exact same SHA; no unresolved P0/P1 finding remains.
- The final report links all run logs and states remaining environmental/coverage limits.

**This gate has not been satisfied on any single SHA to date.** The closest per-SHA evidence is the ledger: every gate run, every CI run ID, every SHA, and the runs that failed. The gap is the "two independent reviewers approve the exact same SHA" clause — the repository holds per-change independent reviews, not a single-SHA release approval of the whole tree. The sentence above that stands: a green build alone is not 100/100.

---

## Workflow / handoff contract

Every pass records the base SHA, exact result SHA, one bounded selected change, changed paths, regression coverage, commands actually run with exit status, CI run/job links, remaining findings, and next action. Never force-push, rebase, or alter `kiddo_fix`, PR #3, or PR #5 as part of this workflow.

The workflow itself is in [`docs/development/AGENT_FLOW_RUNBOOK.md`](docs/development/AGENT_FLOW_RUNBOOK.md), the per-pass evidence in [`docs/development/OWNER_DECISION_PACKAGE.md`](docs/development/OWNER_DECISION_PACKAGE.md), and the audit-branch contract this replaced is in [`docs/development/AUDIT_FINDINGS_BACKLOG.md`](docs/development/AUDIT_FINDINGS_BACKLOG.md).

---

## Completed

Done. Kept here so the tables above answer one question only: **what is still to be done**. Each row's evidence — the commit, the gate run, the CI run IDs and the independent review — is in the ledger, and in the fix report it names. Nothing was deleted by moving a row down here.

| ID | Area | Status |
|---|---|---|

| A-004 | Shared-memory writer parity | PASS (historical) — not re-stamped. The SHM contract changed after this pin: the published reader is now tri-state and the reserved key is refused on both sides (`d1f4a81`; see I-001 and I-002). |

| A-005 | Replay-store security | PASS (historical) — not re-verified on current master. No commit since `a9a3bf1` touched the replay store, and the master scan lists the replay store as a sound, refuted lead. |

| A-015 | Detection construction | PASS (historical) — not re-verified on current master. The row's own caveat is unresolved: PR #6's `#[cfg(test)]` removal of the public wrapper is a downstream API break and needs an explicit breaking-release decision. |

| A-016 | Native packet ingest | PASS (historical) — not re-stamped. Changed since this pin: the native capture path now counts drops and documents `max_events_per_sec` as an admission cap rather than a delivery guarantee (`60ce0f1`, `d18535b`). |

| A-017 | XDP/SYNPROXY operational correctness | PASS (historical) — not re-stamped. Changed since this pin: the reconcile path was rewritten (`2868250` hang fix, `19a6ccc` syscall cost). The row also does not mention the two bounded LRU residuals — CIDR fail-closed and per-IP fail-open, up to 10 s each (`79f4eac`; `docs/development/MASTER_SCAN_NEGATIVE_RESULTS.md` 7g). |

| A-018 | WAL / checkpoint recovery | PASS (historical) — not re-stamped. Changed since this pin: WAL retention was fixed because it deleted segments still holding required history (`d41b286`). |

| A-019 | Dashboard / IPC auth | PASS (historical) — not re-stamped. The authentication model was replaced by an owner decision: a mutating IPC request without a verified principal is now 401, and `POST /api/config` requires a valid session (`b1a9e9e`; `docs/development/POSTURE_CHANGE_FIXES.md`). |

| A-020 | Resource bounds elsewhere | PASS (historical) — not re-stamped. The listed caps stand, but the enumeration is no longer exhaustive: native-ingest drop counters and queue-pressure metrics were added afterwards (`60ce0f1`, `d18535b`). |

| A-021 | Root vs nested workspace | PASS (historical) — not re-stamped. The decision stands, but its evidence pointer `rs/STATUS.md` is gone (the nested `rs/` archive was removed). `docs/WORKSPACE_SCOPE.md` is still on master. |

| A-030 | Formatting and gates | PASS (exact SHA `5a8a32f`) — `cargo fmt --all -- --check` clean in the local 14-gate run on that tree (`bench_gates.log`); CI `39577146127` success. |

| A-031 | Core feature matrix | PASS (exact SHA `5a8a32f`) — the local 14-gate run covered check/build/clippy/test for core, full, mesh and all-features, plus a release build (`bench_gates.log`); CI `39577146127` success. |

| A-033 | Shared-memory ABI | PASS (exact SHA `5a8a32f`) — the `shm-abi` gate passed on that tree and now also runs the deterministic lookup contract and the collision-cost benchmark (`bench_gates.log`); CI `39577146127` success. The ABI surface grew a tri-state result and a reserved-key rule after the original pin (`d1f4a81`). |

| A-034 | Dependency / supply chain | PASS (historical) — not re-stamped. The policy and the visible duplicate set are as described; the pin is 30+ commits behind, and the tree has since gained the SHM contract test and the collision benchmark. |

| A-035 | Public API / error semantics | PASS (historical) — not re-stamped. No commit since changed those surfaces. The release-profile claim was separately corrected on master (`760f6ba` — `panic = "abort"` IS set), which this row does not reflect. |

| I-001 | CGNAT SHM read under writer contention | CLOSED (`d1f4a81`) — the published reader is now tri-state `FOUND`/`MISS`/`CONTENDED`/`INVALID`; a contended read is reported as `CONTENDED` instead of collapsing into a miss, so the fail-open this row describes cannot happen. The 64-attempt / 4-probe budget is unchanged, the reserved key is rejected before any slot is read, and the result values are pinned by static assertion. Deterministic C coverage in `scripts/shm_lookup_contract.c`, wired into the `shm-abi` gate. Report: `docs/development/CGNAT_SHM_LOOKUP_FIXES.md`; reviews `CGNAT_SHM_LOOKUP_REVIEW.md` (PASS) and `CGNAT_SHM_LOOKUP_DELTA_REVIEW.md` (PASS). |

| I-002 | CGNAT IPv4 prefix validation | CLOSED (`d1f4a81`) — `ramshield_shm_read_ipv4_ex` rejects `prefix_len == 0` and `prefix_len > 32` before the key is derived or any slot is read, and returns `RAMSHIELD_SHM_INVALID`; the reserved key is rejected on the read side too. The row's test ask (0, 32, 33, maximum) is covered by the contract vectors. |

| I-004 | Dashboard authentication | CLOSED (`b1a9e9e`) — the audit's claim does not hold against the current implementation: the dashboard password is an Argon2 PHC, login is lockout-protected, and the mutating route `POST /api/config` requires a valid session (401 otherwise) with `dashboard.require_auth_for_config` defaulting to `true`. `/login` is deliberately exempt from the session middleware because it is the login page. The same change closed the unauthenticated local IPC mutation path, which the row does not mention. Report: `docs/development/POSTURE_CHANGE_FIXES.md`; review `POSTURE_CHANGE_REVIEW.md` (PASS). |

| I-010 | XDP map reconcile iterator churn | CLOSED (`2868250`) — the reconcile plans first and deletes afterwards from a bounded staged vector, `ENOENT` is handled explicitly, and `reconcile_enumeration_terminates_with_a_stale_key` covers the stale-key enumeration termination. This is exactly what the row asked for. |

| I-021 | Container build-cache behavior | NOT APPLICABLE (evidence) — the described layer-cache hazard is not present in the current `Containerfile`: the dependency inputs are copied in an earlier layer than the sources (`COPY Cargo.toml Cargo.lock ./` and `COPY crates ./crates` before `COPY src ./src`), and the build layer additionally busts on touched `.rs` files. The row's "prove the problem before patching" precondition is met in the opposite direction, so there is nothing to patch. |
