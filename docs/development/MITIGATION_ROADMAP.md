# Mitigation roadmap

Scope: uncommitted worktree at `chore/untracked-cleanup`, HEAD
`42ab886543efa30c0b6581b5b904b91aeb83bdd1`. This is a sequencing document,
not a backlog row and not an approval to enable BGP or mesh.

## 1. Repository reality

- Upstream `origin/chore/untracked-cleanup` in sync: `git rev-list --left-right --count HEAD...@{u}` = `0 0`.
- One commit ahead of `origin/master`: `42ab8865`.
- No staged changes.
- 22 tracked files changed: `+628 -1000`.
- 1 untracked file: `examples/attack_demo.rs` (5,478 bytes).
- No production daemon code is wired to BGP. Only `Cargo.toml:16` references it.

## 2. Change slices

| Slice | Files | Lines | Owner | Production impact |
|---|---|---:|---|---|
| A — audit/docs/tests | `BACKLOG.md`, `docs/development/OPEN_FINDINGS.md`, `tests/ci_gates.rs`, `tests/k8s_manifests_admissible.rs`, 3 `feat/*.md` | `+149 -47` | this session | none |
| B — IPC demo override | `src/ipc/server.rs` | `+7 -1` | earlier demo work | conditional |
| C — BGP WIP | `Cargo.toml`, `Cargo.lock`, `crates/ramshield-bgp/*` | `+472 -952` | user WIP | none today; crate compiled only |
| D — demo example | `examples/attack_demo.rs` | 5.5 KB | earlier demo work | none; breaks fmt gate |

Slice A closes stale rows with SHAs verified on `origin/master`: `5a252398`,
`5a20439c`, `d1f4a81`, `9f7b1721`, `fc6b6fcb`, `15925197`, `754f20df`.
It also records the hosted-gate failure at exact SHA `42ab8865` and the
16-line whitespace fix for the 3 committed design notes.

## 3. Whole-codebase impact

Evidence from the current dirty tree:

| Check | Command | Result |
|---|---|---|
| Full build | `cargo check --workspace --locked --all-targets --features full` | PASS, exit 0 |
| All-features build | `cargo check --workspace --locked --all-targets --all-features` | PASS, exit 0 |
| Workspace tests | `cargo test --workspace --locked --features full` | PASS, 791 tests |
| BGP tests | `cargo test -p ramshield-bgp --locked` | PASS, 12 tests |
| Strict Clippy | `cargo clippy --workspace --locked --all-targets --features full -- -D warnings` | PASS, exit 0 |
| Dependency audit | `cargo audit --no-yanked` | PASS; 1 allowed `rustix` RUSTSEC-2026-0486 warning |
| License/ban/source | `cargo deny check` | PASS; advisories, bans, licenses, sources ok |
| Formatting | `cargo fmt --all -- --check` | FAIL, exit 1; only `examples/attack_demo.rs` after Slice A fix |
| Whitespace | `git diff --check origin/master` | PASS, exit 0 |

Impact by concern:

- Runtime: Slice A changes no runtime code. Slice B changes replay capacity
  only when `RAMSHIELD_REPLAY_PER_KEY_CAP` is set; default stays 256 and
  `ReplayStore::with_per_key_cap` clamps to `1..=1024`. Slice C is not called
  by `src/` or `tests/` outside its own crate.
- Build/CI: BGP entered the parent workspace (nested `[workspace]` and
  `Cargo.lock` removed). Its 12 tests now run under workspace test; previously
  they were skipped. Root `Cargo.lock` gained `ipnet`, `toml 0.8`,
  `tokio-test`, `tokio-stream`, `serde_spanned 0.6`, and related transitive
  crates.
- Security: no mesh, origin, MAC, or WAL changes. Latent issues: BGP
  `BgpConfig` derives `Debug` while carrying `peer_shared_key`; BGP HMAC uses
  a non-standard 32-byte tag appended after the BGP message.
- Correctness: BGP Schmitt trigger can disengage while flaps continue,
  contradicting its own doc comment.
- Hygiene: `ipnet` and `tokio-test` deps are unused after the current BGP
  edits; `examples/attack_demo.rs` is unformatted.
- Hosted CI: on `42ab8865`, CI PASS (`45011452456`), Backlog Gate FAIL
  (`45011452460`) from committed trailing whitespace, Review Pipeline FAIL
  (`45011452468`) as cascade. Current tree fix is uncommitted and therefore
  invisible to hosted gates.

Logs: `/home/m/.hermes/profiles/ramshield/cache/scratch/current-tree-{check,test,clippy,fmt2,all-features-check,audit,deny}.log`.

## 4. Phases

### Phase 0 — Split and stabilize the worktree

Impact: repository hygiene only; no behavior change.

Actions:
1. Do not use `git add .`. Stage named paths only.
2. Separate Slice A, B, C, D. Never mix them in one commit.
3. Format or exclude `examples/attack_demo.rs`.

Definition of done:
- Four disjoint file sets.
- `cargo fmt --all -- --check` exit 0 on the tree being committed.
- `git diff --check origin/master` exit 0.
- No user WIP overwritten or discarded.
- No commit, push, merge, or force-push performed without explicit authorization.

### Phase 1 — Land the audit/docs/test slice

Impact: ledger and CI evidence only. No packet, storage, mesh, or enforcement
behavior changes.

Actions:
1. Commit Slice A alone.
2. Run exact-SHA local gates on the new commit.
3. Push only if explicitly authorized; verify remote SHA after push.
4. Wait for CI + Backlog Gate on that exact SHA.
5. Obtain independent review; local green is REPORTED, not independent proof.

Definition of done:
- `cargo fmt --all -- --check` exit 0.
- `cargo check --workspace --locked --all-targets --features full` exit 0.
- `cargo clippy --workspace --locked --all-targets --features full -- -D warnings` exit 0.
- `cargo test --workspace --locked --features full` exit 0.
- `cargo audit --no-yanked` exit 0; allowed rustix warning recorded, not relabelled.
- `cargo deny check` exit 0.
- Hosted CI and Backlog Gate PASS with matching `headSha`.
- Independent review recorded.
- A-036 status becomes `PASS (exact SHA)` only after all above; otherwise it stays `IN PROGRESS`.
- No new backlog rows added for review findings.

### Phase 2 — Decide the IPC replay-cap override

Impact: replay capacity for IPC-authenticated requests; conditional on local
environment. Default path unchanged.

Actions:
1. Owner decision: keep demo-only, promote to documented config, or revert.
2. If kept: log the effective cap at startup and add one test that invalid
   input falls back to 256.
3. If reverted: restore the literal `256` argument.

Definition of done:
- Decision recorded in this file or `OPEN_FINDINGS.md`.
- Default cap 256 proven unchanged.
- Effective cap observable via log/metric if retained.
- Full workspace gates PASS.
- No silent production behavior change.

### Phase 3 — Harden BGP WIP before any integration decision

Impact: optional BGP crate only. It is not deployed. Do not enable it in
production in this phase.

Actions:
1. Fix Schmitt hysteresis: reset engagement timer on each flap or track
   `last_flap_at`; add a test where flaps continue inside the disengage window.
2. Redact `peer_shared_key` from `BgpConfig` `Debug`.
3. Remove unused `ipnet` and `tokio-test` deps; regenerate `Cargo.lock`.
4. Document the appended-HMAC framing as proprietary/WIP.
5. Format the crate and run its tests from the workspace.

Definition of done:
- P2 hysteresis finding closed with a regression test that fails on old behavior.
- No secret field printed by `Debug`.
- No unused deps in `crates/ramshield-bgp/Cargo.toml`.
- `cargo fmt --all -- --check`, `cargo test -p ramshield-bgp --locked`, and
  full workspace gates PASS.
- Independent review recorded.
- Production enablement remains OFF.

### Phase 4 — Resolve I-003 (P0, mesh)

Impact: production mesh safety and provenance. No code change until owner
decides.

Actions:
1. Owner decision on `origin/feature/mesh-f01-hmac-edge-secrets` (`d2f159fc`).
2. If rejected: keep mesh optional/disabled; leave I-003 BLOCKED with evidence.
3. If accepted: require exact-SHA CI, independent review, convergence,
   restart/retention, and multi-process fault-injection evidence.

Definition of done:
- Owner decision recorded.
- Branch ancestry verified with `git merge-base --is-ancestor`.
- If enabled, all named gates PASS on one exact SHA.
- If not enabled, mesh remains optional and disabled in production; no
  asymmetric identity or pairwise-HMAC migration without owner approval.

### Phase 5 — Resolve A-037 (P2, benchmark evidence)

Impact: performance documentation and regression policy only.

Actions:
1. Schedule a privileged host with known CPU, RAM, kernel, and NIC.
2. Measure kernel capture/TPACKET ring behavior, isolated store memory, and
   run-to-run variance.
3. Update `docs/development/BENCHMARK_BASELINES.md` with exact SHA and workload.

Definition of done:
- Three measurement classes recorded with raw output.
- Runner variance known.
- Regression thresholds added only if variance supports them.
- A-037 status becomes `PASS (exact SHA)` or `CLOSED` only when the row's
  own acceptance is met.

## 5. Ordering

1. Phase 0 — split and stabilize.
2. Phase 1 — land audit/docs/tests; restore hosted gate green.
3. Phase 2 — decide IPC override.
4. Phase 3 — harden BGP WIP; no production enablement.
5. Phase 4 — owner decision on mesh.
6. Phase 5 — privileged benchmarks.

Do not start Phase 3+ code while Phase 1 has unresolved blocking findings.
Do not combine phases into one commit.
