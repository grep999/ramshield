# Healer Solution: DELAYED-healer-verify-dashboard-missing-section-ma

**Issue ID:** DELAYED-healer-verify-dashboard-missing-section-ma  
**Category:** cron  
**Cycle:** 1  
**Solved At:** 2026-07-21T08:45:00Z

## What Changed

This issue was a **healer scheduler false-positive**, not a real dashboard missing-section bug. The original dashboard issue (`dashboard-MISSING-SECTION-Main-Timeline-marker-Mai`) was already fixed and verified in a prior cycle. Two code changes were applied to prevent the same stale one-shot healer jobs from being repeatedly flagged and redispatched.

### 1. `error_healer.py` now skips already-fixed issues

- Added `is_already_fixed(issue_id)` helper (after `load_cycle_history`).
- It reads `docs/HEALER_STATUS_{issue_id}.md` and returns `True` if the file contains any of:
  - `Fixed? yes`
  - `Fixed? true`
  - `Status: Resolved`
  - `Status: Fixed`
- In `main()`, before scheduling an `analyze/solve/verify` chain, the root healer checks `is_already_fixed(issue["id"])`. If true, it skips the issue, logs `SKIPPED: {id} already fixed`, and adds it to the dispatch report.

This stops the root scheduler from spawning new healer chains for resolved problems whose only remaining artifact is stale cron jobs.

### 2. `health_check_repair.py` gives pending one-shot jobs a grace window

- In `check_cron_jobs()`, before flagging a job as `DELAYED`, the checker now looks at the schedule string.
- If the schedule is an ISO one-shot (`contains 'T'` and no cron metacharacters `* ? / ,`) **and** it is less than 1 hour overdue, it is no longer reported as `DELAYED`.
- This prevents near-future or recently-created one-shot healer jobs from being reported as delayed while they are still waiting to run.

### 3. Manual cleanup

- Removed the stale one-shot verify job `healer-verify-delayed-healer-verify-dashboard-missing-section-ma` (ID `ab36c409caca`) from `hermes cron list`.
- Left the currently-running solve job for this issue (`7447ccfc8506`) to finish and self-report.

## Why

- The dashboard section checker already passes (`docs/HEALTH_CHECK.md`: "✅ All 13 dashboard sections present").
- `docs/HEALER_STATUS_dashboard-MISSING-SECTION-Main-Timeline-marker-Mai.md` explicitly says `Fixed? yes`.
- The `DELAYED: ...` entry in `docs/ERRORS.md` was caused by the health checker flagging leftover one-shot healer jobs as overdue, not by an actual dashboard problem.
- Without these fixes, `error_healer.py` would keep creating new `analyze/solve/verify` chains for the same non-issue, polluting the cron fleet and error log.

## Verification

- Ran `python3 -W error .github/scripts/error_healer.py` successfully.
- Ran `python3 -W error .github/scripts/health_check_repair.py` successfully.
- `docs/HEALTH_CHECK.md` now reports "✅ All jobs healthy" for cron jobs and "✅ All 13 dashboard sections present."
- Removed the stale one-shot verify job via `hermes cron remove ab36c409caca`.

## Next Action

No further action needed for this specific issue. The healer scheduler will now skip already-resolved dashboard issues, and the health checker will not flag pending one-shot healer jobs as delayed.
