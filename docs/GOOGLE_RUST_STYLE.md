# Rust style and refactoring policy

RamShield follows the principles in the [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html), with the repository's existing CI and Rust edition taking precedence where they differ.

This is a project policy, not a claim that every Google recommendation is mandatory for every Rust project.

## Required checks

Run these before merging a Rust change:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked --features full
cargo test --workspace --locked --all-features
```

The full CI workflow also runs dependency, build, ABI, and platform-specific checks. Use CI as the authoritative result when the local environment cannot reproduce all targets.

## Design rules

- Prefer named structs over positional tuples when fields have domain meaning or multiple values of the same type.
- Borrow the value actually needed. Do not add `&Arc<T>` to a function that only needs `&T`; keep `Arc` ownership at the sharing boundary.
- Keep public APIs and crate boundaries small. Make ownership, error handling, and mutation explicit.
- Use standard library types and established workspace dependencies before introducing another dependency.
- Keep performance-sensitive paths allocation-conscious, but require benchmarks or profiling evidence before adding complexity.
- Prefer checked arithmetic for lengths, offsets, and externally controlled values used in memory or packet parsing.
- Keep unsafe blocks small. Each unsafe block must have a nearby `SAFETY:` comment that explains the invariant relied upon, not merely restate the operation.
- Do not suppress lints broadly. Any `allow` should be local, justified, and ideally paired with a test or design note.
- Preserve observable behavior unless a change explicitly fixes a documented bug or security issue. Add regression tests for such fixes.
- Avoid unrelated cleanup in security-sensitive patches. Make reviewable, behavior-bounded changes.

## Refactoring strategy

RamShield is a multi-crate network defense daemon. A rewrite must not be performed as one unreviewable replacement. Refactor one architectural seam at a time, preserving the public contract and running the narrowest relevant tests before the workspace gates.

Recommended order:

1. Types and crate-level API contracts.
2. Storage, WAL, checkpoint, and recovery invariants.
3. Detection inputs, state transitions, and decision outputs.
4. Enforcement interfaces and XDP boundary validation.
5. Native packet ingest and protocol parsing.
6. IPC, configuration, and operational interfaces.
7. Metrics, analytics, forecasting, and optional mesh functionality.
8. Deployment documentation and end-to-end acceptance tests.

For every slice, record the behavior being preserved, add or update regression tests, run formatting and linting, and inspect the diff for unrelated changes. Do not merge until required CI checks pass.

## Scope note

This policy does not assert that the codebase has been fully rewritten or audited. It defines the standard for the dedicated refactoring branch and subsequent changes.
