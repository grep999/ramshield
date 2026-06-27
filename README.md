# RamShield - High-Performance Rust-based Rate Limiting and Threat Detection

RamShield is a robust, concurrent, and highly optimized security service written in Rust, designed to protect web applications and APIs from various forms of malicious traffic, including DDoS attacks, brute-force attempts, and scraping. It operates as a real-time, in-memory detection and blocking engine, leveraging advanced data structures and asynchronous programming to maintain high throughput and low latency.

## Key Features

-   **High-Performance Architecture**: Built with Tokio's multi-threaded runtime, DashMap for concurrent storage, and `crossbeam_channel` for efficient inter-thread communication, ensuring optimal performance under heavy load.
-   **Configurable Detection Engine**: Features a batch-first processing pipeline that aggregates connection events, applies Exponentially Weighted Moving Average (EWMA) for rate tracking, and utilizes a Bloom filter for probabilistic IP tracking.
-   **Adaptive Threat Intelligence**: Incorporates Holt-Winters time series forecasting and Shannon entropy analysis to detect anomalies and identify emerging threats.
-   **WAL-based Persistence**: Write-Ahead Log (WAL) ensures recovery of blocked IP states across restarts, preventing loss of critical security decisions.
-   **Resource-Aware Storage**: An in-memory store with configurable RAM limits (`ram_limit_mb`) and a Time-To-Live (TTL) wheel for efficient eviction of expired entries. Supports bounded history for IP records to prevent unbounded memory growth.
-   **IPC for Integration**: A TCP-based JSON Inter-Process Communication (IPC) server allows external systems to interact with RamShield for reporting events, querying stats, and managing blocks.
-   **Real-time Dashboard**: An `Axum`-based HTTP server provides a live dashboard (`/api/snapshot`, `/api/stream`) for monitoring system health, traffic metrics, and active blocks. The dashboard runs on a dedicated thread to ensure responsiveness even under peak load.
-   **CLI Tooling**: A command-line interface (`ramshield-cli`) for convenient interaction with the running RamShield instance, including checking IP status, manually blocking/unblocking, and retrieving global statistics.
-   **Comprehensive Testing**: Extensive unit tests, integration tests, and Python-based attack simulators (`attack_nexus.py`, `stress_test.py`) to validate functionality and performance under various attack scenarios.

## Technical Details

-   **Rust 2021 Edition**: Leverages modern Rust features for safety and performance.
-   **Concurrency Model**: Achieves fearless concurrency through careful use of `Arc`, `RwLock`, `Mutex`, and `Atomic` types, adhering to Rust's ownership and borrowing rules.
-   **Error Handling**: Uses `thiserror` for library-level error definitions and `anyhow` for application-level error handling, ensuring robust and explicit error propagation.
-   **Configurability**: All major parameters are configurable via TOML files (`config.toml`, `config.stress.toml`) and can be overridden using environment variables (e.g., `RAMSHIELD_ENGINE__RAM_LIMIT_MB`).

## Project Structure

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
├── config.toml          # Default configuration (512 MB, 256 shards)
├── config.stress.toml   # Production-tuned configuration (8 GB, 1024 shards)
└── Cargo.toml           # Project dependencies and metadata
```

## Getting Started

### Prerequisites

-   Rustup: Follow instructions at [rustup.rs](https://rustup.rs/)
-   Python 3: For running attack simulation scripts.

### Build

Navigate to the `rs` directory and build the project:

```bash
cd rs
cargo build --release
```

This will produce the main `ramshield` binary and `ramshield-cli` in `target/release/`.

### Run

To run RamShield with the default configuration:

```bash
cd rs
./target/release/ramshield ./config.toml
```

For a production-tuned configuration:

```bash
cd rs
./target/release/ramshield ./config.stress.toml
```

Override configuration values using environment variables:

```bash
RAMSHIELD_DETECTION__RPS_THRESHOLD=500 RAMSHIELD_ENGINE__RAM_LIMIT_MB=4096 ./target/release/ramshield ./config.stress.toml
```

### Dashboard

Access the real-time dashboard in your browser: `http://127.0.0.1:7891`

Check service health:

```bash
curl http://127.0.0.1:7891/healthz
```

### CLI Usage

Get current system statistics:

```bash
./rs/target/release/ramshield-cli stats
```

Check IP status:

```bash
./rs/target/release/ramshield-cli check 192.168.1.100
```

Manually block an IP:

```bash
./rs/target/release/ramshield-cli block 192.168.1.100 --reason "manual override" --ttl 3600
```

### Attack Simulation

Use the provided Python scripts to simulate traffic and test RamShield's performance:

```bash
# Example: Run a 1 million event attack
python3 rs/scripts/attack_sim_100k.py --events 1000000 --workers 64
```

## Contributing

Contributions are welcome! Please ensure your code adheres to the Idiomatic Rust Coding Standards and passes the Self-Healing Protocol defined in `AGENTS.md`.

## License

This project is licensed under the MIT License.
