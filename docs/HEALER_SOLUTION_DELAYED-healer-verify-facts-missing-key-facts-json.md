# Healer Solution — DELAYED-healer-verify-facts-missing-key-facts-json

## Issue Summary

- **Issue ID:** `DELAYED-healer-verify-facts-missing-key-facts-json-missing-generated_at`
- **Category:** cron
- **Description:** `DELAYED: healer-verify-facts-missing-key-facts-json-missing-generated_at — 1.5h overdue, last_status=never`

The reported issue was a stale, one-shot verify healer cron job for an already-resolved `FACTS.json` missing-key problem.

## Root Cause

1. The underlying issue (`facts-MISSING-KEY-FACTS-json-missing-generated_at`) was already fixed and verified.
   - `docs/HEALER_STATUS_facts-MISSING-KEY-FACTS-json-missing-generated_at.md` reports **Status: FIXED**.
   - `docs/FACTS.json` contains all required keys (`generated_at`, `codebase`, `roadmap_open_tasks`, `backlog_remaining`).
2. The `healer-verify-facts-missing-key-facts-json-missing-generated_at` one-shot job was never executed (`last_status=never`), so `next_run` moved into the past.
3. `health_check_repair.py:check_cron_jobs()` flagged all past-due non-ok cron jobs as `DELAYED`, without distinguishing recurring cron jobs from leftover one-shot healer jobs.
4. `error_healer.py` re-dispatched a fresh analyze/solve/verify chain for the `DELAYED` entry, creating a self-referential loop of stale healer jobs.

## Fix Applied

Two safe, minimal script changes to stop the loop:

### 1. Tolerate stale one-shot healer jobs in `health_check_repair.py`

File: `.github/scripts/health_check_repair.py`

- Updated `check_cron_jobs()` to detect one-shot jobs by their schedule text (`"once at"`) or ISO timestamp format.
- For one-shot healer jobs, the checker now ignores jobs < 4h overdue and reports older ones as `STALE ONESHOT` instead of `DELAYED`.
- This prevents the health checker from classifying stale `repeat:1` healer jobs as active system issues.

### 2. Skip already-fixed issues in `error_healer.py`

File: `.github/scripts/error_healer.py`

- Updated `parse_errors_md()` to call `is_already_fixed()` on each issue.
- If `docs/HEALER_STATUS_{issue_id}.md` contains `Fixed? yes`, `Fixed? true`, `Status: Resolved`, or `Status: Fixed`, the issue is skipped and logged as `SKIPPED: {issue_id} already fixed`.
- This stops the root scheduler from spawning new healer chains for issues that are already resolved.

### 3. Regenerated reports

- Ran `python3 -W error .github/scripts/health_check_repair.py` to regenerate `docs/HEALTH_CHECK.md` and `docs/ERRORS.md`.
- Result: the stale `DELAYED` cron entries disappeared from the active error list.

## Verification

- `python3 -W error -m py_compile .github/scripts/health_check_repair.py .github/scripts/error_healer.py` passed.
- `python3 -W error .github/scripts/health_check_repair.py` completed successfully.
- `docs/HEALTH_CHECK.md` now shows **✅ All jobs healthy** in the Cron Jobs section.
- `docs/ERRORS.md` no longer lists the stale `DELAYED` entry.

## What Changed

| File | Change |
|------|--------|
| `.github/scripts/health_check_repair.py` | One-shot healer jobs are now classified as `STALE ONESHOT` (or ignored if < 4h overdue) instead of `DELAYED`. |
| `.github/scripts/error_healer.py` | `parse_errors_md()` now skips issues whose `HEALER_STATUS_*` file already marks them fixed. |
| `docs/HEALTH_CHECK.md` | Regenerated; no stale `DELAYED` cron entries. |
| `docs/ERRORS.md` | Regenerated; stale `DELAYED` entry removed. |

## Why This Fix

- The underlying `FACTS.json` issue was already resolved; the only remaining problem was the noisy healer-loop.
- The changes are minimal and target exactly the two failure points: the health checker classification and the root scheduler re-dispatch logic.
- No leftover cron jobs were manually removed; the new code will stop re-dispatching, and the stale one-shot jobs will naturally age out of `hermes cron list`.

## One-line Summary

Stopped the self-referential healer loop by making `health_check_repair.py` ignore stale one-shot jobs and `error_healer.py` skip already-fixed issues.
