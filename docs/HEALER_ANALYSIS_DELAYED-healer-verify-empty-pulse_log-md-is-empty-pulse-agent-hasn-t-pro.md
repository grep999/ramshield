# Healer Analysis: DELAYED-healer-verify-empty-pulse_log-md-is-empty-pulse-agent-hasn-t-pro

**Cycle:** 1  
**Issue ID:** DELAYED-healer-verify-empty-pulse_log-md-is-empty-pulse-agent-hasn-t-pro  
**Category:** cron  
**Analyzed:** 2026-07-21 08:25 UTC

---

## Problem

A one-shot `healer-verify-*` temp job for an already-resolved pulse issue is stuck in the cron list with `last_status=never` and is reported as **1.5h overdue**. The underlying `PULSE_LOG.md` problem is fixed, but stale healer verify jobs are accumulating and being re-dispatched as new issues.

## Evidence

1. **The original pulse issue is already resolved.**
   - `docs/HEALER_STATUS_EMPTY-PULSE_LOG-md-is-empty-pulse-agent-hasn-t-pro.md` states: `Fixed? Yes`.
   - `docs/HEALER_SOLUTION_EMPTY-PULSE_LOG-md-is-empty-pulse-agent-hasn-t-pro.md` documents the fix: converted `ramshield-pulse` to a deterministic `no_agent=true` script.
   - `docs/PULSE_LOG.md` has fresh entries up to `Tue 21 Jul 08:21:11 UTC 2026`.
   - `docs/CRON_STATUS.json` lines 139–156 show `ramshield-pulse` is healthy: `last_status=ok`, `execution=completed`.

2. **Stale one-shot verify jobs remain in the cron list.**
   - `docs/ERRORS.md` line 8: `DELAYED: healer-verify-empty-pulse_log-md-is-empty-pulse-agent-hasn-t-pro — 1.7h overdue, last_status=never`.
   - `docs/HEALTH_CHECK.md` lines 7–10 repeat the same four DELAYED healer-verify alerts.
   - `docs/OPERATOR_LOG.md` lines 298–316, 330–348, etc. show these one-shot verify jobs being repeatedly scheduled with `rc=0` but never executing.
   - `docs/HEALER_DISPATCH.md` shows the latest dispatch created another `healer-verify-delayed-healer-verify-empty-pulse_log-md-is-empty` one-shot job at `2026-07-21T08:33:37+00:00` with `repeat:1`.

3. **The error healer keeps re-dispatching chains for these stale verify jobs.**
   - `.github/scripts/error_healer.py` lines 72–130 (`parse_errors_md`) generates issue IDs directly from the error body text. Because the error body begins with `DELAYED: healer-verify-...`, it produces a new ID that does not match the original issue (`EMPTY-PULSE_LOG-...`).
   - `.github/scripts/error_healer.py` lines 161–168 (`load_cycle_history`) only checks exact ID matches, so the new `DELAYED-...` ID returns cycle `0` and triggers a fresh analyze/solve/verify chain.
   - `.github/scripts/error_healer.py` has no guard to skip issues already marked `Fixed? Yes` in existing `docs/HEALER_STATUS_*.md` files.

4. **The health checker misclassifies one-shot healer jobs as overdue.**
   - `.github/scripts/health_check_repair.py` lines 75–91 (`check_cron_jobs`) flags any job whose `next_run` is in the past and `last_status != ok` as `DELAYED` after 0.5h.
   - There is no exemption for `repeat:1` one-shot healer jobs, nor any comparison against a grace window (`now - 15min`).

## Root Cause

The healer scheduler does not clean up or recognize resolved issues:
- Old one-shot `healer-verify-*` jobs linger in the cron list after the original issue is fixed.
- `error_healer.py` treats the DELAYED-stale-job error as a brand-new issue because the generated ID differs from the original.
- `health_check_repair.py` reports these lingering one-shot jobs as active DELAYED errors, which feeds back into the healer and creates more redundant temp jobs.

## Affected Files

- `docs/ERRORS.md` line 8 — surfaces the stale DELAYED alert
- `docs/HEALTH_CHECK.md` lines 7–10 — repeats the DELAYED healer-verify alerts
- `docs/HEALER_STATUS_EMPTY-PULSE_LOG-md-is-empty-pulse-agent-hasn-t-pro.md` — already shows the original issue fixed
- `docs/HEALER_DISPATCH.md` — latest dispatch that created another redundant verify job
- `docs/OPERATOR_LOG.md` — repeated scheduling of the same verify chain
- `.github/scripts/error_healer.py` lines 72–130, 161–168 — issue parsing and cycle history
- `.github/scripts/health_check_repair.py` lines 60–98 — cron job overdue detection

## Proposed Fix

1. **Skip resolved issues in `error_healer.py`.**
   Before dispatching a chain, check whether `docs/HEALER_STATUS_{original_id}.md` exists and contains `Fixed? Yes` / `Status: Resolved`. If so, skip dispatch and optionally delete the stale one-shot healer jobs.

2. **Clean up one-shot healer jobs after verification.**
   When a verify stage reports success, delete the three temp jobs (`healer-analyze-*`, `healer-solve-*`, `healer-verify-*`) from the cron list using `hermes cron remove job_id=<id>`.

3. **Tighten health-checker handling of one-shot jobs.**
   In `health_check_repair.py` `check_cron_jobs`, ignore `repeat:1` jobs until `next_run` is at least 15 minutes in the past, or exclude one-shot healer temp jobs entirely from overdue detection.

4. **Normalize issue IDs in `error_healer.py`.**
   Strip common prefixes such as `DELAYED:`, `STUCK:`, `PAUSED:`, and `healer-verify-` / `healer-solve-` / `healer-analyze-` before generating the issue ID, so stale healer alerts map back to the original issue and increment the cycle counter instead of starting over.

## Verification

After the fix:

```bash
hermes cron list | grep "healer-verify-empty-pulse_log"
# Expected: no stale one-shot verify jobs remain for the resolved pulse issue.

python3 .github/scripts/error_healer.py
# Expected: no new chain is dispatched for the already-fixed pulse issue.
```

---

*Next stage: solve.*
