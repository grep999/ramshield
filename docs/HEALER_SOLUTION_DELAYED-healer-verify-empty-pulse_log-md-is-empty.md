# Healer Solution: DELAYED-healer-verify-empty-pulse_log-md-is-empty

**Cycle:** 1  
**Issue ID:** DELAYED-healer-verify-empty-pulse_log-md-is-empty-pulse-agent-hasn-t-pro  
**Category:** cron  
**Solved:** 2026-07-21 09:07 UTC

---

## Diagnosis

The `DELAYED: healer-verify-empty-pulse_log-md-is-empty-pulse-agent-hasn-t-pro` alert was a stale one-shot healer job, not an active project failure.

Evidence:

1. **Original pulse issue was already resolved.**
   - `docs/HEALER_STATUS_EMPTY-PULSE_LOG-md-is-empty-pulse-agent-hasn-t-pro.md` shows `Fixed? Yes`.
   - `docs/HEALER_SOLUTION_EMPTY-PULSE_LOG-md-is-empty-pulse-agent-hasn-t-pro.md` documents that `ramshield-pulse` was converted to a deterministic `no_agent=true` script.
   - `docs/PULSE_LOG.md` contains valid pulse entries.

2. **Stale one-shot healer jobs remained in the cron list.**
   - `hermes cron list` showed multiple `healer-analyze-`, `healer-solve-`, and `healer-verify-` jobs for the resolved pulse issue with `last_status=never` and `next_run` hours in the past.
   - These jobs had been created by earlier `error_healer.py` runs but never executed (likely skipped by the inference config-drift spend-guard), so they accumulated and were reported as DELAYED.

3. **The healer scheduler kept re-dispatching the stale alert as a new issue.**
   - `error_healer.py` generated issue IDs directly from the `DELAYED: healer-verify-...` error body, producing an ID that did not match the original `EMPTY-PULSE_LOG-...` issue.
   - Without normalization, the healer could not see that the underlying issue was already fixed, so it started a fresh analyze/solve/verify chain every cycle.

## Fix Applied

### 1. Cleaned up stale one-shot healer jobs

Removed all lingering temp jobs for the resolved pulse issue from the cron list:

```bash
cd /home/m/vehicle_of_rationalism/ramshield/beta/rs
hermes cron remove 4bb95a7fa449   # healer-solve-delayed-healer-verify-empty-pulse_log-md-is-empty
hermes cron remove a1adfbf087d6   # healer-verify-delayed-healer-verify-empty-pulse_log-md-is-empty
hermes cron remove fd9071fb8b4b   # healer-analyze-delayed-healer-verify-empty-pulse_log-md-is-empty
hermes cron remove fa1b7f9f8fad   # healer-solve-delayed-healer-verify-empty-pulse_log-md-is-empty
hermes cron remove 34dec99b42ef   # healer-verify-delayed-healer-verify-empty-pulse_log-md-is-empty
```

Result: `hermes cron list | grep empty-pulse_log` returns no matches.

### 2. Patched `error_healer.py` to normalize issue IDs

Added `normalize_issue_id()` that strips common alert prefixes (`DELAYED:`, `STUCK:`, `PAUSED:`, `STALE ONESHOT:`) and nested healer stage prefixes (`healer-analyze-`, `healer-solve-`, `healer-verify-`). This maps stale healer alerts back to the original issue ID so the existing `Fixed? Yes` status file is recognized.

Also made `load_cycle_history()` and `is_already_fixed()` match status files case-insensitively, because older status files use uppercase IDs (e.g. `EMPTY-PULSE_LOG`) while normalized IDs are lowercase.

### 3. Tightened `health_check_repair.py` one-shot detection

In `check_cron_jobs()`, the stale-job detector now also treats any job whose name starts with `healer-analyze-`, `healer-solve-`, or `healer-verify-` as a one-shot healer job, even if the schedule text is unusual. This prevents future stale healer jobs from being misreported as `DELAYED` system cron errors.

## Files Touched

- `.github/scripts/error_healer.py`
  - Added `normalize_issue_id()`.
  - Updated issue parsing to use normalized IDs.
  - Made `load_cycle_history()` and `is_already_fixed()` case-insensitive.
- `.github/scripts/health_check_repair.py`
  - Extended one-shot detection to include `healer-*` job name prefixes.
- `docs/HEALER_SOLUTION_DELAYED-healer-verify-empty-pulse_log-md-is-empty.md`
  - This file.
- `docs/HEALER_STATUS_DELAYED-healer-verify-empty-pulse_log-md-is-empty.md`
  - Verification/status file.

## Why This Fix Is Safe

- No source code or project functionality was changed; only automation scripts were adjusted.
- The stale jobs were explicitly removed rather than left to accumulate.
- Normalization is conservative: it only strips known healer prefixes and status labels, and falls back to a sanitized version of the original body otherwise.
- Case-insensitive status matching is backward-compatible with existing `HEALER_STATUS_*.md` files.

## Verification Commands

```bash
cd /home/m/vehicle_of_rationalism/ramshield/beta/rs
python3 -W error -m py_compile .github/scripts/error_healer.py .github/scripts/health_check_repair.py
python3 -W error .github/scripts/error_healer.py
python3 -W error .github/scripts/health_check_repair.py
hermes cron list | grep -i "empty-pulse_log" || echo "No stale empty-pulse_log healer jobs"
```

Expected result: syntax checks pass, the healer no longer dispatches a chain for the resolved pulse issue, the health check cron section shows `✅ All jobs healthy.`, and no stale `healer-*-empty-pulse_log` jobs remain.

---

*Solver stage completed.*
