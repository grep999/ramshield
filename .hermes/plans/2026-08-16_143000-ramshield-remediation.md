# RamShield Remediation Plan: Telemetry, Tracking, & IPC

**Goal**: Apply review findings from `audit/a_168a` (tracking, telemetry, module stats, serialization).

**Approach**:
1. Fix `TrafficCounters` rate accumulation in `storage/mod.rs` & `forecasting/mod.rs`.
2. Optimize telemetry in `metrics/mod.rs` (targeted `sys` refresh).
3. Wire `Engine` stats to `Metrics` in `engine/mod.rs`.
4. Integrate dropped event telemetry & manual block recording in `ipc/server.rs`.
5. Implement type-safe JSON serialization in `cli.rs`.

---

### Task 1: Update TrafficCounters (Tracking)
- Modify `src/storage/mod.rs`: Update `TrafficCounters` to use `get_and_reset_1s_rate()`.
- Verify `src/forecasting/mod.rs` uses correct RPS metric.

### Task 2: Optimize System Telemetry
- Modify `src/metrics/mod.rs`: Replace `sys.refresh_all()` with `sys.refresh_cpu()` and `sys.refresh_memory()`.

### Task 3: Wire Live Module Statistics
- Modify `src/engine/mod.rs`: Wire `Engine::get_module_stats` to `Metrics::get_module_stats_data`.

### Task 4: IPC & Dropped Event Telemetry
- Modify `src/ipc/server.rs`: Add dropped event accounting to `metrics.events_rejected` and `metrics.record_block`.

### Task 5: Type-Safe CLI Serialization
- Modify `src/cli.rs`: Use `serde_json::to_string` instead of raw string interpolation.

---

### Verification
- Run: `cargo test`
- Run: `cargo clippy -D warnings`
