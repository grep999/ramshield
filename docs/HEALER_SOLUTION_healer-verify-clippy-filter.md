# Healer Solution: Clippy Warnings Filter

## Issue
`facts_collector.py` `count_clippy_warnings()` was counting ALL compiler warnings including dependency crates (e.g., `proc-macro2`, `unicode-ident`, etc.) from crates.io, not just the workspace crate (`ramshield@`).

## Root Cause
The clippy warning counter did not filter by `package_id`. It counted every `level == "warning"` in the JSON output, which includes warnings from external dependencies.

## Fix Applied
Modified `count_clippy_warnings()` in `.github/scripts/facts_collector.py` to filter warnings by `package_id` containing `"ramshield@"`.

```python
if (msg.get("reason") == "compiler-message"
    and msg.get("message", {}).get("level") == "warning"
    and "ramshield@" in msg.get("package_id", "")):
    warnings += 1
```

## Verification
- `python3 -W error .github/scripts/facts_collector.py` → `clippy_warnings: 0`
- `python3 -W error .github/scripts/health_check_repair.py` → `0 issues, 2 fixes`

## Healer Job Status
The healer job (`healer-verify-clippy-filter`) was a stale one-shot cron job stuck due to the unpinned-LLM config-drift spend-guard. The underlying issue is now fixed in the source code. The cron job itself remains stuck but is harmless — the fix is already in the repo.

## Cron-Layer Remediation (not executed from solve stage)
To actually remove the stuck healer cron job:
```
hermes cron remove job_id=healer-verify-clippy-filter
```
Or pin it to prevent future spend-guard skips:
```
hermes cron update job_id=healer-verify-clippy-filter provider=<current> model=<current>
```