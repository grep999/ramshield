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

- The tracker consumes completed per-IP batch-rate samples; it does not enforce the upstream bucket cadence. The flush pipeline supplies `now` as the timestamp, so cadence is the configured flush cadence rather than a guaranteed one-second bucket.
- `TrafficContext::observed_at_ns` and the sample timestamp must use the same monotonic clock domain. The current `merge_record` call site passes no forecast context; contextual modulation is covered by detector-level tests but is not activated in this runtime path.
- The new `IpRecord` fields use `#[serde(default)]` for old serialized records. All known Rust struct literals in storage, detection, enforcement, checkpoint tests, and benchmarks were updated to initialize the fields.
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

The adaptive detector is now wired into `DetectionEngine::merge_record` through `update_adaptive_detector`. Its state is persisted in `IpRecord` using independent fields for adaptive baseline, CUSUM, accepted sample count, variance, timestamp, recovery count, and mode. The pre-existing detector fields and gates remain independent and continue to run, so the prototype adds an adaptive breach signal without reinterpreting legacy CUSUM state. Runtime integration regression tests cover persistence across updates and atomic rejection of repeated timestamps / invalid rates.

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
