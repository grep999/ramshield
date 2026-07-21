# Healer Analysis — DELAYED-healer-solve-facts-missing-key-facts-json

## Issue
- **Issue ID:** `DELAYED-healer-solve-facts-missing-key-facts-json-missing-generated_at`
- **Category:** cron
- **Description:** `DELAYED: healer-solve-facts-missing-key-facts-json-missing-generated_at — 1.6h overdue, last_status=never`
- **Cycle:** 1

## Problem

This issue is the **SOLVE stage** of a second-generation healer chain that was dispatched for a stale `DELAYED` entry in `docs/ERRORS.md`. The underlying `FACTS.json` problem is already resolved.

### 1. The original FACTS.json issue is fixed and verified
- `docs/FACTS.json` currently exists and contains all required keys, including `"generated_at": "2026-07-21T08:20:35Z"`.
- `docs/HEALER_STATUS_facts-MISSING-KEY-FACTS-json-missing-generated_at.md` reports `Status: **FIXED**` and was verified at `2026-07-21T08:19:30Z`.
- `docs/HEALTH_CHECK.md` reports: `## 5. FACTS.json Health ✅ FACTS.json valid and fresh.`

### 2. The "delayed" object is a stale one-shot solve cron job
- The `healer-solve-facts-missing-key-facts-json-missing-generated_at` job was created by `error_healer.py:schedule_healer_chain()` as a `repeat: 1` one-shot SOLVE stage for the original `facts-MISSING-KEY-FACTS-json-missing-generated_at` issue.
- It was scheduled at 08:23 UTC but was never executed, leaving `last_status=never` and `next_run` in the past.
- `health_check_repair.py:check_cron_jobs()` (lines 60–98) flags any cron job whose `next_run` is in the past and `last_status != "ok"` as `DELAYED`, without distinguishing active recurring jobs from leftover one-shot healer jobs.

### 3. The root healer scheduler re-dispatches for already-fixed issues
- `error_healer.py:load_cycle_history()` (lines 161–168) only counts previous cycles; it does **not** check whether `docs/HEALER_STATUS_{issue_id}.md` already says the issue is fixed.
- Because the original verify job never completed, `ramshield-error-healer` spawned a second-generation chain for the `DELAYED-healer-verify-facts-missing-key-facts-json` entry at 08:13 UTC, and the current issue is the SOLVE stage of that redundant chain.
- The same pattern is documented in `docs/HEALER_ANALYSIS_DELAYED-healer-verify-facts-missing-key-facts-json.md` and `docs/HEALER_ANALYSIS_DELAYED-healer-solve-markdown-malformed-docs-cron_.md`.

### 4. Underlying execution failure is shared with other LLM healer jobs
- The recurring LLM jobs (`ramshield-helper-agent`, `ramshield-research-agent`, `ramshield-reviewer`) are also failing/skipping due to unpinned config drift against the current provider/model.
- The healer jobs are LLM agent jobs (`Skills: autonomous-project-agents`) and are unpinned, so they are at risk of being silently skipped by the spend-guard even if scheduled correctly.

## Exact Files / Lines

| File | Lines | Problem |
|------|-------|---------|
| `.github/scripts/health_check_repair.py` | 60–98 | `check_cron_jobs()` flags any past-`next_run` / non-ok cron job as `DELAYED`, including `repeat:1` one-shot healer jobs that were never executed. |
| `.github/scripts/error_healer.py` | 161–168, 337–347 | `load_cycle_history()` counts cycles but never checks whether `docs/HEALER_STATUS_{issue_id}.md` already says the issue is fixed; it dispatches a new chain for any `ERRORS.md` line. |
| `docs/ERRORS.md` | 6 | Lists the stale `DELAYED: healer-solve-facts-missing-key-facts-json-missing-generated_at` entry as an active issue. |
| `docs/HEALER_STATUS_facts-MISSING-KEY-FACTS-json-missing-generated_at.md` | 7 | Already reports `Status: **FIXED**`, proving the underlying issue is resolved. |

## Proposed Fix

1. **Make `error_healer.py` skip already-fixed issues.**
   - In `main()` before scheduling, read `docs/HEALER_STATUS_{issue_id}.md`.
   - If it contains `Fixed? yes`, `Status: **FIXED**`, or `Status: Resolved`, skip the issue and log `SKIPPED: {issue_id} already fixed`.

2. **Make `health_check_repair.py` tolerate stale one-shot healer jobs.**
   - In `check_cron_jobs()`, ignore `repeat: 1` jobs with `last_status=never` whose `next_run` is less than ~15 minutes past due, or add a grace window for one-shot jobs.
   - Alternatively, report them under a separate category such as `STALE ONESHOT` rather than `DELAYED`.

3. **Clean up the leftover cron job (solve stage only).**
   - Once the scheduler skips fixed issues, the existing stale `healer-solve-facts-missing-key-facts-json-missing-generated_at` one-shot job can be removed via `hermes cron remove <id>`.

4. **Regenerate `docs/ERRORS.md` / `docs/HEALTH_CHECK.md` so the stale DELAYED entry disappears.**
   - Run `python3 .github/scripts/health_check_repair.py` after the cleanup so the report reflects current state.

## One-line Summary

The DELAYED solve job is a stale `repeat:1` one-shot healer cron job for an already-resolved FACTS.json issue; the health checker flags all past-due non-ok cron jobs as delayed, and the root healer scheduler re-dispatches chains without checking whether the issue is already marked fixed.
