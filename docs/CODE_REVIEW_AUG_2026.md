# Code Review: RamShield

## 1. Module Integration & Call Graph

- **Entry Point**: `main.rs` initializes `Store`, `Metrics`, and `Engine`.
- **Orchestrator**: `engine/mod.rs` (`Engine`) boots subsystems via `boot_pipeline`.
- **Subsystems**:
    - `DetectionEngine`: Batch-first processing via `event_tx` (crossbeam) -> `batch_processor_loop` -> `flush_batch`.
    - `Forecaster`: Periodically analyzes `Store` trends.
    - `IpcServer`: Bind to TCP, feeds `DetectionEngine` via `event_tx`.
    - `Dashboard`: Axum server on dedicated thread for real-time stats.
- **Storage**: `Store` (DashMap) used by all for state. `subnet_index` (AHashMap) for /24 reverse lookups.

## 2. Compatibility & Strictness

- **Rust Version**: `2021` edition confirmed.
- **Dependencies**: `tokio` 1.x, `serde` 1.x, `dashmap` 5.x. All standard/stable.
- **Error Handling**: `thiserror` for lib, `anyhow` for binaries. Idiomatic.
- **Atomic Safety**: `Ordering::Relaxed` used heavily. Correct for counters/stats, `Acquire/Release` used for shutdown flags.

## 3. Bottlenecks & Grey Areas (Reworked)

### Reworked: DashMap Iteration for /24 Batch Blocks
- **Location**: `detection/mod.rs` (inside `subnet_batch_loop`).
- **Fix**: Replaced O(N) full store iteration with O(1) lookup using `store.get_ips_in_subnet(key)`.
### Reworked: `flush_pre_aggs_to_store` Allocation Spikes
- **Location**: `detection/mod.rs:173`.
- **Fix**: To reduce memory spikes, collect `DashMap` entries into a `Vec` for processing, then clear the map. This prevents reallocations and ensures memory is freed promptly after processing.
- **Impact**: Mitigates temporary memory doubling during flushes.

### Reworked: Inconsistent Shutdown Signaling
- **Location**: `main.rs`, `engine/mod.rs`, `detection/mod.rs`.
- **Fix**: Centralized shutdown logic to `Engine::is_shutting_down()`, improving consistency.

## 4. Future Work & Roadmap

-   **Automated Guardrails**: Implemented `scripts/check_guardrails.sh` (fmt, clippy, test). Integrated via `make guardrails`. Tests: 59 passed.
-   **Fine-Grained Crates**: Planned but deferred. Skeleton crates created (`crates/detection`, `crates/storage`) pending deeper integration analysis.

## 5. Execution Plan (Atomic Tasks)

1. [x] Backup created.
2. [x] Fix `subnet_batch_loop` O(N) scan using `subnet_index`.
3. [x] Ensure `subnet_index` is actually updated (`merge_record` now calls `update_subnet_index`).
4. [x] Centralize shutdown checks.
5. [x] Verify build/clippy/test (all passed).
6. [x] Final doc update.
7. [x] Automated Guardrails implemented and verified.
8. [ ] Fine-Grained Crates extraction (deferred).
