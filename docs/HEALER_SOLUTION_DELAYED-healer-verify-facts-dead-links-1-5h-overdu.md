# Healer Solution — DELAYED-healer-verify-facts-dead-links-1-5h-overdu

**Issue ID:** `DELAYED-healer-verify-facts-dead-links-1-5h-overdu`  
**Category:** cron  
**Cycle:** 1

## What Changed

### 1. `error_healer.py` already skips already-fixed issues

The root scheduler was already guarded (lines 133–139 and 364–369) to skip issues whose `docs/HEALER_STATUS_{issue_id}.md` reports `Status: **FIXED**` or `Status: Resolved`. The underlying issue `facts-dead-links` was already fixed and verified, so the real fix needed is at the reporting layer, not the scheduler.

### 2. Fixed dead-link false positives in `health_check_repair.py`

Two stale one-shot healer jobs (`healer-verify-facts-dead-links` and `healer-solve-facts-dead-links`) were being reported as `DELAYED` because the health checker's dead-link detector was inconsistent with the `facts_collector.py` checker.

- **File:** `.github/scripts/health_check_repair.py`
- **Location:** `check_dead_links()` (around line 154)
- **Change:** Added inline-code-span stripping before the Markdown link regex:
  ```python
  content = re.sub(r'`[^`]*`', '', content)
  ```
  This matches the behavior in `facts_collector.py::check_local_links()` and prevents the false positives caused by `![demo](docs/assets/demo.gif)` inside backticks.

### 3. Removed inline-code link syntax from a leftover analysis file

- **File:** `docs/HEALER_ANALYSIS_DELAYED-healer-solve-facts-dead-links-1-6h-overdue.md`
- **Change:** Replaced the literal `` `![demo](docs/assets/demo.gif)` `` inside a sentence with a prose description, so both the updated checker and any other tooling will not see it as a link.

## Verification

Ran the health checker with Python warnings as errors:

```bash
python3 -W error .github/scripts/health_check_repair.py
```

Output:

```
Health check complete: 0 issues, 0 fixes → .../docs/HEALTH_CHECK.md
```

Confirmed:

- `docs/HEALTH_CHECK.md` now reports `## 2. Dead Links` ✅ No dead links found.
- `docs/ERRORS.md` no longer lists any dead-link entries.
- `docs/HEALER_STATUS_facts-dead-links.md` still shows `Status: **FIXED**`.
- `docs/FACTS.json` still contains `"dead_links": []`.

## Why This Fix

- **Minimal:** no risky refactors; only aligned the two dead-link checkers and removed one inline-code link literal.
- **Safe:** the reported `DELAYED` cron jobs were stale one-shot verify/solve jobs for an already-resolved issue, so the correct action is to stop the false reporting rather than re-dispatch a new healer chain.
- **Follows the documented maintenance pattern:** skip anchor-only links, deduplicate, and strip inline code spans before link extraction.

## Remaining Note

The stale one-shot healer cron jobs (`healer-verify-facts-dead-links` and `healer-solve-facts-dead-links`) may still exist in the cron list with `last_status=never`. Because the underlying issue is fixed and the health checker no longer reports them as `DELAYED`, they can be safely removed via `hermes cron list` followed by `hermes cron remove <id>` if desired. The `health_check_repair.py` one-shot tolerance introduced in the prior fix already handles past-due one-shot healer jobs gracefully.
