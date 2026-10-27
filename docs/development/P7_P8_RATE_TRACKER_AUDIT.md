# P7/P8 Adaptive Rate Tracker — Integration Audit

**Status:** source changes committed on `fix/p7-p8-adaptive-rate-tracker`; runtime verification pending.  
**Scope:** integrate the adaptive per-IP CUSUM tracker without removing the existing `rate_tracker` helper API.

## Integration decisions

1. Preserve the existing functions in `crates/ramshield-detection/src/rate_tracker.rs` (`ewma`, `cusum_step_capped`, `cusum_fired`, `ewma_alpha_slow`, `is_exceeded`, and pulse tracking). The detector is appended as additive API; existing callers and benchmark imports are retained.
2. Add `ramshield-forecasting` as a direct dependency of `ramshield-detection`, because the new `TrafficContext` consumes the public `Hypothesis` enum. The dependency graph is acyclic in the current workspace: forecasting depends on config, storage, metrics, and types, not detection.
3. Update `Cargo.lock` to record the new direct dependency.
4. Keep the new detector tests in `rate_tracker::adaptive_tests` so they do not collide with the existing `rate_tracker::tests` module.
5. Use `checked_add` for the sample counter and return `InvalidInternalSampleCount` before state mutation if it overflows.
6. Reset accumulated CUSUM and recovery progress after a timestamp gap greater than `max_gap_tolerance_ns`. A long gap breaks a consecutive-recovery sequence.

## Corrective logic covered

- Timestamp zero is accepted for the first sample; all later timestamps must strictly increase.
- Profiles reject non-finite/invalid thresholds, floors, alpha, clear ratio, and zero warmup/recovery counts.
- Context confidence is validated; stale or future-dated context is not applied.
- Advisory context scaling cannot push `k` or `h` below configured floors.
- Arithmetic results are checked before the commit section; a numerical failure must leave the entire `PerIpRecord` unchanged.
- Breach recovery requires consecutive eligible samples. A burst or excessive timestamp gap resets recovery progress.
- Regression tests cover timestamp replay, invalid profiles, variance overflow, threshold-floor protection, numerical-failure atomicity, gap behavior, and sample-count overflow.

## Operational/integration caveats

- Adaptive CUSUM now receives only completed one-second buckets. Per-IP counts from sub-second flushes are accumulated in `IpRecord`; the previous bucket is submitted when the next bucket begins. Empty buckets are not synthesized when an IP has no flushes, so the gap policy—not fabricated zero samples—governs long inactivity.
- Forecasting publishes its Bayesian classification through an optional callback. The daemon wires that callback to an `ArcSwapOption<TrafficContext>` on `DetectionEngine`; the per-IP hot path reads a snapshot without a forecasting mutex. Both sides use the shared process-local monotonic clock in `ramshield-types`.
- During a fresh, confident `FlashCrowd` classification, the adaptive detector's context-scaled decision takes precedence over static legacy gates. The adaptive breach signal remains authoritative.
- The variance Serde default is positive (`0.5`, matching the default detector profile floor); the migration regression test removes all adaptive fields from a serialized legacy record and checks safe defaults.
- All known Rust struct literals in storage, detection, enforcement, checkpoint tests, and benchmarks were updated for both adaptive detector and bucket state.
- Review exhaustive matches on `DetectorError` in downstream code.
- Confirm that the detector's `Breached -> Monitoring` transition and CUSUM reset match enforcement expectations before merge.

## Commit sequence

1. `build(detection): declare forecasting dependency for adaptive rate tracker`
2. `feat(detection): add validated adaptive CUSUM rate tracker`
3. `build: update lockfile for detection forecasting dependency`
4. `docs: record P7/P8 rate tracker integration audit`
5. `build: keep lockfile dependencies in canonical order`
6. `feat(storage): persist adaptive detector state per IP`
7. `feat(detection): wire adaptive CUSUM into per-IP runtime decisions`
8. `fix(storage): isolate adaptive state from legacy detector counters`
9. `test(detection): verify adaptive runtime state persistence and atomic rejection`
10. Follow-up commits initialize adaptive fields in all known record literals, preserve the legacy detector alongside the new signal, and document persisted state.

The adaptive detector is wired into `DetectionEngine::merge_record` through a one-second quantization gate and `update_adaptive_detector`. Its baseline, CUSUM, sample count, variance, timestamp, recovery count, mode, and pending bucket are persisted in `IpRecord`; the legacy detector state remains independent. The forecasting-to-detection bridge is connected in daemon boot. Integration tests cover state persistence, rejected updates, runtime breach signaling, one-second quantization, clock regression, and FlashCrowd precedence.

## Verification status

**Not run in this environment.** Do not treat this branch as verified or merge-ready until these commands pass in a Rust-enabled checkout:

```sh
cargo fmt --check
cargo test -p ramshield-detection --lib rate_tracker::adaptive_tests
cargo test -p ramshield-detection --lib adaptive_integration_tests
cargo clippy -p ramshield-detection --all-targets --all-features -- -D warnings
cargo test --workspace
```

Also inspect downstream `DetectorProfile` struct literals and exhaustive `DetectorError` matches, then review the final diff and CI results.


## Acceptance checklist from the architectural review

- [x] D-01 through D-06 detector invariants (unit tests in `rate_tracker.rs`).
- [x] Positive Serde migration default and legacy-record migration regression.
- [x] One-second bucket accumulation and completed-bucket-only adaptive updates.
- [x] Lock-free forecasting context snapshot bridge wired in daemon boot.
- [x] FlashCrowd contextual decision is not overridden by static legacy gates.
- [ ] Criterion or allocator-instrumented evidence proving zero heap allocations per steady-state IP evaluation.
- [x] Labeled macro replay suite added for benign, FlashCrowd, volumetric, slow-ramp, and pulsing traces (execution result still pending CI).
- [ ] Full CI green, including workspace Clippy and tests.
- [ ] Miri run for the detector unit tests (if supported by the configured toolchain).

The source changes and deterministic replay tests are present, but replay results still need to be observed in CI. Allocation evidence, green full CI, and Miri remain explicit merge gates. Do not promote this PR to merge-ready until those gates are evidenced by CI/bench results.
