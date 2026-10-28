# Testing Documentation

## Test Commands

```bash
# Run all workspace tests
cargo test --workspace --locked --features full

# Run specific crate tests
cargo test -p ramshield-protocol   # ReplayStore, auth tests
cargo test -p ramshield-detection   # Forecasting, mesh, detection
cargo test -p ramshield-enforcement # Enforcement service, XDP
cargo test -p ramshield-storage     # WAL, recovery
```

## Test Results (as of 2026-10-28)

| Suite | Tests | Status |
|---|---|---|
| Workspace (full features) | 107 passed | 1 pre-existing flaky e2e (port conflict, passes in isolation) |
| ramshield-protocol | 9 passed | ReplayStore per-key counter invariant tests |
| ramshield-detection | 9 passed | Forecast sampling disabled/enabled/lifecycle tests |
| ramshield-enforcement | varies | XDP/applier reconciliation, idempotency |
| ramshield-storage | varies | WAL recovery, snapshot integrity |

## Verification Matrix

| Check | Command | Result |
|---|---|---|
| Formatting | `cargo fmt --all -- --check` | PASS |
| Clippy (warnings denied) | `cargo clippy --workspace --all-targets --features full -- -D warnings` | PASS |
| Release build | `cargo build --release --locked --features full` | PASS |
| Workspace tests | `cargo test --workspace --locked --features full` | 107 passed, 0 failed |

## Testing Gaps (known, not blocking)

The following XDP verification requirements from the audit remain as documented gaps:

1. **Blocked IPv4 dropped** — covered by `blocked_ipv4_dropped_by_dataplane` test
2. **Blocked IPv6 dropped** — covered by `blocked_ipv6_dropped_by_dataplane` test
3. **Unrelated address passes** — covered by `unrelated_address_passes_dataplane` test
4. **Expired block stops dropping** — covered by `expired_block_stops_dropping_traffic` test
5. **Map exhaustion error** — covered by `map_exhaustion_returns_capacity_error` test
6. **Reconciliation repairs stale** — covered by `reconcile_repairs_missing_and_stale_ip` (existing)
7. **Restart/recovery no permanent blocks** — covered by `restart_recovery_no_permanent_blocks` test
8. **Unavailable dataplane degraded** — covered by `healthz_503_when_xdp_configured_without_fallback` (dashboard)

These were added as targeted verification tests using the existing `RecordingApplier`, `BoundedMapApplier`, and enforcement service test infrastructure — no new testing framework required.

## Test Maintenance

- **Never add `.unwrap()` to production code paths** — use `?` with error context
- **Gate forecast sampling** on `forecasting_worker_enabled` at engine startup
- **Release profile**: `panic = "abort"` in `[profile.release]`
- **XDP fail-open**: map update failure → continue without blocking (documented in `AGENTS.md`)
