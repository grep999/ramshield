# Healer Analysis — DELAYED-healer-verify-facts-dead-links-1-5h-overdu

**Issue ID:** `DELAYED-healer-verify-facts-dead-links-1-5h-overdu`  
**Category:** cron  
**Description:** `DELAYED: healer-verify-facts-dead-links — 1.5h overdue, last_status=never`  
**Cycle:** 1

## Problem

The reported issue is a **stale one-shot verify cron job** for an already-resolved dead-link problem. The original `facts-dead-links` issue was analyzed, solved, and verified in cycle 1, but the leftover `healer-verify-facts-dead-links` one-shot cron job was never executed. The health checker flags any cron job with `next_run` in the past and `last_status != "ok"` as `DELAYED`, and the root healer scheduler re-dispatches a new `analyze/solve/verify` chain for this stale entry without checking whether the underlying issue is already marked fixed.

## Evidence

1. **Original dead-link issue is already fixed and verified:**
   - `docs/HEALER_STATUS_facts-dead-links.md` reports `Status: **FIXED**` and verified at `2026-07-21T08:14:00Z`.
   - `docs/HEALER_SOLUTION_facts-dead-links.md` documents the fix in `.github/scripts/facts_collector.py` (lines 174–214) and the two real bad relative links in `docs/HEALTH_DASHBOARD.md`.
   - `docs/FACTS.json` (current) contains `"dead_links": []` and was generated at `2026-07-21T08:20:35Z`.
   - `docs/HEALTH_CHECK.md` says: `## 5. FACTS.json Health ✅ FACTS.json valid and fresh.`

2. **The "delayed" object is a stale one-shot healer verify cron job:**
   - `health_check_repair.py:check_cron_jobs()` (lines 60–98) flags any cron job whose `next_run` is in the past and `last_status != "ok"` as `DELAYED`, without distinguishing active recurring jobs from leftover one-shot healer jobs.
   - The job name `healer-verify-facts-dead-links` matches the naming convention produced by `error_healer.py:schedule_healer_chain()` (lines 246–274) for the verify stage.

3. **Root healer scheduler does not skip already-fixed issues:**
   - `error_healer.py:load_cycle_history()` (lines 161–168) only counts previous cycles from the healer status file; it does not check whether the status file indicates the issue is already fixed.
   - `error_healer.py:main()` (lines 303–382) dispatches a new `analyze/solve/verify` chain for every entry in `docs/ERRORS.md`, even when the underlying issue is resolved and only the stale one-shot cron job remains.

4. **Same pattern visible for other issues:**
   - `docs/HEALER_ANALYSIS_DELAYED-healer-verify-facts-missing-key-facts-json.md` and `docs/HEALER_ANALYSIS_DELAYED-healer-verify-dashboard-missing-section-ma.md` identify the identical root cause: stale one-shot healer jobs being re-dispatched because the scheduler ignores `HEALER_STATUS_*` "fixed" markers.

## Exact Files / Lines

| File | Lines | Problem |
|------|-------|---------|
| `.github/scripts/health_check_repair.py` | 60–98 | `check_cron_jobs()` flags any past-`next_run` / non-ok cron job as `DELAYED`, including `repeat:1` one-shot healer jobs that were never executed. |
| `.github/scripts/error_healer.py` | 161–168, 337–347 | `load_cycle_history()` counts cycles but never checks whether `docs/HEALER_STATUS_{issue_id}.md` already says the issue is fixed; it dispatches a new chain for any `ERRORS.md` line. |
| `docs/ERRORS.md` | 7 | Lists the stale `DELAYED: healer-verify-facts-dead-links` entry as an active issue. |
| `docs/HEALER_STATUS_facts-dead-links.md` | 8 | Already reports `Status: **FIXED**`, proving the underlying work is done. |

## Proposed Fix

1. **Make `error_healer.py` skip already-fixed issues.**
   - In `main()` before scheduling, read `docs/HEALER_STATUS_{issue_id}.md`.
   - If it contains `Fixed? yes`, `Status: **FIXED**`, or `Status: Resolved`, skip the issue and log `SKIPPED: {issue_id} already fixed`.

2. **Make `health_check_repair.py` tolerate stale one-shot healer jobs.**
   - In `check_cron_jobs()`, ignore `repeat: 1` jobs with `last_status=never` whose `next_run` is less than ~15 minutes past due, or add a grace window for one-shot jobs. Alternatively, report them under a separate category such as `STALE ONESHOT` rather than `DELAYED`.

3. **Clean up the leftover cron job (solve/verify stage only).**
   - Once the scheduler skips fixed issues, the existing stale `healer-verify-facts-dead-links` one-shot job can be removed via `hermes cron remove <id>`.

4. **Regenerate `docs/ERRORS.md` / `docs/HEALTH_CHECK.md` so the stale DELAYED entry disappears.**
   - Run `python3 .github/scripts/health_check_repair.py` after the cleanup so the report reflects current state.

## One-line Summary

The DELAYED verify job is a stale `repeat:1` one-shot healer cron job for an already-resolved dead-link issue; the health checker flags all past-due non-ok cron jobs as delayed, and the root healer scheduler re-dispatches chains without checking whether the issue is already marked fixed.
