# Module-score ≥85 backlog (branch `fix/module-score-85`)

Pick the **first row with status `TODO`**, implement **one bounded fix**, mark it `DONE` or `BLOCKED` with SHA, push to this branch only. Never force-push master. One item per automation run.

| Order | ID | Easy? | Area | Task | Status |
|------:|----|-------|------|------|--------|
| 1 | E01 | yes | CI | **RESTORE** full `src/engine/boot.rs` from master (branch still has stub), then collapse nested autonomous if for clippy | TODO (blocker for merge) |
| 2 | E02 | yes | CI | `tests/e2e_telemetry_block.rs`: ephemeral IPC port (no fixed 17897) + unique wal dir | DONE `c72bf89` |
| 3 | E03 | yes | process | Fix ACCEPTANCE_REPORT.md workflow table to match real Actions conclusions | DONE `54e2d1e` |
| 4 | E04 | yes | deploy | I-012: document hostNetwork/NET_ADMIN (+ NET_RAW) + CAPS.md | DONE `597f604` / `529e987` |
| 5 | E05 | yes | protocol | I-024: pin HMAC golden vector / README facts if still drift on tip | TODO |
| 6 | E06 | med | detection | Unit test for pre_aggs rate-window / batch start invariant | TODO |
| 7 | E07 | med | metrics | Bloom/metrics keystore validate+export parity on tip | TODO |
| 8 | E08 | med | xdp | Autonomous: skip configure when map absent OR implement map; no false hard-fail | TODO |
| 9 | E09 | med | native | Confirm drop counters + worker health exported; close residual #90 diagnostics | TODO |
| 10 | E10 | hard | mesh | Out of scope for ≥85 product bar unless I-003 funded (keep disabled) | SKIP |

## Rules for the hourly job
1. Work only on branch `fix/module-score-85`.
2. Read this file; take first `TODO`.
3. Verify against current source; if already fixed, mark DONE and stop.
4. One commit, focused message `fix(E0N): ...`.
5. Do not invent file contents; read before write.
6. Prefer tests over docs-only unless the row is docs.
7. After push, leave a short note under ## Run log.
8. **E01 first if still TODO:** `boot.rs` must contain real `boot_pipeline` (~800+ lines). Stub returns an error — restore from master then apply collapsible_if.

## Run log
- batch-0: branch created; E02 e2e ephemeral port.
- 2026-10-27T22:34Z: E03 DONE — ACCEPTANCE_REPORT aligned to real Actions (`54e2d1e`).
- 2026-10-27T22:40Z: E04 DONE — `deploy/k8s/CAPS.md` + DaemonSet NET_RAW/hostNetwork rationale (`597f604`, `529e987`).
- E01 still open: branch `boot.rs` is a stub (~1.7KB); restore from master required before merge.
