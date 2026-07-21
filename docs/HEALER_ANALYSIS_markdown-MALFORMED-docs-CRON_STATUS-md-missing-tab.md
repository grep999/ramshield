# Healer Analysis — markdown-MALFORMED-docs-CRON_STATUS-md-missing-tab

## Issue
- MALFORMED: docs/CRON_STATUS.md missing table alignment row
- MALFORMED: docs/FACTS.json missing generated_at timestamp

## Problem

### 1. CRON_STATUS.md table alignment mismatch
The health checker in `.github/scripts/health_check_repair.py` does a literal substring check for `| :--- |` when validating `docs/CRON_STATUS.md`:

```python
# .github/scripts/health_check_repair.py:177
("docs/CRON_STATUS.md", "| :--- |", "table alignment row"),
```

However, `.github/scripts/cron_status_collector.py` generates markdown tables with colon-less alignment rows (`|-------|-------|`):

```markdown
# docs/CRON_STATUS.md:13-14
| State | Count |
|-------|-------|
```

These tables render correctly in Markdown, but the literal checker flags the file as malformed.

### 2. FACTS.json intermittently loses generated_at
`docs/FACTS.json` currently on disk has a valid `generated_at` timestamp, but the repair loop intermittently truncates it. The root cause is a timeout mismatch:

- `.github/scripts/facts_collector.py:76-92` runs `cargo clippy --all-targets --message-format=json` with a 120-second timeout.
- `.github/scripts/health_check_repair.py:311-325` calls the collector with only a 30-second timeout.

When repair kills the collector after 30s, the JSON write is interrupted and `generated_at` (and other keys) are missing until the next successful run.

Additionally, `.github/scripts/facts_collector.py:274-276` writes directly to `docs/FACTS.json`, so a killed run leaves a partially written file on disk.

## Evidence

- `docs/CRON_STATUS.md:13-14` uses `|-------|-------|` instead of `| :--- | :--- |`.
- `docs/FACTS.json:2` currently shows `"generated_at": "2026-07-21T06:22:19Z"`, but `docs/ERRORS.md` reports it missing, indicating intermittent regeneration failure.
- `docs/HEALTH_CHECK.md` previously logged: `FACTS regeneration error: ... timed out after 30 seconds`.

## Exact Files / Lines

| File | Lines | Problem |
|------|-------|---------|
| `.github/scripts/health_check_repair.py` | 177 | Literal check for `| :--- |` is too strict. |
| `.github/scripts/cron_status_collector.py` | 23-30 | Emits alignment rows without colons. |
| `.github/scripts/facts_collector.py` | 76-92, 274-276 | 120s clippy timeout; non-atomic JSON write. |
| `.github/scripts/health_check_repair.py` | 311-325, 420-424 | Calls collector with 30s timeout, less than clippy step. |

## Proposed Fix

1. **Fix CRON_STATUS.md alignment**: Update `cron_status_collector.py` to emit `| :--- | :--- |` style alignment rows so the literal checker passes. Alternatively, relax `health_check_repair.py` to accept any `|[-:]+|` delimiter row.

2. **Atomic FACTS.json write**: Update `facts_collector.py` to write to `docs/FACTS.json.tmp` and then `os.replace()` to `docs/FACTS.json`, preventing half-written files when killed.

3. **Fix timeout mismatch**: Either raise repair-loop timeouts in `health_check_repair.py` to at least 150 seconds, or add a `FAST_MODE` / env flag to `facts_collector.py` that skips the slow `cargo clippy` step when invoked from repair.

## One-line Summary
CRON_STATUS.md is flagged malformed because the checker expects `| :--- |` while the collector emits colon-less alignment rows; FACTS.json intermittently loses `generated_at` because repair kills the clippy-heavy collector after 30s and writes the JSON file non-atomically.
