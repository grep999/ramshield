# AGENTS.md - RamShield Project Context & Rust Standards

This file defines the project anatomy and coding standards that AI coding assistants MUST follow when working on this repository.

## 1. Tech Stack & Environment

- **Project Type**: Rust Library/Application (Rust 2021 Edition)
- **Primary OS Target**: Linux Mint (Debian/Ubuntu-based)
- **Async Runtime**: Tokio (multi-thread, `features = ["full"]`)
- **Web Framework**: Axum 0.7
- **Serialization**: Serde + serde_json
- **Concurrent Map**: DashMap 5
- **Error Handling**: thiserror (library), anyhow (application)
- **Logging**: tracing + tracing-subscriber
- **CLI**: clap (derive features)
- **Compression**: lz4_flex
- **Hashing**: ahash, crc32fast

## 2. Project Structure

The `ramshield` workspace is designed for high-throughput, low-latency threat detection. Each module has a distinct responsibility.

```
rs/
├── src/
│   ├── main.rs          # Server entry point (binary). Minimal logic, defers to `engine`.
│   ├── cli.rs           # CLI admin tool (binary). For stats, health checks, and manual actions.
│   ├── lib.rs           # Library root. Exports key types and orchestrates features.
│   ├── config.rs        # TOML config with strict validation at startup.
│   ├── error.rs         # RsError enum (`thiserror`) for library-level errors.
│   ├── engine/          # Core orchestrator (`Engine` struct). Integrates all subsystems.
│   ├── detection/       # Batch-first detection pipeline. Hot path.
│   │   ├── batch.rs     # IP aggregation logic. Optimized for speed.
│   │   └── rate_tracker.rs  # EWMA + threshold checks.
│   ├── storage/         # In-memory store with persistence and eviction.
│   │   ├── blob_store.rs # The `DashMap` based core data store.
│   │   ├── ttl_wheel.rs # Hierarchical timing wheel for efficient key expiration.
│   │   └── wal.rs       # Write-Ahead Log for crash recovery.
│   ├── metrics/         # Atomic counters for performance monitoring.
│   ├── forecasting/     # Predictive models (Holt-Winters, Shannon entropy).
│   ├── dns/             # DNS monitoring subsystem.
│   │   └── forecasting/
│   ├── learning/        # Pattern learner for adaptive threat identification.
│   ├── prediction/      # Prediction engine using learned models.
│   ├── dashboard/       # Axum HTTP server + SSE for the real-time web dashboard.
│   ├── ipc/             # TCP JSON protocol for agent-server communication.
│   └── util/            # Shared utilities like `BoundedVecDeque`.
├── scripts/             # Python attack simulators for load and correctness testing.
├── config.toml          # Default config (dev-friendly).
├── config.stress.toml   # Production-tuned config for high load.
└── Cargo.toml
```

## 3. Idiomatic Rust Coding Standards

### Memory & Ownership
- Prefer static dispatch (`impl Trait`) over dynamic dispatch (`dyn Trait`).
- Avoid `.clone()` where a reference (`&T`) or a change in ownership would suffice.
- Use `Arc<T>` strictly for shared ownership across threads.
- Use `Box<T>` only for trait objects or recursive types.
- Prefer `&str` over `&String` and `&[T]` over `&Vec<T>` in function signatures.

### Async Concurrency (Tokio)
- Isolate blocking or CPU-intensive work with `tokio::task::spawn_blocking`.
- Keep `Mutex` / `RwLock` guards short. Drop guards explicitly with `drop(guard)` before any `.await`.
- Use `tokio::sync::broadcast` for async fan-out and `crossbeam_channel` for sync/async bridges.
- Use atomic types for simple counters and flags instead of locks.

### Error Handling
- **No `.unwrap()` or `.expect()` in production code.** Use the `?` operator.
- Use `thiserror` for library boundaries and `anyhow` in binary entry points (`main.rs`, `cli.rs`).
- Log all errors via `tracing`. Do not ignore `Result::Err` variants.

## 4. The Self-Healing Protocol (MANDATORY)

Before marking ANY task complete, the full verification suite MUST be run. This is non-negotiable and inspired by the rigorous CI guardrails of mature projects like `jcode`.

1.  **Build All Targets**: `cargo build --all-targets`
    *   Ensures all code, including binaries and tests, compiles successfully.
2.  **Run Clippy (Strict)**: `cargo clippy --all-targets -- -D warnings`
    *   Enforces zero warnings. All lints must be addressed.
3.  **Run Tests**: `cargo test --all`
    *   Ensures all unit and integration tests pass.

A single command for this sequence is provided in `Build Commands`.

## 5. `jcode`-Inspired Practices & Future Enhancements

The `jcode` project showcases advanced Rust engineering patterns. While `ramshield` has a different focus, we can adopt several of its philosophies.

-   **Fine-Grained Crates**: As `ramshield` grows, we may break large modules (like `detection` or `storage`) into their own crates within the workspace. This improves compile times and enforces clearer boundaries. For now, the single-package structure is sufficient.
-   **Profile-Guided Optimization**: `jcode` uses `[profile.<name>.package.<dep>]` to selectively optimize performance-critical dependencies even in debug builds. If profiling reveals bottlenecks in `ramshield`'s dependencies (e.g., hashing or compression libraries), we should adopt this technique to improve development-loop latency.
-   **Automated Guardrails**: `jcode`'s `scripts/check_guardrails.sh` automates checks for code formatting, linting, and various code quality ratchets. We should consider a similar script for `ramshield` to ensure consistency before commits.

## 6. Performance-Critical Paths

| Path | Requirement |
|------|-------------|
| `detection/batch_processor_loop` | Dedicated OS thread, never blocks, checks shutdown flag. |
| `detection/flush_batch` | No allocations in the inner loop, no `.await`. |
| `storage/Store::insert` | Enforce RAM limits; rollback on capacity failure. |
| `dashboard/` | Dashboard updates must not starve the detection engine. |

## 7. Build Commands

```bash
# Debug build (all targets)
cd rs && cargo build --all-targets

# Release build
cd rs && cargo build --release

# Full verification (build, clippy, test)
cd rs && cargo build --all-targets && cargo clippy --all-targets -- -D warnings && cargo test --all

# Run server (production config)
./rs/target/release/ramshield ./rs/config.stress.toml

# Run attack simulation
python3 rs/scripts/attack_sim_100k.py --events 1000000

# Check server health
curl http://127.0.0.1:7891/healthz
```
