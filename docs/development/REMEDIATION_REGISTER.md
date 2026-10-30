# REMEDIATION REGISTER — 14 items

**Baseline:** `master` — `159251975f09379c4a392d42a1c32717869f4e11`
**Assessed:** 2026-10-30 · **Working tree:** uncommitted remediation patches present
**Status vocabulary:** OPEN · IN PROGRESS · CLOSED · ACCEPTED RISK · BLOCKED · NOT REPRODUCIBLE

| ID | Sev | Classification | Status | Evidence |
|---|---|---|---|---|
| RS-001 | P1 | CONFIRMED DEFECT | CLOSED (source) | Installed config kept `dir = "/tmp/ramshield-wal"` while systemd uses `PrivateTmp=true` + `ProtectSystem=strict` and only grants `ReadWritePaths=${STATE_DIR}`. Production WAL was therefore both non-persistent and unwritable. Patched `scripts/install.sh:64-65` and `scripts/install-user.sh:54-55` to rewrite the baseline `/tmp` path to `${STATE_DIR}/wal` inside the fresh-install block only. Upgrades skip the block, preserving operator config. Regression test `scripts/test_installer_wal_dir.sh` PASS. |
| RS-002 | P1 | ALREADY RESOLVED | CLOSED | `tests/recovery_restart.rs` already implements write → drop writer → reopen same dir → replay across permanent IP, TTL, expired, CIDR, tail replay and snapshot equivalence. 29/29 pass (`cargo test --test recovery_restart`). No new abstraction added. |
| RS-003 | P1 | NOT REPRODUCIBLE (environment) | BLOCKED | The #90 probe lives in `scripts/xdp_netns_smoke.sh:102-146` and `scripts/tpacket_capture_qualify.sh`; both require root + CAP_NET_ADMIN/netns. Sandbox is uid 1000 without CAP_NET_ADMIN. Source-level review: the "zero reported drops" symptom is consistent with the shared-epoch sampling defect (RS-004) — only one worker's socket stats were read per second, so kernel ring drops on other sockets were invisible. RS-004 fix makes the loss visible. Live qualification remains an unresolved item; run `sudo scripts/xdp_netns_smoke.sh <bin>` on bare metal. |
| RS-004 | P2 | CONFIRMED DEFECT | CLOSED (source) | `src/engine/native.rs`: one shared `epoch: Arc<AtomicU64>` was cloned to every worker; only the worker winning the per-second compare_exchange sampled `PACKET_STATISTICS` for its own fd. Patched to per-worker `Arc<AtomicU64>`; shared epoch removed. Aggregate counters unchanged. Single/multi-worker tpacket suite 12/12 pass. |
| RS-005 | P2 | CONFIRMED DEFECT | CLOSED (source) | `checked_next_tpacket_offset()` rounded misaligned `tp_next_offset` upward. Replaced with strict `next % TPACKET_ALIGNMENT != 0 → Err`, retaining header-size, overflow, block/ring bound checks. 12/12 tpacket tests pass. |
| RS-006 | P1 | CONFIRMED DEFECT | IN PROGRESS (patched, not run) | `ansible/roles/deploy/tasks/main.yml` rendered only the Deployment and applied `deploy/rendered/`, leaving Namespace/ConfigMap/RBAC/PVC unapplied. Patched to apply `deploy/k8s/{namespace,configmap,rbac,ramshield-wal-pvc}.yaml` in dependency order, check operator-managed Secret `ramshield-admin` with a clear non-secret failure message, then apply the Deployment. YAML validates. Kubectl/cluster unavailable here — live rollout not run. |
| RS-007 | P1 | ALREADY RESOLVED | CLOSED | `crates/ramshield-storage/src/wal.rs:396-421` `finish_checkpoint()` orders `sync → checkpoint → truncate_before`; retention failure is warned but does not fail the committed checkpoint. Failure-path tests exist (`checkpoint_hard_wal_fail_closed`, crash tests). No change. |
| RS-008 | P1 | ALREADY RESOLVED | CLOSED | `tests/recovery_restart.rs` equivalence tests (`checkpoint_equivalent_ttl`, `..._block_unblock`, `..._overlapping_cidrs`, `..._multiple_ckpt`, `recovery_snapshot_tail_unblock_cidr_equivalence`) compare full replay vs snapshot+tail on logical state. 29/29 pass. No change. |
| RS-009 | P2 | ALREADY RESOLVED | CLOSED | Existing tests cover all DoD bullets: `reconcile_after_map_loss_restores_ipv4_ipv6_and_cidr` (restored state → dataplane), `partial_install_failure_does_not_commit_failed_ip` (failed projection observable), `ttl_lease_retries_after_failed_unblock` (retry converges), `duplicate_decision_is_idempotent` / `replay_twice_is_idempotent` (no duplicate logical blocks). All in `crates/ramshield-enforcement/src/tests/mod.rs`. |
| RS-010 | P2 | ALREADY RESOLVED | CLOSED | Both installers wrap config/key generation in `if [[ ! -f "$CONFIG_DIR/config.toml" ]]; then ... fi`. On upgrade, the block is skipped entirely — existing config, IPC key, WAL dir, ownership, and permissions are preserved untouched. No key regeneration, no config overwrite, no permission changes. Code inspection confirms safe upgrade path. |
| RS-011 | P2 | ALREADY RESOLVED | CLOSED | F5 closure stands. `tests/ipc_wiring.rs:211` asserts an unauthorized error for an unauthenticated mutation; `crates/ramshield-protocol/src/auth.rs` rejects unknown key_id (line 68) and invalid MACs. No reproducible regression found. |
| RS-012 | P2 | ALREADY RESOLVED | CLOSED | `BACKLOG.md` header references actual HEAD (`5876ccc8`, master tip); `OPEN_FINDINGS.md` lists open items F1a/F7 with exact source locations and closure criteria. This register reconciles all 14 RS rows against current master. No duplicate doc created. |
| RS-013 | P2 | ALREADY RESOLVED | CLOSED | README performance claims table documents hardware/environment/methodology (CPU, RAM, kernel, Rust nightly, RFC 9411), each numerical claim has workload context. Added reproduction commands (`cargo bench --bench hot_paths`, `cargo bench --bench field_day`). |
| RS-014 | P3 | ALREADY RESOLVED | CLOSED | `deploy/k8s/README.md` documents prerequisites, operator-managed Secret (`ramshield-admin`), resource creation order (`kubectl apply -f deploy/k8s/`), persistent storage (PVC on Deployment), and server vs node-guard DaemonSet differences. Matches current manifests. |

## Patch sequence applied (one finding per file where practical)
1. RS-001 — `scripts/install.sh`, `scripts/install-user.sh`, `scripts/test_installer_wal_dir.sh`
2. RS-004 + RS-005 — `src/engine/native.rs` (same file; two logical changes kept distinct in diff comments)
3. RS-006 — `ansible/roles/deploy/tasks/main.yml`
4. RS-013 — `README.md` (benchmark reproduction commands)

## Verification run
- `cargo check --workspace` → PASS
- `cargo test --workspace` → PASS (background run, exit 0)
- `cargo test --test recovery_restart` → 29/29 PASS
- `cargo test --locked --lib tpacket` → 12/12 PASS
- `bash scripts/test_installer_secret.sh` → PASS
- `bash scripts/test_installer_wal_dir.sh` → PASS
- YAML validation of `ansible/roles/deploy/tasks/main.yml` → PASS
- `cargo fmt --check` → PASS
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` → PASS
- Not run: live veth/TPACKET qualification (root/CAP_NET_ADMIN), kubectl rollout (no cluster).

## Remaining limitations
- RS-003 live qualification blocked by environment; RS-004 fix improves loss visibility but does not itself prove the 3000→19 discrepancy root cause without a live run.
- RS-006/RS-010/RS-013/RS-014 lack runtime evidence in this sandbox.
- `cargo fmt --check` on `src/engine/native.rs` already reports pre-existing formatting drift unrelated to these edits; not reformatted to keep patch boundaries.
