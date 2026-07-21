# Healer Solution — facts-dead-links

**Issue ID:** facts-dead-links  
**Category:** deadlink  
**Cycle:** 1  
**Solved at:** 2026-07-21T08:11:00Z

## What Changed

### 1. Fixed dead-link checker false positive in `.github/scripts/facts_collector.py`

The regex in `check_local_links()` captured an empty `link_target` for anchor-only Markdown links like `[Section 7 — IPC Protocol](#7-ipc-protocol)` and reported them as dead links.

- Replaced the overly greedy regex with a stricter one that captures the path cleanly: `r'\[.*?\]\((?!https?://)([^#)]*)(?:#.*?)?\)'`
- The existing `if not link_target: continue` guard now correctly skips anchor-only links because the captured path is genuinely empty, not a leftover from an ambiguous group.
- Added a preprocessing step that strips inline code spans (`` `...` ``) before matching links. This prevents example/checklist text like `` `![demo](docs/assets/demo.gif)` `` from being treated as a real link.
- The function already deduplicates with `list(set(dead_links))` before returning.

### 2. Fixed two real broken relative links in `docs/HEALTH_DASHBOARD.md`

- `[See full report here](docs/DEPENDENCY_AUDIT.md)` → `[See full report here](DEPENDENCY_AUDIT.md)`
- `[See full roadmap here](docs/ROADMAP.md)` → `[See full roadmap here](ROADMAP.md)`

These links live inside `docs/`, so the extra `docs/` prefix made them point to `docs/docs/...`, which does not exist.

### 3. Left unchanged: `docs/drafts/demo-gif-guide.md`

The remaining reported dead link `docs/assets/demo.gif` appears only inside an inline code checklist example (`` `![demo](docs/assets/demo.gif)` ``). After stripping code spans in the checker, this is no longer reported as a dead link. No edit to the draft was needed.

## Verification

Re-ran the facts collector with Python warnings as errors:

```bash
python3 -W error .github/scripts/facts_collector.py
```

Output:

```
FACTS.json written: 22696 bytes, 0 TODOs, 22 roadmap tasks, 0 dead links detected
```

Confirmed `docs/FACTS.json` now contains `"dead_links": []`.

## Why This Fix

- Minimal: only adjusted the checker regex/ preprocessing and two real bad links.
- Safe: no refactors of unrelated code; preserves existing deduplication and absolute-path handling.
- Follows the documented maintenance pattern: skip anchor-only links, deduplicate, and ignore links embedded in code spans.
