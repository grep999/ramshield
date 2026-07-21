# Healer Analysis — facts-dead-links

**Issue ID:** facts-dead-links  
**Category:** deadlink  
**Reported by:** FACTS.json collector  
**Cycle:** 1

## Problem

`docs/FACTS.json` reports one dead local link:

```
"DOCUMENTATION.md: Broken link to ''"
```

This is a **false positive** from the link checker in `.github/scripts/facts_collector.py`, not an actual broken link in `docs/DOCUMENTATION.md`.

## Evidence

1. **`docs/DOCUMENTATION.md` line 262** contains a valid in-document anchor link:

   ```markdown
   See [Section 7 — IPC Protocol](#7-ipc-protocol).
   ```

   The heading `## 7. IPC Protocol` exists on line 311, so the anchor is valid.

2. **`facts_collector.py::check_local_links()`** (lines 174–214) uses the regex on line 186:

   ```python
   re.finditer(r'\[.*?\]\((?!https?://)(.*?)(?:#.*?|\))\)', content)
   ```

   For `[Section 7 — IPC Protocol](#7-ipc-protocol)`, the captured group only gets the text before `#`, which is empty. The function then does `link_target = match.group(1).split('#')[0]`, producing `''`, and reports it as a dead link because `''` is neither a file nor in `all_docs`.

3. `health_check_repair.py` already documents the correct fix (skip anchor-only links and deduplicate), but `facts_collector.py` was not updated.

## Exact Files / Lines

| File | Line(s) | Description |
|------|---------|-------------|
| `.github/scripts/facts_collector.py` | 174–214 | `check_local_links()` generates false positives for anchor-only links |
| `.github/scripts/facts_collector.py` | 186 | Regex captures empty `link_target` for `[text](#anchor)` links |
| `docs/DOCUMENTATION.md` | 262 | Valid anchor link that is misreported as dead |

## Proposed Fix

Update `check_local_links()` in `.github/scripts/facts_collector.py` to:

1. **Skip anchor-only links** (`link_target == ''`). In-document anchors are not dead links in the same-file sense.
2. **Deduplicate** results using a `set` before returning.
3. Optionally validate that the anchor exists in the same file, but the immediate fix is to skip empty targets.

After the fix, re-run the facts collector and verify `docs/FACTS.json` reports `"dead_links": []`.
