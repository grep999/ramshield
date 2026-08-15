# Contributing to RamShield

We welcome contributions. This document explains how to contribute effectively.

## Ways to Contribute

- **Code**: bug fixes, features, performance improvements
- **Documentation**: README, API docs, guides
- **Testing**: unit tests, integration tests, stress tests
- **Issues**: bug reports, feature requests, questions

## Development Setup

```bash
# Prerequisites: Rust 1.70+
rustup update stable

# Clone and build
git clone https://github.com/grep999/ramshield.git
cd ramshield/beta/rs
cargo build --all-targets --all-features
```

## Code Standards

- **Edition**: Rust 2024 (see `Cargo.toml`)
- **Formatting**: `cargo fmt --all` (enforced in CI)
- **Linting**: `cargo clippy --all-targets --all-features -- -D warnings` (zero warnings)
- **Testing**: `cargo test --all-targets --all-features` (all tests must pass)
- **Dependencies**: minimal, prefer stdlib; no new deps without justification

## Commit Format

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
type(scope): short description

Longer explanation if needed. Wrap at 72 chars.
```

Types: `feat`, `fix`, `refactor`, `docs`, `test`, `ci`, `chore`, `perf`

Examples:
```
feat(detection): add subnet aggregation to batch processor
fix(storage): correct TTL eviction race condition
docs(readme): add benchmark results table
```

## Branch Naming

- `feat/description` — new features
- `fix/description` — bug fixes
- `refactor/description` — code restructuring
- `docs/description` — documentation
- `ci/description` — CI/CD changes

## Pull Request Process

1. Fork and create a branch from `main`
2. Make changes with tests
3. Run full verification locally:
   ```bash
   cargo fmt --all --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets --all-features
   ```
4. Push and open PR against `main`
5. CI must pass (clippy, check, build, test)
6. PR will be reviewed; address feedback
7. Squash merge on approval

## Review Guidelines

- Does it solve the stated problem?
- Are types correct (ownership, borrowing, lifetimes)?
- Are errors returned as `Result` (not panics)?
- Is the hot path allocation-free?
- Are new dependencies justified?
- Do tests cover the change?
- Is documentation updated?

## Performance Changes

For performance-related PRs:
- Include `cargo bench` before/after
- Prefer `criterion` for statistical rigor
- Report allocations (`cargo llvm-lines`) and binary size (`cargo bloat`)

## Security

Report security issues privately via GitHub Security Advisories. Do not open public issues for vulnerabilities.

## Questions

Open a GitHub Discussion or issue with the `question` label.