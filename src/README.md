# ramshield (main binary)

## Problem

The workspace crates (config, detection, storage, enforcement, forecasting, metrics, protocol, types, xdp) are libraries. They need an entry point that wires them together, manages lifecycle, and exposes network interfaces. This crate is that entry point — it produces the `ramshield` daemon and `ramshield-cli` client.

## How it works

### Boot sequence (src/main.rs)

```
1. Parse --config <path> and --version
2. Load Config (TOML + env overrides + validation)
3. Create Store (DashMap, RAM-limited)
4. Create Metrics (atomic counters + block log)
5. Create Engine → start_async()
6. [optional] Spawn dashboard on dedicated OS thread
7. Wait for Ctrl+C → graceful shutdown
```

### Engine (src/engine/mod.rs)

The orchestrator. Owns shared state and boots the full pipeline:

```rust
pub struct Engine {
    pub config: ConfigHandle,        // hot-reloadable
    pub store: Arc<Store>,           // shared state
    pub metrics: Arc<Metrics>,       // counters
    enforcement_tx: mpsc::Sender<EnforceCommand>,
    // ...
}
impl Engine {
    pub fn new(cfg: Config, store: Arc<Store>, metrics: Arc<Metrics>) -> Self
    pub fn start_async(&self) -> Result<JoinHandle<()>>
    pub fn shutdown(&self)
    pub fn dashboard_snapshot(&self) -> DashboardSnapshot
}
```

`start_async()` spawns the `rs-engine` thread with a multi-thread tokio runtime. Inside `boot_pipeline`:

```
1. Create ConfigHandle (ArcSwap)
2. Select XDP: AyaXdpApplier or StubXdpApplier
3. Create EnforcementService → spawn task
4. Open WAL → replay blocks → re-arm TTL expirations
5. Create DetectionEngine → spawn workers
6. Spawn Forecaster (if enabled)
7. Bind IpcServer
8. tokio::select! on server vs shutdown signal
```

### IPC server (src/ipc/server.rs)

TCP JSON server on port 7890 (configurable):

```rust
pub struct IpcServer { ... }
impl IpcServer {
    pub async fn bind(config, engine, event_tx, store, enforcement_tx) -> Result<Self>
    pub async fn start(&self)  // accept loop
}
```

Newline-delimited JSON. HMAC-SHA256 auth (optional). Backpressure via tokio Semaphore. Each connection runs as a tokio task.

Request dispatch:
| Request | Action |
|---------|--------|
| `report_connections` | Batch convert → detection channel |
| `block_ip` | Create EnforceCommand → enforcement channel |
| `unblock_ip` | Create EnforceCommand → enforcement channel |
| `check_ip` | Read Store → return IpStatus |
| `get_stats` | Aggregate Store + Metrics → return Stats |

### Dashboard (src/dashboard/mod.rs)

Axum HTTP server on port 9999 (configurable). Runs on a dedicated OS thread with its own tokio runtime — isolated from detection load.

| Endpoint | Description |
|----------|-------------|
| `/healthz` | Health check (no auth) |
| `/metrics` | Prometheus exposition text |
| `/api/snapshot` | Full dashboard JSON |
| `/api/history/batches` | Batch history |
| `/api/history/blocks` | Block history |
| `/api/config` | Read/write runtime config |

Auth middleware (src/dashboard/auth.rs): Argon2 password hashing, per-IP rate limiting, cookie-based sessions.

### CLI (src/cli.rs)

```bash
ramshield-cli [--addr 127.0.0.1:7890] [--key <hex>] <command>
```

| Command | IPC request |
|---------|-------------|
| `check <ip>` | `check_ip` |
| `block <ip>` | `block_ip` |
| `unblock <ip>` | `unblock_ip` |
| `stats` | `get_stats` |
| `info <ip>` | `get_ip_stats` |

### Data flow (complete)

```
Client App ──TCP 7890──► IpcServer
                              │
                    ┌─────────┼──────────┐
                    ▼         ▼          ▼
             ReportConn   BlockIp    CheckIp
                    │         │          │
                    ▼         ▼          ▼
             Detection    Enforcement  Store
             Engine       Service     (read)
                    │         │
                    ▼         ▼
             EnforceCmd  Store + WAL + XDP
                    │
                    ▼
             Dashboard (HTTP 9999) ◄── Metrics
```

### Graceful shutdown

1. `engine.shutdown()` — sets AtomicBool + sends watch signal
2. Workers flush remaining PreAgg entries (final detection tick)
3. `join_workers(5s)` — waits via `spawn_blocking` (doesn't park tokio RT)
4. Dashboard thread exits when runtime drops
5. "Shutdown complete."

## Dependencies

```
ramshield (main binary)
  ← all workspace crates
  ← tokio, axum, tower-http, clap, tracing, anyhow, sysinfo
```

## What to read next

- `crates/ramshield-detection/` — the analytical core
- `crates/ramshield-storage/` — the shared state layer
- `crates/ramshield-enforcement/` — the blocking layer
- `docs/IPC.md` — protocol documentation
- `docs/DOCUMENTATION.md` — full system documentation
