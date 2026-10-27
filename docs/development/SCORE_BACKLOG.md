# Module-score ≥85 backlog (branch `fix/module-score-85`)

Pick the **first row with status `TODO`**, implement **one bounded fix**, mark it `DONE` or `BLOCKED` with SHA, push to this branch only. Never force-push master. One item per automation run.

| Order | ID | Easy? | Area | Task | Status |
|------:|----|-------|------|------|--------|
| 1 | E01 | yes | CI | **RESTORE** full `src/engine/boot.rs` from master, then collapsible_if | TODO (merge blocker) |
| 1b | E05-restore | yes | protocol | **RESTORE** full `crates/ramshield-protocol/src/auth.rs` from master | TODO (merge blocker) |
| 2 | E02 | yes | CI | e2e ephemeral IPC port | DONE `c72bf89` |
| 3 | E03 | yes | process | ACCEPTANCE_REPORT vs Actions | DONE `54e2d1e` |
| 4 | E04 | yes | deploy | I-012 CAPS.md + DaemonSet rationale | DONE `597f604` / `529e987` |
| 5 | E05 | yes | protocol | I-024 HMAC pin | VERIFIED on master; restore auth.rs on branch first | BLOCKED |
| 6 | E06 | med | detection | pre_aggs rate-window unit test | DONE `4d75d44` |
| 7 | E07 | med | metrics | keystore validate+export parity | DONE `23f9285` (`tests/bloom_keystore_parity.rs`) |
| 8 | E08 | med | xdp | autonomous map skip/implement | TODO |
| 9 | E09 | med | native | drop counters + #90 | TODO |
| 10 | E10 | hard | mesh | out of scope | SKIP |

## Rules for the hourly job
1. Work only on `fix/module-score-85`.
2. First merge-blocker TODO first.
3. **Never replace a large source file with a stub.**
4. One commit per item.
5. Read before write; pass correct blob SHA on update.

## Run log
- E02/E03/E04 DONE.
- E06 DONE `4d75d44`.
- E07 DONE `23f9285`: bloom KEYSTORE ↔ Prometheus export + value parity tests.
- E01/E05-restore still required before merge (`boot.rs` / `auth.rs`).
