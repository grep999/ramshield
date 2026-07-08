# AGENTS.md - RamShield Project Context & Rust Standards

This file defines the project anatomy and coding standards that AI coding assistants MUST follow when working on this repository.

## 1. Tech Stack & Environment

- **Project Type**: Rust Library/Application (Rust 2021 Edition)
- **Primary OS Target**: Linux Mint (Debian/Ubuntu-based)
- **Rust Edition**: Rust 2021 Edition (STRICTLY enforced, no 2015/2018 exceptions)
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

```
rs/
├── src/
│   ├── main.rs          # Server entry point (binary)
│   ├── cli.rs           # CLI admin tool (binary)
│   ├── lib.rs           # Library root
│   ├── config.rs        # TOML config with validation
│   ├── error.rs         # RsError enum (thiserror)
│   ├── engine/          # Core orchestrator (Engine struct)
│   ├── detection/       # Batch-first detection pipeline
│   │   ├── batch.rs     # IP aggregation logic
│   │   └── rate_tracker.rs  # EWMA + threshold checks
│   ├── storage/         # DashMap store + WAL + TTL wheel
│   │   ├── blob_store.rs
│   │   ├── ttl_wheel.rs
│   │   └── wal.rs
│   ├── metrics/         # Atomic counters + dashboard snapshot
│   ├── forecasting/     # Holt-Winters + Shannon entropy
│   ├── dns/             # DNS monitoring + forecaster
│   │   └── forecasting/
│   ├── learning/        # Pattern learner
│   ├── prediction/      # Prediction engine
│   ├── dashboard/       # Axum HTTP server + SSE
│   ├── ipc/             # TCP JSON protocol
│   └── util/            # BoundedVecDeque, DataProcessor
├── scripts/             # Python attack simulators
│   ├── attack_sim_100k.py
│   ├── attack_nexus.py
│   ├── attack_extreme.py
│   └── stress_test.py
├── config.toml          # Default config (512 MB, 256 shards)
├── config.stress.toml   # Production-tuned (8 GB, 1024 shards)
└── Cargo.toml
```

## 3. Idiomatic Rust Coding Standards

### Memory & Ownership
- Prefer static dispatch over `dyn Trait` wherever possible.
- Do NOT insert excessive `.clone()` to bypass the borrow checker. Restructure lifetimes or use reference passing.
- Use `Arc<T>` only when true concurrent multi-ownership is required.
- Use `Box<T>` sparingly — only for trait objects or recursive types.
- Prefer `&str` over `&String`, `&[T]` over `&Vec<T>` in function signatures.

### Async Concurrency (Tokio)
- NEVER run CPU-bound or synchronous I/O directly in async context. Use `tokio::task::spawn_blocking`.
- Keep `Mutex` / `RwLock` guards short-lived. Always `drop(guard)` before `.await` points.
- Use `crossbeam_channel` for dedicated threads, `tokio::sync::broadcast` for async fan-out.
- Prefer `AtomicU64` / `AtomicBool` over locks for simple counters and flags.
- Dedicated OS threads for blocking loops must check shutdown flags.

### Error Handling
- NEVER use `.unwrap()` or `.expect()` in production modules.
- Use `Result` + `?` operator. `thiserror` for library boundaries, `anyhow` for application entrypoints.
- Log errors with `tracing::warn!` or `tracing::error!` — never silently ignore.
- Convert `let _ = result` to explicit error logging.

### Type Safety
- Use newtypes for domain concepts.
- Prefer `#[derive(Debug, Clone, Serialize, Deserialize)]` on all public types.
- Use `#[serde(default)]` for config fields with sensible defaults.
- Validate config at startup, not at runtime.

## 4. The Self-Healing Protocol (MANDATORY)

Before marking ANY task complete, execute ALL steps:

1. **`cargo build --all-targets`** — must compile clean
2. **Diagnostics Loop** — if build fails, read stderr, fix exact line, repeat
3. **`cargo clippy --all-targets -- -D warnings`** — zero lints allowed
4. **`cargo test`** — all assertions pass
5. **Manual Review** — scan for unnecessary clones, unwraps, or dead code

Expected output:
```
   Compiling ramshield v0.1.0
    Finished dev profile
    Checking ramshield v0.1.0
    Finished dev profile
     Running unittests
test result: ok. N passed; 0 failed
```

## 5. Performance-Critical Paths

| Path | Requirement |
|------|-------------|
| `detection/batch_processor_loop` | Dedicated OS thread, never blocks, checks shutdown flag |
| `detection/flush_batch` | No allocations in inner loop, no await |
| `engine/dashboard_snapshot` | Separate tokio runtime, never starves |
| `engine/ipc_server` | Semaphore backpressure, graceful conn limit |
| `storage/Store::insert` | RAM limit enforced, rollback on capacity |
| `storage/BoundedVecDeque` | Fixed capacity, no unbounded growth |

## 6. Testing Strategy

- Unit tests in `#[cfg(test)]` modules within each source file.
- Integration via `cargo test`.
- Performance benchmarks via `scripts/stress_test.py`.
- Attack simulation via `scripts/attack_sim_100k.py` and `scripts/attack_nexus.py`.

## 7. Configuration

- Default: `config.toml` (512 MB RAM, 256 shards)
- Production: `config.stress.toml` (8 GB RAM, 1024 shards, tuned thresholds)
- Env overrides: `RAMSHIELD_DETECTION__RPS_THRESHOLD=500`

## 8. Build Commands

```bash
# Debug build
cd rs && cargo build --all-targets

# Release build
cd rs && cargo build --release

# Full verification
cd rs && cargo build --all-targets && cargo clippy --all-targets -- -D warnings && cargo test

# Run server
./rs/target/release/ramshield ./rs/config.stress.toml

# Run attack simulation
python3 rs/scripts/attack_sim_100k.py --events 1000000 --workers 64

# Check server health
curl http://127.0.0.1:7891/healthz

# Get server stats
./rs/target/release/ramshield-cli stats
```
