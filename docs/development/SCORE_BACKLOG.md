# Module-score ≥85 backlog (branch `fix/module-score-85`)

Pick the **first row with status `TODO`**, implement **one bounded fix**, mark it `DONE` or `BLOCKED` with SHA, push to this branch only. Never force-push master. One item per automation run.

| Order | ID | Easy? | Area | Task | Status |
|------:|----|-------|------|------|--------|
| 1 | E01 | yes | CI | Collapse nested `if autonomous` in `src/engine/boot.rs` (clippy collapsible_if under full) | DONE (batch-0) |
| 2 | E02 | yes | CI | `tests/e2e_telemetry_block.rs`: ephemeral IPC port (no fixed 17897) + unique wal dir | DONE (batch-0) |
| 3 | E03 | yes | process | Fix ACCEPTANCE_REPORT.md workflow table to match real Actions conclusions on cited SHAs | DONE (aligned table to real failure/success on 13d7a40; commit 54e2d1e) |
| 4 | E04 | yes | deploy | I-012: document hostNetwork/NET_ADMIN in `deploy/k8s/daemonset.yaml` + short CAPS note | TODO |
| 5 | E05 | yes | protocol | I-024: pin HMAC golden vector / README facts if still drift on tip | TODO |
| 6 | E06 | med | detection | Unit test for pre_aggs rate-window / batch start invariant | TODO |
| 7 | E07 | med | metrics | Bloom/metrics keystore validate+export parity on tip | TODO |
| 8 | E08 | med | xdp | Autonomous: skip configure when map absent OR implement map; no false hard-fail | TODO |
| 9 | E09 | med | native | Confirm drop counters + worker health exported; close residual #90 diagnostics | TODO |
| 10 | E10 | hard | mesh | Out of scope for ≥85 product bar unless I-003 funded (keep disabled) | SKIP |

## Rules for the 30-min/hourly job
1. Work only on branch `fix/module-score-85`.
2. Read this file; take first `TODO`.
3. Verify against current source; if already fixed, mark DONE and stop.
4. One commit, focused message `fix(E0N): ...`.
5. Do not invent file contents; read before write.
6. Prefer tests over docs-only unless the row is docs.
7. After push, leave a short note under ## Run log.

## Run log
- batch-0: branch created; E01 boot.rs collapsible_if; E02 e2e ephemeral port.
- 2026-10-27T22:34Z: E03 DONE — aligned ACCEPTANCE_REPORT.md workflow table/conclusions to real Actions (CI/Review/Subnet failure, lint success) on 13d7a40; commit 54e2d1e3f8c752916453015b6be142a3bf10f71f.
