# Audit Branch Recovery Plan

**Target branch:** `audit-branch`
**Operating rule:** one bounded change per fixer pass; reviewer independently inspects the exact resulting commit and actual gate output.
**No merge, force-push, or changes to `kiddo_fix` / existing PRs as part of this plan.**

## Goal

Make the core build and tests reproducible first, then make mesh an optional, independently testable subsystem, validate it in isolation, and only then reconnect it to enforcement.

"Error-free" means no known blocking defects and all agreed checks passing on the same exact commit. A source review or successful partial test is not a substitute for the full gate.

## Current baseline (2026-10-10)

- `master`: `821eab72e93e584cd6835d1f95171d5d380daaec`
- `audit-branch` before the CI-gate change: `f0402b0613dfa1be676ef36e13366b0036c5ecc0`
- `audit-branch` was 15 commits ahead and 0 behind master, with 21 changed paths.
- No GitHub Actions runs were returned for the previous audit head or for the audit branch. The CI workflow previously filtered push and pull-request events to `main`, `develop`, and `master`; the branch filter is being amended to include `audit-branch`.
- The execution environment used for the previous pass had no local Rust toolchain/checkout, so no compile, test, rustfmt, or Clippy pass is asserted here.

## Stage 1 — Establish an executable baseline

1. Run the branch-specific CI on the exact audit head.
2. Capture the first failure from each job; do not patch based on guessed compiler errors.
3. Classify each failure as core build, test/correctness, feature-boundary, or environment/toolchain.
4. Preserve current security regression tests. Fix compile blockers in the smallest scope possible.
5. Re-run all affected focused tests, then the full gate on the resulting exact SHA.

**Exit criteria:** `cargo fmt --all -- --check`, workspace check/build, workspace tests, and Clippy pass on the same SHA, with CI logs linked in the handoff.

## Stage 2 — Make mesh independently buildable and opt-in

The initial source inspection shows the root package currently depends on `ramshield-mesh` unconditionally, imports mesh types from `src/engine/mod.rs` unconditionally, and the enforcement crate also depends on mesh unconditionally. Adding only a root feature flag would therefore be incomplete.

1. Introduce a named `mesh` Cargo feature and make the root mesh dependency optional.
2. Add a matching optional dependency/feature boundary in `ramshield-enforcement`; gate mesh-only fields, methods, modules, and boot wiring consistently.
3. Keep the mesh crate in the workspace so it can be tested independently even when the root feature is disabled.
4. Define disabled-feature behavior explicitly: mesh configuration enabled in a build without the feature must fail with a clear startup/configuration error, not silently pretend clustering is active.
5. Preserve core enforcement semantics when mesh is disabled. Avoid unrelated CRDT or wire-protocol redesign.

**Exit criteria:** both core-only and mesh-enabled builds compile; the core-only tests do not link or start mesh; mesh unit tests can run directly.

## Stage 3 — Validate mesh in isolation

Add/retain focused tests for:
- frame exactly at the maximum size and one byte over it, including delimiter arriving in a later read;
- truncated frame, malformed JSON, invalid HMAC, stale/future timestamp, and self-originated messages;
- inbound connection semaphore limits and bounded queue behavior;
- outbound frame size and bounded concurrent peer sends;
- cancellation, listener failure, and shutdown behavior;
- CRDT merge/idempotence and bounded anti-entropy snapshots.

Check that attacker-controlled lengths are rejected before unbounded allocation. Ensure all spawned tasks have an explicit lifecycle or a documented bounded runtime lifetime.

**Exit criteria:** mesh crate focused tests pass with no network privileges required; integration tests use loopback and deterministic time/test seams where feasible.

## Stage 4 — Reintegrate mesh behind a narrow boundary

1. Keep mesh transport and CRDT logic inside `ramshield-mesh`.
2. Connect it to enforcement through a small adapter/command channel; do not hold checkpoint/WAL locks across network awaits.
3. Make mesh start only when both the binary feature and runtime configuration enable it.
4. Verify mesh bind/startup errors propagate clearly; shutdown is bounded and joined/cancelled.
5. Verify core-only behavior and mesh-enabled behavior independently.

**Exit criteria:** core-only, mesh-enabled, and full workspace gates all pass on one reviewed SHA. Independent reviewer records APPROVE; otherwise fix findings and repeat.

## Required acceptance commands

Run in a clean checkout at the exact candidate SHA:

```bash
cargo fmt --all -- --check
cargo check --workspace --locked --all-targets
cargo build --workspace --locked --all-targets
cargo test --workspace --locked
cargo clippy --workspace --locked --all-targets -- -D warnings

# Core without optional mesh (after feature isolation)
cargo check -p ramshield --no-default-features --locked
cargo test -p ramshield --no-default-features --locked

# Mesh in isolation
cargo test -p ramshield-mesh --locked

# Full production feature set
cargo check --workspace --locked --all-targets --features full,mesh
cargo test --workspace --locked --features full,mesh
cargo clippy --workspace --locked --all-targets --features full,mesh -- -D warnings

git diff --check origin/master...HEAD
```

Adjust package/feature commands only after confirming the actual Cargo feature graph; record any command that is not applicable and why. Do not report a gate as passing without its exit code and output.

## Handoff contract

Every audit/fixer/reviewer handoff includes: base SHA, exact head SHA, one selected action, changed paths, regression test, commands actually run with results, workflow/job URLs and status, unresolved risks, and the next responsible stage. No merge until the exact SHA has passing required gates and independent approval.
