# ramshield (main binary)

```text
main()
  │
  ├── Config::load()
  ├── Store::new()
  ├── Metrics::new()
  ├── Engine::new(cfg, store, metrics)
  │
  ├── [thread: dashboard] dashboard::serve()  ──→ HTTP :9999
  │
  ├── Engine::start_async()
  │     │
  │     ├── [runtime: engine rt] boot_pipeline()
  │     │     ├── WAL::open() + replay
  │     │     ├── EnforcementService::new()
  │     │     ├── DetectionEngine::new()
  │     │     ├── DetectionEngine::spawn_workers()
  │     │     ├── Forecaster::new()
  │     │     ├── IpcServer::bind()  ──→ TCP :7890
  │     │     └── tokio::select! { IPC, detection, forecast, enforcement }
  │     │
  │     └── graceful shutdown on SIGTERM/SIGINT
  │
  └── [main thread] wait for shutdown ──→ cleanup ──→ exit
```

## Why it exists

The workspace crates are libraries. They need an entry point that creates shared state (Store, Metrics), boots subsystems in the right order, wires them together through channels, and handles graceful shutdown. `src/` is that entry point — a thin orchestration layer with no business logic.

## How it works

### Boot Sequence

`main()` runs six steps in order:

1. **Parse CLI** — `clap` parses `--config path/to/config.toml` (default: `config.toml`).
2. **Load config** — `Config::load()` reads TOML, applies env overrides, validates.
3. **Create shared state** — `Store::new(shard_count)` and `Metrics::with_block_log(1000)` are wrapped in `Arc` and shared across all subsystems.
4. **Create engine** — `Engine::new(cfg, store, metrics)` allocates the enforcement channel (`mpsc::channel(4096)`), the shutdown watcher, and the detection mutex.
5. **Start dashboard** — a dedicated OS thread runs `dashboard::serve()` on HTTP `:9999` (configurable). This thread has its own tokio runtime — it must not compete with the engine's runtime for CPU.
6. **Start engine** — `Engine::start_async()` spawns a named thread (`rs-engine`) that creates a multi-threaded tokio runtime and runs `boot_pipeline()`.

### Pipeline

`boot_pipeline()` runs inside the engine's tokio runtime:

1. **WAL replay** — if enabled, `Wal::open()` opens the write-ahead log and `replay_wal_into_store()` restores blocked IPs and their TTLs.
2. **Enforcement** — `EnforcementService::new(store, metrics, xdp, shutdown)` takes the enforcement channel receiver and spawns a tokio task that processes `EnforceCommand`s.
3. **Detection** — `DetectionEngine::new(cfg, store, metrics, enforcement_tx)` creates the batch processing system and `spawn_workers()` launches N batch processor threads (default: CPU core count) plus one subnet aggregation thread.
4. **Forecasting** — `Forecaster::new(store, config, enforcement_tx, metrics)` spawns a tokio task that runs `tick()` every 10 seconds.
5. **IPC** — `IpcServer::bind(addr)` starts accepting TCP connections. Each connection spawns a tokio task that reads line-delimited JSON, dispatches to detection/enforcement, and writes responses.
6. **Select loop** — `tokio::select!` monitors the shutdown signal. When SIGTERM or SIGINT arrives, it sets the shutdown flag and breaks the loop.

### Graceful Shutdown

The shutdown sequence:

1. `Engine::shutdown()` sets `AtomicBool` to true and sends `watch::channel` signal.
2. IPC server stops accepting new connections, drains existing ones (5-second grace).
3. Detection workers stop pulling from the event channel — events in flight are processed but no new batches start.
4. Enforcement service drains its command channel — all pending blocks are applied.
5. `Engine::join_workers()` waits up to 5 seconds for batch threads to finish.
6. Main thread exits.

### Engine Struct

```rust
pub struct Engine {
    pub config: Arc<ArcSwap<Config>>,  // hot-reloadable
    pub store: Arc<Store>,
    pub metrics: Arc<Metrics>,
    pub shutdown: Arc<AtomicBool>,
    detection: Mutex<Option<Arc<DetectionEngine>>>,
    enforcement_tx: mpsc::Sender<EnforceCommand>,
    enforcement_rx: Mutex<Option<mpsc::Receiver<EnforceCommand>>>,
    xdp_active: Arc<AtomicBool>,
    shutdown_tx: watch::Sender<bool>,
}
```

`Engine` is wrapped in `Arc` and shared between the engine thread, the dashboard thread, and the IPC server. It provides accessor methods for dashboard data — `dashboard_snapshot()`, `get_block_log()`, `get_hot_subnets()` — that read from the shared Store and Metrics.

### IPC Server

```rust
pub struct IpcServer {
    listener: TcpListener,
    engine: Arc<Engine>,
    max_connections: usize,
    connection_count: AtomicUsize,
}
```

Binds to `127.0.0.1:7890` (configurable). Each connection is a tokio task that:

1. Reads a line (up to `max_line_length`, default 32MB).
2. Parses it as `Message` (protocol crate).
3. Verifies HMAC authentication if configured.
4. Dispatches the `Request` to the appropriate handler.
5. Writes the `Response` back as a JSON line.
6. Repeats until the client disconnects or the read timeout expires.

### Dashboard

A thin HTTP server on `:9999` serving:

- `GET /healthz` — returns `200 OK` (no auth required).
- `GET /login` — serves the login page.
- `POST /login` — validates credentials against Argon2 hash.
- `GET /api/snapshot` — returns `DashboardSnapshot` as JSON (requires auth).
- `GET /metrics` — returns Prometheus text format (no auth, for scraping).
- `GET /api/history/blocks` — returns block history ring buffer.

Session management uses HMAC-signed cookies — no database, no server-side session store.

### CLI

```bash
ramshield --config config.toml    # default: config.toml
ramshield --version               # build version from Cargo.toml
```

No subcommands. The daemon is a long-running process — all configuration is in the TOML file. Operators who need to block/unblock IPs manually use the IPC protocol or the dashboard, not CLI flags.

## Uniqueness

**Two runtime architecture.** The engine runs on a dedicated OS thread with its own multi-threaded tokio runtime. The dashboard runs on a separate OS thread with its own runtime. This prevents dashboard request handling from competing with detection and enforcement for tokio task slots — critical during an attack when the engine's runtime is saturated.

**Enforcement channel is one-shot.** `enforcement_rx` is taken out of the Mutex exactly once — `Mutex<Option<Receiver>>` becomes `None` after the first take. This prevents accidental double-start of the enforcement service, which would create duplicate TTL rings and WAL writes.

**No subcommands.** The CLI is intentionally minimal — just `--config`. All operational decisions (which algorithms to run, what thresholds to use, whether XDP is enabled) are in the config file. This keeps the daemon stateless and configuration-driven — operators version-control their `config.toml` and deploy it like any other config.

## Dependencies

**Reads from:** `ramshield-config` (`Config`), `ramshield-storage` (`Store`), `ramshield-metrics` (`Metrics`), `ramshield-detection` (`DetectionEngine`), `ramshield-enforcement` (`EnforcementService`, `replay_wal_into_store`), `ramshield-forecasting` (`Forecaster`), `ramshield-ipc` (`IpcServer`), `ramshield-dashboard` (`serve`), `ramshield-storage::wal` (`Wal`), `ramshield-types` (`ConnectionEvent`).

**Written by:** nothing — this is the leaf of the dependency tree.

## Benchmarks

No standalone benchmarks — the binary is the integration point. End-to-end benchmarks are run via `scripts/perf_test.py` and `scripts/attack_nexus.py`, which start the binary and measure throughput, latency, and resource usage from outside.

## Testing

157 integration tests across the workspace test individual subsystems. The binary itself is tested through `scripts/verify.sh` — a smoke test that starts the daemon, sends events, checks the dashboard, and verifies block behavior.
