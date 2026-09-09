# ramshield (main binary)

Entry point, engine pipeline, IPC server, dashboard, and CLI for the RamShield DDoS detection and mitigation system. This crate ties all workspace crates into a single deployable binary.

## Binary targets

| Binary | Path | Purpose |
|--------|------|---------|
| `ramshield` | `src/main.rs` | Full daemon: engine + IPC + dashboard |
| `ramshield-cli` | `src/cli.rs` | Lightweight client for IPC commands |

## src/main.rs — daemon entry point

```rust
#[tokio::main]
async fn main() -> Result<()>
```

Boot sequence:
1. Parse `--config <path>` and `--version` flags.
2. Load `Config` (TOML + env overrides + validation).
3. Create `Store` (DashMap with configured shard count and RAM limit).
4. Create `Metrics` (atomic counters + block log).
5. Create `Engine` (orchestrates detection, forecasting, enforcement).
6. Start `Engine::start_async()` — spawns the batch processing pipeline.
7. Start dashboard on a dedicated OS thread (isolated tokio runtime).
8. Wait for Ctrl+C, then graceful shutdown with 6s timeout.

### Shutdown sequence

1. `engine.shutdown()` — signals all workers to stop.
2. Workers flush remaining `PreAgg` entries (final detection tick).
3. `join_workers(5s)` — waits for workers to finish, runs in `spawn_blocking` to avoid parking the tokio runtime.
4. Dashboard thread exits when its runtime drops.
5. Log "Shutdown complete."

## src/engine/mod.rs — pipeline orchestrator

```rust
pub struct Engine {
    config: ConfigHandle,
    store: Arc<Store>,
    metrics: Arc<Metrics>,
    // ... internal channels and workers
}
impl Engine {
    pub fn new(config: Config, store: Arc<Store>, metrics: Arc<Metrics>) -> Self
    pub fn start_async(&self) -> Result<JoinHandle<()>>
    pub fn shutdown(&self)
    pub fn join_workers(&self, timeout: Duration)
}
```

### Data flow

```
IPC Server ─── ConnectionEvents ──► Engine
                    │
                    ▼
            PreAggregator (DashMap, 50ms window)
                    │  flush()
                    ▼
            Detection Engine
                    ├── EWMA + CUSUM + Pulse
                    ├── Bloom filter
                    ├── Subnet swarm gate
                    └── Threat scoring
                    │
                    ▼
            Store (DashMap, RAM-limited)
                    │
        ┌───────────┼──────────────┐
        ▼           ▼              ▼
   Enforcement  Forecasting    Metrics
   (WAL+XDP)   (HoltWinters   (Prometheus
                 + Bayesian)    + Dashboard)
```

### Worker threads

The engine spawns dedicated worker threads for:
- **Batch processing:** receives `ConnectionEvent` from IPC, feeds into `PreAggregator`.
- **Detection tick:** every 50ms, flushes `PreAgg` → detection → store.
- **Forecasting tick:** every 1s, reads global stats → HoltWinters + Bayesian → enforcement.
- **Enforcement apply:** receives `EnforceCommand` from detection/forecasting, applies to Store + WAL + XDP.

Each worker runs on its own thread (not a tokio task) to avoid blocking the async runtime with CPU-intensive detection math.

## src/ipc/server.rs — TCP JSON server

```rust
pub struct IpcServer { ... }
impl IpcServer {
    pub fn bind(config: &Config, store: Arc<Store>, engine: Arc<Engine>) -> Result<Self>
    pub async fn run(&self) -> Result<()>
}
```

Newline-delimited JSON over TCP. Accepts connections, reads frames, dispatches to handlers, writes responses.

### Connection handling

Each connection gets:
- Read/write timeouts (configurable, default 5s).
- Idle timeout (default 30s).
- Max connection bytes (default 1MB) — limits buffer growth.
- Max line length (default 32MB) — protects against oversized frames.
- HMAC-SHA256 authentication (optional, configured via `auth_keys`).

### Request dispatch

| Request type | Handler |
|-------------|---------|
| `check_ip` | `Store::get()` → format response |
| `block_ip` | `EnforcementEngine::apply(Block)` |
| `unblock_ip` | `EnforcementEngine::apply(Unblock)` |
| `get_ip_stats` | `Store::get()` → detailed IpRecord fields |
| `get_stats` | Aggregate from `Store` + `Metrics` |
| `report_connection` | Convert to `ConnectionEvent` → engine channel |
| `report_connections` | Batch convert → engine channel |
| `flush` | Force `PreAggregator::flush()` |

### Concurrency

The IPC server uses a `Semaphore` to limit concurrent connections (default 256). Each connection runs as a tokio task. The server's main loop accepts connections and spawns handler tasks.

## src/dashboard/mod.rs — HTTP API server

```rust
pub async fn serve(engine: Arc<Engine>, addr: &str, cfg: &Config) -> Result<(), String>
```

Axum-based HTTP server running on a dedicated OS thread with its own tokio runtime. This isolation guarantees dashboard responsiveness even under heavy detection load.

### Endpoints

| Path | Method | Description |
|------|--------|-------------|
| `/` | GET | Dashboard HTML page |
| `/login` | GET/POST | Login form + authentication |
| `/healthz` | GET | Health check: `{"healthy":true}` |
| `/metrics` | GET | Prometheus exposition text |
| `/api/snapshot` | GET | Full dashboard JSON snapshot |
| `/api/history/batches` | GET | Batch history (paginated) |
| `/api/history/blocks` | GET | Block history (paginated) |
| `/api/traffic/subnets` | GET | Subnet traffic data |
| `/api/status/modules` | GET | Module status (detection, forecasting, XDP) |
| `/api/config` | GET/POST | Read/write runtime config |

### Authentication

Session-based auth with HMAC-signed tokens. Login flow:
1. POST username + password to `/login`.
2. Password verified against `argon2` hash in config.
3. Session token (HMAC-signed, TTL'd) returned as cookie.
4. All `/api/*` requests require valid session token.
5. Rate limiting: max login attempts (default 5) with exponential backoff.

## src/dashboard/auth.rs — session management

```rust
pub async fn require_auth(cookies: CookieJar, ...) -> Result<impl IntoResponse, Redirect>
```

Middleware that validates session tokens on protected endpoints. Tokens are HMAC-SHA256 signed with a server-side secret. Invalid or expired tokens redirect to `/login`.

## src/cli.rs — command-line client

```bash
ramshield-cli [--addr 127.0.0.1:7890] [--key <hex>] <command>
```

| Command | IPC request |
|---------|-------------|
| `check <ip>` | `check_ip` |
| `block <ip> [--reason R] [--ttl N]` | `block_ip` |
| `unblock <ip>` | `unblock_ip` |
| `stats` | `get_stats` |
| `status [--json]` | `get_status` |
| `info <ip>` | `get_ip_stats` |

Supports HMAC authentication via `--key` flag or `RAMSHIELD_IPC_KEY` env var.

## Re-exports (lib.rs)

```rust
pub mod config;    // ramshield_config::*
pub mod detection;  // ramshield_detection::*
pub mod enforcement;// ramshield_enforcement::*
pub mod forecasting;// ramshield_forecasting::*
pub mod metrics;    // ramshield_metrics::*
pub mod storage;    // ramshield_storage::*
pub mod ipc;
pub mod dashboard;
pub mod engine;

pub use engine::Engine;
pub use ramshield_config::{Config, ConfigHandle};
```

The `lib.rs` re-exports all crates as flat modules, making `ramshield::storage::Store` equivalent to `ramshield_storage::Store`. This simplifies imports for downstream users.

## Dependencies

`tokio` (async runtime), `axum` (HTTP), `tower-http` (CORS), `clap` (CLI parsing), `tracing` + `tracing-subscriber` (logging), `anyhow` (error handling), `sysinfo` (system metrics). Plus all workspace crates.

## Tests

- Config loading: TOML parse, env overrides, validation.
- IPC wiring: bind, accept, send frame, receive response.
- Dashboard: health endpoint returns 200, snapshot returns valid JSON.
- CLI: connects to IPC, sends command, prints response.
- Graceful shutdown: engine stops, workers flush, no panics.
