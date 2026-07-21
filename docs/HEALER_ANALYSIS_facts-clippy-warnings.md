# Healer Analysis: facts-clippy-warnings

**Issue:** FACTS.json reports `clippy_warnings: 21`
**Root cause:** 11 unique clippy lint warnings across 6 source files. Each warning emits twice (once per compilation unit — lib + tests), totaling 21 diagnostic messages.

## Evidence

Verified via `cargo clippy --all-targets --message-format=json`. All 21 warnings confirmed present on current HEAD (`b056753`).

## Warnings by File

| # | File | Line | Clippy Lint | Severity | Auto-fixable |
|---|------|------|-------------|----------|--------------|
| 1 | `src/config.rs` | 17 | `derivable_impls` | Style | Yes — replace manual `impl Default` with `#[derive(Default)]` |
| 2 | `src/detection/mod.rs` | 47 | `manual_div_ceil` | Style | Yes — `(bit_count + 63) / 64` → `bit_count.div_ceil(64)` |
| 3 | `src/detection/mod.rs` | 119 | `result_unit_err` | Design | No — `Result<(), ()>` needs a real error type or `anyhow::Result` |
| 4 | `src/detection/mod.rs` | 346 | `assign_op_pattern` | Style | Yes — `x = x / 2` → `x /= 2` |
| 5 | `src/dns/mod.rs` | 35 | `new_without_default` | Idiom | Yes — add `impl Default for DnsMonitor` delegating to `new()` |
| 6 | `src/dns/forecasting/mod.rs` | 150 | `assertions_on_constants` | Correctness | Yes — remove `assert!(true)` (no-op) |
| 7 | `src/forecasting/mod.rs` | 220 | `manual_clamp` | Style | Yes — `.max(1).min(50)` → `.clamp(1, 50)` |
| 8 | `src/storage/blob_store.rs` | 24 | `suspicious_open_options` | Correctness | Maybe — needs `.truncate(true)` or `.truncate(false)` decided by intent |
| 9 | `src/storage/wal.rs` | 96 | `unnecessary_map_or` | Style | Yes — `.map_or(false, ...)` → `.is_some_and(...)` |
| 10 | `src/storage/mod.rs` | 169 | `unnecessary_map_or` | Style | Yes — `.map_or(false, ...)` → `.is_some_and(...)` |
| 11 | `src/storage/mod.rs` | 273 | `unnecessary_map_or` | Style | Yes — `.map_or(false, ...)` → `.is_some_and(...)` |

## Why FACTS.json Counts 21

`facts_collector.py:113-129` counts every `compiler-message` JSON line with `level == "warning"`. Cargo clippy emits each lint once per compilation target (lib crate + integration tests). There are ~2 targets, so 11 unique lints × ~2 = 21 total messages.

**This is not a bug in the collector.** The count accurately reflects the number of diagnostic messages produced. The underlying issue is 11 actual clippy warnings in the source code.

## Proposed Fix

Apply `cargo clippy --fix --allow-dirty --allow-staged` for all machine-applicable lints (9 of 11). Manually fix the remaining 2:

1. **`result_unit_err` (detection/mod.rs:119):** Replace `Result<(), ()>` with `Result<(), anyhow::Error>` or a custom `DetectionError`. Depends on whether callers match on `()`.
2. **`suspicious_open_options` (blob_store.rs:24):** Determine if blob_store writes are overwrite or append, then add the explicit `.truncate(...)` call.

After fixes, verify:
```bash
cargo clippy --all-targets -- -D warnings
# Expected: 0 warnings
```

The FACTS.json count will drop to 0 on next collector run.
