# Healer Analysis — facts-clippy-warnings

## Issue
FACTS.json reports `"clippy_warnings": 21` in the facts_collector output, but `cargo clippy --all-targets` runs clean (0 warnings).

## Evidence
- **FACTS.json line 36** (from 2026-07-21T12:52:33Z run): `"clippy_warnings": 21`
- **Current FACTS.json line 36** (from 2026-07-21T12:55:33Z run): `"clippy_warnings": 0`
- **Manual cargo clippy run**: 0 warnings (exit code 0)
- **JSON clippy output grep**: 0 warning messages

## Root Cause
The `count_clippy_warnings()` function in `.github/scripts/facts_collector.py` (lines 113-129) parses JSON compiler messages and counts any message with `"level": "warning"`. However, the `--message-format=json` output includes **dependency build-script artifacts** (build.rs compilations for proc-macro2, quote, serde_derive, etc.) that emit compiler-artifact messages, not actual clippy lints on the project's own code.

The count was inflated because:
1. The function runs `cargo clippy --all-targets --message-format=json`
2. The JSON stream includes build-script compilation of **all dependencies** (100+ crates)
3. Some build scripts emit warnings (e.g., deprecated APIs in build.rs)
4. The collector counts **every** `"level": "warning"` in the entire JSON stream, not just warnings from the project's own crates

The **current run shows 0** because the dependency tree was already built (cached), so no build-script re-compilation occurred. On a clean build or after `cargo clean`, the count would spike again.

## Files / Lines
- `.github/scripts/facts_collector.py`: lines 113-129 (`count_clippy_warnings` function)
- `Cargo.toml`: 27 dependencies with build scripts (proc-macro2, quote, serde_derive, syn, etc.)

## Proposed Fix
Filter clippy warnings to only count messages where `package_id` matches the workspace crate (`ramshield@0.1.0`). In the JSON output, compiler messages include a `package_id` field like:
```
"package_id": "path+file:///home/m/vehicle_of_rationalism/ramshield/beta/rs#ramshield@0.1.0"
```

Change the filter from:
```python
if msg.get("reason") == "compiler-message" and msg.get("message", {}).get("level") == "warning":
```
to:
```python
if (msg.get("reason") == "compiler-message" 
    and msg.get("message", {}).get("level") == "warning"
    and "ramshield@" in msg.get("package_id", "")):
```

This restricts counting to warnings emitted by the workspace crate itself, ignoring dependency build-script noise.

## Verification
Run `python3 -W error .github/scripts/facts_collector.py` and verify `clippy_warnings` matches `cargo clippy --all-targets 2>&1 | grep -c "^warning:"` (should be 0).