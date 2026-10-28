# Module-score ≥85 backlog (branch `fix/module-score-85`)

Pick the **first row with status `TODO`**, implement **one bounded fix**, mark it `DONE` or `BLOCKED` with SHA, push to this branch only. Never force-push master. One item per automation run.

| Order | ID | Easy? | Area | Task | Status |
|------:|----|-------|------|------|--------|
| 1 | E01 | yes | CI | restore boot.rs + collapsible_if | DONE `5375be40` |
| 1b | E05-restore | yes | protocol | restore auth.rs from master after PLACEHOLDER | DONE `4814ce44` |
| 2 | E02 | yes | CI | e2e ephemeral IPC port | DONE `c72bf89` |
| 3 | E03 | yes | process | ACCEPTANCE_REPORT vs Actions | DONE `54e2d1e` |
| 4 | E04 | yes | deploy | I-012 CAPS.md + DaemonSet | DONE `597f604` / `529e987` |
| 5 | E05 | yes | protocol | I-024 HMAC pin | DONE (master verified + branch restored; module doc has key_id) |
| 6 | E06 | med | detection | pre_aggs rate-window unit test | DONE `4d75d44` |
| 7 | E07 | med | metrics | keystore validate+export parity | DONE `23f9285` |
| 8 | E08 | med | xdp | autonomous map skip/implement | DONE `cbbb9ad` (AUTONOMOUS_CONFIG Array map added to BPF) |
| 9 | E09 | med | native | drop counters + #90 | TODO |
| 10 | E10 | hard | mesh | out of scope | SKIP |

## Rules for the hourly job
1. Work only on `fix/module-score-85`.
2. First TODO only; one commit.
3. **Never replace a large source file with a stub.**
4. Read before write; pass correct blob SHA on update.

## Run log
- E01 DONE; E05-restore DONE `4814ce44` (full auth.rs + corrected module MAC doc).
- E02–E07 DONE.
- E08 DONE `cbbb9ad` (implemented AUTONOMOUS_CONFIG map in XDP BPF; logic pending).
- Next: **E09** native drop counters.
