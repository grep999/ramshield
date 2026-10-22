# RamShield Production-Essential Audit Backlog

**Target branch:** `master` · **Assessed at:** `2adc966` · **master HEAD at assessment:** `1be1318` · **master HEAD now:** `f1eacf9`

`2adc966` is the parent of `bd417cd`; the rows below were assessed
against `2adc966`, and `bd417cd` rebuilt this file's tables without
changing the code. The rows whose fixes have since landed on `master`
(`f900664`, PR #37) say so in their own status column.

**CI on `2adc966`:** CI 39587482510 · Review Pipeline 39587482567 · lint-no-unwrap 39587482557 · Subnet Stress Pin 39587482512 — all success. Local 14-gate run on the same tree: 14/14 EXIT=0, 1858 passed / 0 failed / 42 ignored.

**CI on `bd417cd`:** CI 39589296986 · Review Pipeline 39589297074 · lint-no-unwrap 39589297157 · Subnet Stress Pin 39589297129 · Agent Flow Contract 39617102370 — all success.

**CI on `35f4557`:** CI 39714123651 · Review Pipeline 39714123719 · lint-no-unwrap 39714123629 · Subnet Stress Pin 39714123723 — all success.

**CI on `f1eacf9`:** CI 39724446000 · Review Pipeline 39724445978 · lint-no-unwrap 39724445986 · Subnet Stress Pin 39724446022 — all success.

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

| I-003 | P0 | Mesh production safety and provenance | Keep mesh optional and disabled in production. Do not ship/enable until authenticated per-origin signatures/identity, receiver-confirmed recovery completion (generation + WAL boundary + deterministic state digest), restart/retention behavior, and multi-process fault-injection convergence gates pass on the same exact SHA with independent review. | BLOCKED — still correct, and the gate is still unmet. The pairwise-HMAC branch `feature/mesh-f01-hmac-edge-secrets` is unpublished and not merged: no mesh transport or aworset change from it is on master. Mesh stays optional and disabled in production. No cryptographic change is proposed here — the v1.2 constraint (HMAC stays, no asymmetric signatures, no public-key identity, no pairwise-HMAC migration without owner approval) is respected by the current state. |

| I-011 | P1 | Storage eviction vs threat accumulators | Verify cardinality-driven LRU eviction cannot repeatedly erase the attacker's own CUSUM/pulse-wave history and create a threshold bypass. Define bounded behavior under capacity pressure; add adversarial rotating-IP tests before changing policy. | CLOSED (`682435a`, PR #43) — the row's premise does not hold: there is no cardinality-driven LRU eviction of the threat accumulators. The store evicts only on TTL expiry and refuses net-new keys under RAM pressure (`crates/ramshield-storage/src/store/capacity.rs`, `store/write.rs`), and the per-IP pulse observations are capped in place (`crates/ramshield-detection/src/rate_tracker.rs:291`). The adjacent real defect found while verifying the row — enforcement's block path rolling the detection counters back — was fixed in `682435a` with deterministic regression tests (F9). |

| I-012 | P1 | Host-network Kubernetes capability review | Review `deploy/k8s/daemonset.yaml` capabilities against the exact XDP/capture operations required. Minimize capabilities and document why each retained capability is required; validate manifests and deployment smoke tests. Do not remove capabilities blindly if it breaks enforcement. | OPEN — verified still present: `deploy/k8s/daemonset.yaml` still runs with `hostNetwork: true` and a `NET_ADMIN` capability, and nothing in the master records documents why each retained capability is required. The row's own acceptance ask is unmet. |

| I-013 | P1 | SYNPROXY fixture parity | Compare `tests/fixtures/synproxy.nft` with `config.baseline.toml` and actual generator behavior; align ports and prove port 80 coverage if discrepancy remains. | CLOSED — on `master` as `f900664` (PR #37). The fixture was a hand-maintained copy that had drifted from the ruleset the daemon installs: it declared `tcp dport { 22, 443 }` while the shipped baseline and the code default both configure `[22, 53, 80, 443]`, so the `nft -c -f` gate was proving syntax for a ruleset production never loads. The fixture is now the generator's own output, compared knob by knob against the shipped baseline config, and a port value that is not a TCP port fails loudly instead of being truncated. The check now runs in CI, in the `nft-fixture` job, though the nft syntax step self-skips on runners without the `nft_synproxy` module (`.github/workflows/ci.yml:176-177`). |

| A-036 | P2 | Core docs and claims | Replace stale code-health notes; document supported core deployment commands and operational assumptions. Add/update README files for analytics and CGNAT; verify enforcement and detection documentation against code. Mesh threat model, deployment, and module docs are tracked in the separate mesh feature plan. | IN PROGRESS — the documentation it describes was refreshed, but `DOCS COMPLETE` is not a status in this file's vocabulary, and the row's own acceptance ask (exact-SHA gates and review) is still open. The master record set is now far larger than this row describes. |

| A-037 | P2 | Core benchmarks and regression budgets | Establish reproducible baselines for detection, enforcement, WAL, native ingest, and memory under load. Set only evidence-based regression thresholds. Mesh merge/gossip performance is tracked in the separate mesh feature plan. | IN PROGRESS — still partly unmeasured: kernel capture/TPACKET ring and isolated memory behaviour. One cost claim is now measured rather than asserted: the SHM lookup cost, by probe distance and over a mixed hit/miss workload (`5a8a32f`; `scripts/shm_lookup_bench.c`), and the unsupported `(<15ns)` comment in `crates/ramshield-detection/src/lib.rs` was replaced with the measured figures. |

| A-038 | P2 | Final independent audit | Two independent source-only reviewers assess the same exact candidate SHA and current diff against master; each records findings, severity, evidence, and APPROVE/BLOCK. Fix all P0/P1; resolve or justify P2s. | OPEN — the gate this row describes (two independent reviewers on one exact SHA) has not been met on any current SHA. Per-change independent reviews exist — `CGNAT_SHM_LOOKUP_REVIEW.md` and `CGNAT_SHM_LOOKUP_DELTA_REVIEW.md` (F1), `POSTURE_CHANGE_REVIEW.md` (F5), `NATIVE_INGEST_DROPS_REVIEW.md` (F3), `MASTER_AUDIT_2026_10_21_REVIEW.md` (the 21 Oct scan) — but that is per-change review, not a single-SHA release approval of the whole tree. |

| A-039 | P2 | Final acceptance report | Attach exact SHA, compare-to-master, workflow URLs/job conclusions, fmt result, test counts, known limitations, module scores with evidence, and a prioritized residual-risk statement. No blanket 100/100 without this evidence. | IN PROGRESS — the acceptance evidence now exists in a newer, per-SHA form: the gate ledger and CI run table in `docs/development/OWNER_DECISION_PACKAGE.md`. It is not attached here as a single final report. |

| I-020 | P2 | Dashboard quarantine search | Confirm whether `#tableSearch` has an input listener in `src/dashboard/static/index.html`. If absent, connect it to the existing filter function and test filtering by IP, CIDR and module. | CLOSED — on `master` as `f900664` (PR #37). The input had no event binding, and the 10-second row rebuild destroyed any filter state; the input now calls `filterTable()` on input and the rebuild re-applies the filter. The placeholder said CIDR, which the filter does not do: block rows carry a plain `IpAddr`, so a CIDR query matches zero rows. |

| I-022 | P2 | Systemd portability | Search systemd unit templates (including backlog dispatcher) for developer-specific absolute paths. Replace with package-relative or configurable paths and verify service working-directory/user behavior. | CLOSED — on `master` as `f900664` (PR #37). The dispatcher unit hard-coded a developer path for both `WorkingDirectory` and `ExecStart`; it now reads the checkout from `EnvironmentFile` (`RS_DISPATCHER_SCRIPT`), uses `%h` for PATH, and fails with a readable message when the variable is unset. Verified on a live user manager: unset exits 1 with the message; a set value whose path contains a space reaches ExecStart as one argument. The same path was removed from `scripts/backup_project.sh`, `scripts/run_xdp_tests.sh`, the `ddos_stress.rs` header and `deployment_cli.rs`. |

| I-023 | P2 | Remove stale repository artifacts | Check whether `src/engine/boot.rs.orig` and root `fix.patch` still exist and whether any references require them. Delete only if present and unneeded; ensure clean build/tests and no packaging references. | CLOSED — on `master`. `src/engine/boot.rs.orig` was deleted earlier, after its content was proven to live in git history (`6efa8f9`). The root `fix.patch` is deleted here: it is a 2400-byte leftover working artifact whose content is a patch that would REMOVE `crates/ramshield-forecasting` from the workspace and remove the `ramshield-cli` binary, plus a stray heredoc fragment. Nothing references it, and no packaging file (`Containerfile`, `Makefile`, `.dockerignore`) mentions it. The two stray compiled binaries at the repository root (`validate_mesh_fix`, `verify_boot_fix`, 4.4 MB each, committed by accident in `415f75f`) were removed as F12 and both names are in `.gitignore`. |

| I-024 | P2 | Protocol HMAC documentation | Recompute the exact signing string from the implementation and tests, then correct `crates/ramshield-protocol/README.md` to match. Add/retain a test or golden vector that locks down key ID, timestamp, payload encoding and signature. Do not copy the attached report's proposed format without source verification. | CLOSED — on `master` as `f900664` (PR #37). The README documented a signing string the code never used (`{ts_ms}:{payload}`) and a 30-second skew window where the code has 10. It now states the real rule — the server MACs a key-sorted re-serialisation, not the client's bytes — with a golden vector that is independently reproducible and a guard test that fails if the wording drifts back. Which rule to ENFORCE is still the owner's decision, tracked as F10; the documentation now matches the code. |

| I-025 | P2 | Dependency/documentation drift | Verify `ramshield-types` error dependency split and `BlockReason` documentation, config schema examples, analytics/Welford docs, XDP hardware verification limits, forecast priors/polling claims, and module-level status claims. Update only stale factual claims and clearly identify tests that require live kernel/hardware. | CLOSED (`f900664`, PR #37, and the follow-up sweep) — the sweep recorded 54 wrong claims across 8 crate READMEs and left the rest tracked as F11. F11 is now fixed: all 62 claims are fixed, and every crate README the sweep covers is pinned by a test (`crates/ramshield-metrics/tests/readme_claims.rs` and the same guard in the other six crates). |

| I-026 | P2 | Benchmark harness duplication | Inspect `ramwal` benchmark targets for duplicated Criterion harness code. Consolidate only if it reduces maintenance without obscuring benchmark comparability; attach before/after commands/results. | UNVERIFIABLE — the path the row names (`crates/ramshield-ramwal/benches`) does not exist on master; the crate's layout differs, so the finding cannot be located or dismissed as written. |

| I-027 | P1 | Kubernetes node-guard DaemonSet is refused at admission | The node-guard DaemonSet sets `mountPropagation: Bidirectional` on `/sys/fs/bpf` while `securityContext.privileged` is not set. The Kubernetes API server rejects that combination, so the DaemonSet never starts and the XDP profile cannot be deployed. | CLOSED — the owner chose to drop the Bidirectional propagation rather than ask for a privileged container: `deploy/k8s/daemonset.yaml` now mounts the host's `/sys/fs/bpf` with `HostToContainer` The upstream validation checks the `privileged` field alone, so adding capabilities does not pass it. Nothing in the repo applies the manifest, and `kubectl apply --dry-run=client` does not run server-side validation, so it cannot be caught that way. Fix: `privileged: true` (which needs `--allow-privileged` on the cluster) or drop the propagation to `HostToContainer`. This is a distinct defect from I-012, which is the capability-minimisation review. |

| I-028 | P2 | The Ansible quality gate reports a failed CI run as a pass | The `quality` role's poll script exits 0 as soon as a workflow run is `completed`, whatever the conclusion. `ci_failed` is then derived only from that exit code, so a `failure` conclusion is printed and then ignored. | CLOSED (`48f2f33`, PR #48) — the role now compares the run's conclusion with `success` and exits 1 otherwise, so a failed CI run is no longer reported as a pass. |

| I-029 | P2 | The dashboard quarantine table and its CSV export disagree on the columns | The table declares 5 headers and injects 6 cells per row (the sixth is a trailing `—`), and the CSV export declares 5 columns while slicing 4, so every exported row silently drops the Time value. | CLOSED (`48f2f33`, PR #48) — the surplus cell is gone and the CSV export takes all five columns; a test pins the header count against the row count. |

| I-030 | P3 | `attack_nexus.py`'s interactive shell cannot stop a running flood | `start_flood` creates a local `StopFlag` and passes it to the flood thread, but the `stop` command signals the outer flag, which nothing polls — so the flood runs to its full duration. | CLOSED (`48f2f33`, PR #48) — the flood polls the flag that the `stop` command sets. |

| I-031 | P3 | The integration runners read a config example that no longer exists | `scripts/auth_e2e.py` and `scripts/final_integration.py` read `config.prod.toml.example`, which was deleted in `af3f465`, so both crash with `FileNotFoundError`. | CLOSED (`1416a60`, PR #49) — the dead read is gone and the other runner reads the shipped baseline. |

| I-032 | P3 | `network_create.py`'s `dual` family produces IPv4 only | The `family != "v6"` test sends the `dual` case down the IPv4 branch, so no IPv6 network is generated. | CLOSED (`1416a60`, PR #49) — `build_plan` now expands `dual` into a /24 and a /64 per index. One gap remains and is not the row's subject: the CLI still builds v4 only (`scripts/network_create.py:72` calls `build_plan(args.count, "v4", 24, 32, 7)`), so `dual` is reachable only through the module. |

| I-033 | P3 | `xdp_netns_sim.py` ignores `--dashboard` in its pre-flight check | The pre-flight snapshot request is hardcoded to `127.0.0.1:9999`, so a run against another dashboard address fails with a traceback instead of the intended message. | CLOSED (`1416a60`, PR #49) — the pre-flight check uses the parsed dashboard URL. |

| I-034 | P2 | The forecasting subsystem is never constructed by the daemon | The `Forecaster` is implemented and unit-tested, but `src/engine/boot.rs` never constructs it, so the production daemon produces no forecast-driven mitigation. | FIXED — `src/engine/boot.rs` constructs the `Forecaster` and runs it when `forecasting.enabled` is set, so the Holt-Winters forecast, the CUSUM drift detector, the Bayesian hypothesis tracker and the entropy signal run in production. `forecasting.min_entropy` is read: below it the traffic is concentrated, so an entropy drop is reported as an anomaly rather than a flash crowd. |

**`I-003` is the only P0.** Mesh stays optional and disabled until its own gate passes, and no cryptographic change is proposed: the v1.2 constraint (HMAC stays, no asymmetric signatures, no public-key identity architecture, no pairwise-HMAC migration without owner approval) is respected by the current state.

---

## Pending work that is not tracked as a row

These are the items tracked as findings rather than as rows: the open items from the 21 Oct master audit, and the ones found since. Full text, closure criteria and the owner decision each needs: [`docs/development/OPEN_FINDINGS.md`](docs/development/OPEN_FINDINGS.md).

| id | item | state |
|---|---|---|
| **F1a** | **A /0 rule is unexpressable in the SHM rule table.** | OPEN — owner decision. See the note below. |
| **F2** | Native capture worker lifecycle. | CLOSED — on `master` as `15e0551` (PR #58). The owner decided the policy half: degrade, keep serving. A dead capture shows as `Degraded` with the health reason `native capture stopped`, and the four `ramshield_native_capture_*` series say what happened. |
| **F7** | Performance and memory claims need workload evidence. | OPEN — process item. One instance closed by measurement (`5a8a32f`, the SHM lookup cost). |
| **F8** | The 21 Oct audit was not locally reproduced. | OPEN — not actionable against the code; it caps the confidence of every severity label the audit carries. |
| **F9** | Enforcement's block path is a lost update against detection. | CLOSED — on `master` as `682435a` (PR #43). |
| **F10** | The IPC server MACs a key-sorted payload, not the client's bytes. | OPEN — the wire-contract decision is the owner's. `scripts/auth_e2e.py` is the client-side face of it. |
| **F11** | Crate README documentation drift. | CLOSED — all 62 claims are fixed; every crate README the sweep covers is pinned by a test. |
| **F23** | `forecasting.min_entropy` is a config knob nothing reads. | OPEN — the config crate validates it and the forecasting crate never reads it. |
| **F25** | `ramshield-protocol` declares a dependency on `ramshield-storage` it never uses. | FIXED — the dependency is out of `crates/ramshield-protocol/Cargo.toml`; the crate declares only `ramshield-types`. |
| **F26** | `FAIL_SAFE_BEHAVIOR.md` claims a global fail-closed posture the dataplane does not have. | FIXED — the document states each subsystem's own posture, and `tests/fail_safe_claims.rs` pins it to the source. |
| **F27** | `scripts/prod_smoke.sh` claims checks it does not perform, and it deletes an env-overridable path. | OPEN — the unblock, CIDR and TTL checks are advertised and absent; `rm -rf "$WAL_DIR"` runs on a path the environment can override. |
| **F28** | The advanced CI workflow runs the benchmarks as tests and masks the benchmark build failure. | OPEN — `cargo test --all-targets` and `cargo bench --no-run || true`. |
| **F29** | The Subnet Stress Pin gate asserts against a fixed four-second sleep. | FIXED — the script waits for the counters to settle. The run on `4b88864` was red on a docs-only commit (206 IPs tracked against a threshold of 800). |
| **F30** | `block_ip` trusts the caller to pass a blocked fallback; the only guard is a `debug_assert!`. | OPEN — the release build publishes whatever `block_state` the fallback carries, and a clean fallback would put the key in the blocked index while the record says clean. The only production caller passes the right thing. |
**The 2026-10-22 full-codebase audit** (8 areas, read-only) added 20 findings, F-31..F-50, all in `docs/development/OPEN_FINDINGS.md`. The ones that change what an operator sees or what a deployment does:

| id | finding | severity |
|---|---|---|
| F-31 | the dashboard's rate readouts read keys the SSE writer never sets, so every rate reads 0 | P2 |
| F-32 | the dashboard's auth counters go to a Metrics instance that is never exported | P1 |
| F-33 | the shipped server ConfigMap's `ipc.max_connections` exceeds the validated limit, so the pod refuses to start | P1 |
| F-34 | the Kubernetes probes target the pod IP while the daemon binds loopback | P1 |
| F-35 | `ramshield-cli` is a five-line stub but the release ships it, the installer requires it and the docs document its commands | P1 |
| F-36 | the 403 rejections are counted but never exported | P2 |
| F-37 | the store-RAM refusals are counted but no surface exposes them | P2 |
| F-38 | the XDP-divergence stale flag is erased 250 ms after it is raised | P2 |
| F-39 | the "Drops vs Ingress" chart plots a literal 0 for drops | P2 |
| F-40 | the blocklist gauge compares a lifetime counter with a hardcoded capacity | P2 |
| F-41 | two dashboard rows are overwritten with `n/a` by the 100 ms SSE path | P2 |
| F-42 | the release gate greps for a dependency-audit flag the workflow no longer uses | P2 |
| F-43 | the installer writes a unit that grants the daemon `/proc/sys` write access it must never have | P2 |
| F-44 | the Ansible deploy role renders into a directory that does not exist and applies a 22-line stub template | P2 |
| F-45 | two exported series miss the `ramshield_` namespace | P3 |
| F-46 | the pipeline stage bars mix units | P3 |
| F-47 | the quarantine table fabricates the severity and tier of every row | P3 |
| F-48 | `/metrics` bypasses the session auth | P3 |
| F-49 | the backlog gate's open-row parser never matches `BACKLOG.md` | P3 |
| F-50 | three scheduled workflows are gated on a branch that does not exist | P3 |


**The 2026-10-22 master audit** (`35f4557`) is in this table already, item
by item: the node-guard admission blocker is **I-027**/F15, the IPC
signing contract is **F10**, the capture lifecycle is **F2**, the `/0`
representation is **F1a**, the performance claims are **F7**, the unused
knob and the unused dependency are **F23** and **F25**, and the historical
header metadata is this file's header. Its three new items are **F26**,
**F27** and **F28** above. The audit's own note that the ansible gate and
the dashboard columns are already fixed matches the rows below.

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

| A-015 | Detection construction | PASS (historical) — not re-verified on current master. The caveat as written was inaccurate: PR #6's `#[cfg(test)]` removal of the public wrapper never landed. Master keeps `pub fn new` as a deprecated wrapper and adds `try_new` / `try_new_with_shm_path` (`crates/ramshield-detection/src/lib.rs:162-202`), so there is no downstream API break. |

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
