# Module-score ≥85 backlog (branch `fix/module-score-85`)

Pick the **first row with status `TODO`**, implement **one bounded fix**, mark it `DONE` or `BLOCKED` with SHA, push to this branch only. Never force-push master. One item per automation run.

| Order | ID | Easy? | Area | Task | Status |
|------:|----|-------|------|------|--------|
| 1 | E01 | yes | CI | **RESTORE** full `src/engine/bot.rs` → `src/engine/boot.rs` from master, then collapsible_if | TODO (merge blocker) |
| 1b | E05-restore | yes | protocol | **RESTORE** full `crates/ramshield-protocol/src/auth.rs` from master (branch tip is `PLACEHOLDER_WILL_FAIL`) | TODO (merge blocker) |
| 2 | E02 | yes | CI | e2e ephemeral IPC port | DONE `c72bf89` |
| 3 | E03 | yes | process | ACCEPTANCE_REPORT vs Actions | DONE `54e2d1e` |
| 4 | E04 | yes | deploy | I-012 CAPS.md + DaemonSet rationale | DONE `597f604` / `529e987` |
| 5 | E05 | yes | protocol | I-024 HMAC golden vector + README pin | **VERIFIED on master** (README + `golden_vector_pins_the_mac_input_layout` + `tests/readme_protocol.rs`); only residual was module-doc drift; branch auth.rs currently **corrupted** — restore first | BLOCKED until E05-restore |
| 6 | E06 | med | detection | pre_aggs rate-window unit test | TODO |
| 7 | E07 | med | metrics | keystore validate+export parity | TODO |
| 8 | E08 | med | xdp | autonomous map skip/implement | TODO |
| 9 | E09 | med | native | drop counters + #90 | TODO |
| 10 | E10 | hard | mesh | out of scope | SKIP |

## Rules for the hourly job
1. Work only on `fix/module-score-85`.
2. First `TODO` / merge-blocker first.
3. **Never replace a large source file with a stub.** If a push would truncate, stop and mark BLOCKED.
4. One commit per item.
5. Read before write; pass correct blob SHA on update.

## E05 verification notes (2026-10-27)
On **master**, I-024 is already closed:
- MAC input: `ts_ms` ++ `"."` ++ `key_id` ++ `payload` (`sign`/`verify` in auth.rs)
- `MAX_CLOCK_SKEW_MS = 10_000`
- Golden vector test + `tests/readme_protocol.rs` pin README facts
- README documents the real layout and 10s skew
Module-level `//!` comment on master still said `` `<ts_ms>.<payload>` `` (missing key_id) — doc-only drift.

## Run log
- E02/E03/E04 DONE on branch.
- E05 attempt truncated `auth.rs` to placeholder — **must `git checkout origin/master -- crates/ramshield-protocol/src/auth.rs`** then optional one-line module-doc fix.
- E01 still needs the same for `src/engine/boot.rs`.
