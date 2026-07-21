# Healer Solution — DELAYED-healer-solve-facts-dead-links-1-6h-overdue

| Field | Value |
| --- | --- |
| Issue ID | DELAYED-healer-solve-facts-dead-links-1-6h-overdue |
| Category | cron |
| Cycle | 1 |
| Status | **FIXED** |
| Solved at | 2026-07-21T09:26:00Z |

## What Changed

### 1. Underlying issue already resolved

The root issue (`facts-dead-links`) was already fixed, verified, and marked `Status: **FIXED**` in `docs/HEALER_STATUS_facts-dead-links.md`. `docs/FACTS.json` confirmed `"dead_links": []`. The `DELAYED: healer-solve-facts-dead-links` entry was a **stale one-shot healer cron job**, not a new unresolved problem.

### 2. Fixed a lingering dead-link false positive in `.github/scripts/health_check_repair.py`

The health checker's dead-link detector still reported a false positive even after the previous one-shot fix, because the regex-based inline-code span removal was fragile and left behind the Markdown image link syntax inside multi-backtick code spans.

- **File:** `.github/scripts/health_check_repair.py`
| **Location:** `check_dead_links()` (around line 152)  
| **Change:** Replaced the simple regex `content = re.sub(r'\`[^\`]*\`', '', content)` with a proper Markdown-aware `strip_inline_code_spans()` helper that handles runs of 1–N backticks, including nested backticks inside multi-backtick code spans.  

This ensures text like `\`![demo](docs/assets/demo.gif)\`` (or `\`\` \`![demo](docs/assets/demo.gif)\` \`\``) is removed entirely before link extraction, matching the intent of `facts_collector.py::check_local_links()`.

## Verification

Ran the health checker with Python warnings as errors:

```bash
python3 -W error .github/scripts/health_check_repair.py
```

Output:

```
Health check complete: 0 issues, 0 fixes → /home/m/vehicle_of_rationalism/ramshield/beta/rs/docs/HEALTH_CHECK.md
```

Confirmed:

- `docs/HEALTH_CHECK.md` reports `## 2. Dead Links` ✅ No dead links found.
- `docs/ERRORS.md` no longer lists any dead-link entries.
- `docs/HEALER_STATUS_facts-dead-links.md` still shows `Status: **FIXED**`.
- `docs/FACTS.json` still contains `"dead_links": []`.

## Why This Fix

- **Minimal:** only aligned the inline-code handling in the health checker; no source files were re-edited.
- **Safe:** the underlying issue was already resolved; the remaining work was to stop the false reporting.
- **Follows the documented maintenance pattern:** strip inline code spans before link extraction, deduplicate, and skip anchor-only links.

## Remaining Note

The stale one-shot healer cron jobs (`healer-solve-facts-dead-links` and `healer-verify-facts-dead-links`) may still exist in the cron list with `last_status=never`. The underlying issue is fixed and the health checker no longer reports them as `DELAYED`, so they can be safely removed via:

```bash
hermes cron list
hermes cron remove <id>
```

This is a cron-layer cleanup, not a code fix.
