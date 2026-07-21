# Healer Analysis — facts-MISSING-KEY-FACTS-json-missing-generated_at

## Issue
- MISSING KEY: FACTS.json missing 'generated_at'
- MISSING KEY: FACTS.json missing 'codebase'
- MISSING KEY: FACTS.json missing 'roadmap_open_tasks'
- MISSING KEY: FACTS.json missing 'backlog_remaining'

## Evidence

### 1. Current FACTS.json is actually complete
Reading `docs/FACTS.json` now shows all required keys present:

```json
{
  "generated_at": "2026-07-21T05:58:54Z",
  "codebase": { "rust_files": 27, "lines_of_code": 4513, "clippy_warnings": -1 },
  "roadmap_open_tasks": [ "Implement XGBoost threat scoring model", ... ],
  "backlog_remaining": 18,
  ...
}
```

So the error report is stale — it was generated while the file was transiently corrupted.

### 2. Health check log shows the exact failure mode
`docs/HEALTH_CHECK.md` (line 37) records:

```
🔧 FACTS regeneration error: Command '['python3', '/home/m/.../.github/scripts/facts_collector.py']' timed out after 30 seconds
```

When `health_check_repair.py` detects missing keys, it calls `regenerate_facts()` with a **30-second timeout**:

```python
# .github/scripts/health_check_repair.py:308-325
subprocess.run(
    ["python3", str(collector)],
    cwd=str(WORKSPACE), timeout=30,
    ...
)
```

### 3. The collector can need far more than 30 seconds
Inside `.github/scripts/facts_collector.py:76-92`, `count_clippy_warnings()` runs `cargo clippy` with its own **120-second timeout**:

```python
subprocess.check_output(
    ["cargo", "clippy", "--all-targets", "--message-format=json"],
    ... timeout=120
)
```

A full Rust build + clippy can easily exceed 30 seconds. The health check kills the collector mid-run, leaving a partially written `docs/FACTS.json` that lacks the later keys (`generated_at` is written near the end of the JSON object, so it is especially likely to be missing in a truncated file).

### 4. Non-atomic write exposes partial output
`facts_collector.py:274-276` writes directly to `docs/FACTS.json`:

```python
out = Path("docs") / "FACTS.json"
out.write_text(json.dumps(facts, indent=2), encoding="utf-8")
```

There is no temp-file + rename pattern. If the process is killed during the write, the file is left in a truncated, invalid state.

## Exact Files / Lines

| File | Lines | Problem |
|------|-------|---------|
| `.github/scripts/health_check_repair.py` | 308-325 | `regenerate_facts()` timeout is only 30s, while `cargo clippy` can take 120s. |
| `.github/scripts/facts_collector.py` | 76-92 | `count_clippy_warnings()` runs `cargo clippy` with 120s timeout. |
| `.github/scripts/facts_collector.py` | 274-276 | Writes FACTS.json directly (non-atomic), so a killed run leaves a truncated file. |

## Proposed Fix

1. **Raise the repair timeout**: Increase `regenerate_facts()` timeout in `health_check_repair.py` to at least 150 seconds (30s buffer above the 120s clippy timeout).

2. **Make FACTS.json write atomic**: In `facts_collector.py`, write to a temp file (`docs/FACTS.json.tmp`) and then `os.replace()` it to `docs/FACTS.json`. A killed run will leave the old valid file intact instead of a truncated one.

3. **Short-circuit clippy when unsafe**: Optionally skip clippy when the collector is invoked by the repair path (e.g. via an env var) to avoid heavy work during a quick health check.

4. **Regenerate FACTS.json once**: After the fix, run `.github/scripts/facts_collector.py` directly to ensure the file is fresh and complete, which will clear the stale error.

## One-line Summary
FACTS.json missing-key errors are caused by health_check_repair.py killing facts_collector.py with a 30s timeout during a 120s cargo clippy run, leaving the JSON file truncated and non-atomic.
