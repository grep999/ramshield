# Healer Status: facts-clippy-warnings

**Cycle:** 1
**Stage:** ANALYZE → complete
**Fixed? No** (analysis only — fix in SOLVE stage)

## Summary

FACTS.json reports `clippy_warnings: 21`. Root cause: 11 unique clippy lints across 6 files, each emitted twice (lib + tests) = 21 total diagnostic messages.

See `docs/HEALER_ANALYSIS_facts-clippy-warnings.md` for full breakdown.

## Next Action

SOLVE stage: apply `cargo clippy --fix` for 9 machine-applicable lints; manually fix 2 remaining (`result_unit_err`, `suspicious_open_options`).