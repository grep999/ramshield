# Healer Analysis — DELAYED-healer-verify-facts-missing-key-facts-json

## Issue
- **Issue ID:** `DELAYED-healer-verify-facts-missing-key-facts-json-missing-generated_at`
- **Category:** cron
- **Description:** `DELAYED: healer-verify-facts-missing-key-facts-json-missing-generated_at — 1.7h overdue, last_status=never`
- **Cycle:** 1

## Evidence

1. **The underlying FACTS.json issue is already resolved.**
   - `docs/FACTS.json` exists and is valid; it contains all required keys:
     - `generated_at`: `2026-07-21T09:41:57Z`
     - `codebase`: present
     - `roadmap_open_tasks`: present (22 tasks)
     - `backlog_remaining`: present (18)
   - `docs/HEALER_STATUS_facts-MISSING-KEY-FACTS-json-missing-generated_at.md` already reports **Status: FIXED**.

2. **The reported object is a stale one-shot verify cron job.**
   - The name `healer-verify-facts-missing-key-facts-json-missing-generated_at` matches the `healer-verify-*` one-shot jobs created by `error_healer.py` (`repeat: 1`).
   - It never executed (`last_status=never`), its `next_run` is in the past, and `health_check_repair.py` flags it as `DELAYED`.

3. **This is a duplicate/stale dispatch.**
   - A prior cycle already analyzed, solved, and verified this same stale one-shot situation in:
     - `docs/HEALER_ANALYSIS_DELAYED-healer-verify-facts-missing-key-facts-json.md`
     - `docs/HEALER_SOLUTION_DELAYED-healer-verify-facts-missing-key-facts-json.md`
     - `docs/HEALER_STATUS_DELAYED-healer-verify-facts-missing-key-facts-json.md`
   - Those files identify the root cause as the health checker classifying stale one-shot healer jobs as `DELAYED` and the root scheduler re-dispatching already-fixed issues.
   - The fixes applied in that cycle (`health_check_repair.py` one-shot tolerance and `error_healer.py` already-fixed skip) should prevent new duplicates once they take effect, but leftover one-shot jobs from before the fix continue to surface as stale `DELAYED` entries.

## Exact Files / Lines

| File | Lines | Problem |
|------|-------|---------|
| `docs/FACTS.json` | 2 | All required keys present; no actual missing-key issue. |
| `docs/HEALER_STATUS_facts-MISSING-KEY-FACTS-json-missing-generated_at.md` | 7 | Already reports **FIXED**. |
| `docs/HEALER_STATUS_DELAYED-healer-verify-facts-missing-key-facts-json.md` | 7 | Already reports **Fixed? Yes** for this stale one-shot. |
| `.github/scripts/health_check_repair.py` | 60-122 | `check_cron_jobs()` flags past-due non-ok cron jobs as `DELAYED`; one-shot healer jobs need cleanup rather than re-dispatch. |
| `.github/scripts/error_healer.py` | 132-139, 189-206 | Already skips already-fixed issues; this dispatch is a leftover stale job. |

## Proposed Fix

1. **Do NOT re-edit source files** — the underlying check already passes.
2. **Treat this as a stale one-shot cleanup task**, not a new codebase issue.
3. **Remove the leftover `healer-verify-facts-missing-key-facts-json-missing-generated_at` one-shot cron job** via `hermes cron remove <id>` (infrastructure-layer action, not solve-stage).
4. **Regenerate `docs/ERRORS.md`** by running `python3 -W error .github/scripts/health_check_repair.py` so the stale `DELAYED` entry disappears.

## One-line Summary
Stale `repeat:1` verify healer job for an already-fixed FACTS.json missing-key issue; the underlying check passes and only the leftover one-shot cron job remains.
