# Healer Analysis: DELAYED-healer-verify-dashboard-missing-section-ma

**Issue ID:** DELAYED-healer-verify-dashboard-missing-section-ma
**Category:** cron
**Description:** DELAYED: healer-verify-dashboard-missing-section-main-timeline-marker-mai — 1.5h overdue, last_status=never
**Cycle:** 1

## Problem

The issue is a **stale, already-healed cron job** that is still being reported as delayed/overdue. The dashboard missing-section problem was already resolved in a previous cycle (`dashboard-MISSING-SECTION-Main-Timeline-marker-Mai`), but the root `error_healer.py` scheduler does not clean up the original one-shot healer jobs that were delayed and never ran. Health check then sees these as `DELAYED: ... last_status=never` and opens a new healing cycle for a non-existent issue.

## Evidence

1. **Original issue is fixed:**
   - `docs/HEALER_STATUS_dashboard-MISSING-SECTION-Main-Timeline-marker-Mai.md` says `Fixed? yes` and was verified at `2026-07-21T10:02:30Z`.
   - `docs/HEALER_SOLUTION_dashboard-MISSING-SECTION-Main-Timeline-marker-Mai.md` documents the fix in `.github/scripts/health_check_repair.py` (lines 233-247).
   - `docs/HEALTH_CHECK.md` says: `## 4. Dashboard Sections ✅ All 13 dashboard sections present.` (no `MISSING SECTION` errors).
   - `docs/ERRORS.md` lists only cron/deadlink issues, none of them are dashboard `MISSING SECTION` errors.

2. **Stale one-shot jobs still exist in `hermes cron list`:**
   - `healer-analyze-delayed-healer-verify-dashboard-missing-section-ma` — once at 08:13, already ran (current time is 08:23).
   - `healer-solve-delayed-healer-verify-dashboard-missing-section-ma` — once at 08:23.
   - `healer-verify-delayed-healer-verify-dashboard-missing-section-ma` — once at 08:33.
   These are still shown as "active" in the cron list because `repeat: 1` one-shot jobs remain until they run (or are deleted). They show `last_status=never` because the old `healer-verify-dashboard-missing-section-main-timeline-marker-mai` (the original bug) is not the same job ID; the `DELAYED` entry is the health check reporting the original one-shot jobs as delayed, not a new independent failure.

3. **Health check uses `last_status=never` + `next_run` past-due detection to flag these as DELAYED:**
   - `health_check_repair.py` lines 75-92: if `next_run < now` and `status != "ok"`, it flags `DELAYED` or `STUCK`.
   - Since the one-shot healer jobs were created at a time in the past and not yet run (or not yet considered ok), the health check reports them as delayed.

4. **Original delayed job no longer exists:**
   - `grep -i 'healer-verify-dashboard-missing-section-main-timeline-marker-mai'` in `hermes cron list` returns nothing — the original job is gone, but the name lives on in the newly-categorized issue.

## Root Cause

The root healer scheduler (`error_healer.py`) does not skip issues when a corresponding `HEALER_STATUS_*` file already reports the issue as `Fixed? yes`. It schedules a new chain (`analyze`/`solve`/`verify`) for any error body it sees in `docs/ERRORS.md`, even if the underlying issue has already been resolved and only stale cron jobs remain. The health checker, in turn, reports any cron job with `next_run` in the past and `last_status=never` as `DELAYED`, creating a self-referential loop of delayed-healer jobs for fixed problems.

## Affected Files / Lines

- `.github/scripts/error_healer.py`
  - `load_cycle_history()` (lines 161-168): only counts cycles, but does not check the actual content of `HEALER_STATUS_*.md` for `Fixed? yes`.
  - `main()` (lines 303-382): never short-circuits if a status file indicates the issue is already resolved.
- `.github/scripts/health_check_repair.py`
  - `check_cron_jobs()` (lines 60-98): flags one-shot jobs as `DELAYED` when they are simply scheduled and not yet run, without distinguishing one-shot delayed jobs from active stuck jobs.

## Proposed Fix

1. **In `error_healer.py`, skip scheduling when an issue is already fixed.**
   - Add a check in `main()` or `load_cycle_history()` that reads `docs/HEALER_STATUS_{issue_id}.md` and, if it contains `Fixed? yes` or `Fixed? true` or `Status: Resolved`, skips the issue and logs `SKIPPED: {issue_id} already fixed`.
   - This prevents the root scheduler from spawning healers for already-resolved problems.

2. **In `health_check_repair.py`, improve `check_cron_jobs()` for one-shot jobs.**
   - When a job is `Repeat: 1` and already scheduled, don't flag it as `DELAYED` just because its `next_run` is within the next 30 minutes; only flag one-shot jobs as delayed if their `next_run` is more than ~1 hour in the past **and** `last_status != ok`.
   - Alternatively, add a grace period based on the current time: `next_run < now - timedelta(minutes=15)` instead of `next_run < now`.
   - This reduces false-positive `DELAYED` alerts for near-future one-shot jobs that are still waiting to run.

3. **(Manual cleanup for this issue):** Remove the stale one-shot healer jobs that are already scheduled for the fixed dashboard issue.
   - `hermes cron remove <id>` for the `healer-analyze-delayed-...`, `healer-solve-delayed-...`, and `healer-verify-delayed-...` jobs related to this dashboard issue. (Do not remove during ANALYZE stage; this is only a proposed fix for the SOLVE stage.)

4. **Do NOT modify the dashboard generator or health check section markers** — they are already correct and verified as fixed.

## Summary

This is a **healer scheduler false-positive**, not a real dashboard issue. The dashboard missing-section problem was already fixed and verified. The current issue is caused by stale one-shot healer jobs being flagged as delayed by the health checker, which then re-dispatches the same healing chain. The fix is to make the healer scheduler skip already-resolved issues and to relax the cron delay detection for pending one-shot jobs.
