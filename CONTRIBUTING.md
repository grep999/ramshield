# Contributing to RamShield

Thank you for your interest in contributing! This document outlines the process and standards for contributing to RamShield.

## Ways to Contribute

- **Bug reports** — Found a crash, data race, or incorrect behavior? Open an issue with a minimal reproduction.
- **Feature requests** — Have an idea? Start a [Discussion](https://github.com/grep999/ramshield/discussions) first.
- **Code contributions** — Fix bugs, add features, improve performance, or enhance documentation.
- **Testing** — Run attack simulators, stress tests, or add test cases.
- **Documentation** — Improve README, add examples, clarify config options.

## Development Setup

### Prerequisites

- Rust 1.70+ (2021 edition) — [rustup.rs](https://rustup.rs/)
- Python 3.8+ — for attack simulators

### Clone & Build

```bash
git clone https://github.com/grep999/ramshield.git
cd ramshield/rs
cargo build --all-targets
cargo test
```

## Code Standards

- **Edition**: Rust 2021
- **Formatting**: `cargo fmt --all`
- **Linting**: `cargo clippy --all-targets -- -D warnings` (zero warnings)
- **Error handling**: `thiserror` for library, `anyhow` for application
- **Async**: Tokio full; never block in async context
- **Concurrency**: Prefer atomics/channels over locks; short guard lifetimes

## Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add Holt-Winters forecasting engine
fix: correct EWMA alpha calculation
perf: reduce batch lock contention by 40%
docs: update CLI reference
```

## Pull Request Process

1. Fork → feature branch → PR
2. Run full verification:
   ```bash
   cargo build --all-targets
   cargo clippy --all-targets -- -D warnings
   cargo test --all-targets
   cargo fmt --all -- --check
   ```
3. PR must include: clear title, description, linked issue
4. All CI checks must pass
5. Squash on merge

## PR Checklist

- [ ] All CI checks pass
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test` passes
- [ ] New code has tests
- [ ] Documentation updated
- [ ] No `unwrap()` in production paths
- [ ] Performance impact considered

## Testing

```bash
# Unit + integration
cargo test

# Attack simulation
python3 scripts/attack_sim_100k.py --events 1000000 --workers 64
```

## Security

Do not open public issues for security vulnerabilities. See [SECURITY.md](SECURITY.md).

## Getting Help

- [GitHub Discussions](https://github.com/grep999/ramshield/discussions)
- [Discord](https://discord.gg/ramshield)
- [Issues](https://github.com/grep999/ramshield/issues)

First-time contributors welcome! Look for `good first issue` labels.