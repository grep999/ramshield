# ramshield (main binary)

Entry point and orchestration for RamShield DDoS detection and mitigation.

## src/ modules

### engine/mod.rs
- `boot_pipeline()`: constructs all components (Store, Forecaster, Enforcement, Metrics)
- `run()`: main loop — starts IPC server, dashboard, forecaster, enforcement tasks
- Graceful shutdown via CancellationToken
- Three concurrent tasks: IPC accept loop, forecaster tick loop, enforcement apply loop

### ipc/server.rs
- TCP server with HMAC request signing
- Requests: `event_batch` (ingest), `block_request` (enforce), `unblock_request`
- Responses: JSON with HMAC signature
- `max_connection_bytes`: per-connection buffer limit (configurable, default 1MB)
- Batch processing: up to 1M events per batch, deduplication via event hash
- `BATCH_MAX`: 1M events per flush cycle

### dashboard/mod.rs
- Axum HTTP server for operational dashboard
- Endpoints: /healthz, /api/snapshot, /api/history/blocks, /api/history/batches, /api/config
- Session-based auth (HMAC-signed tokens)
- Real-time metrics: events/sec, blocks, memory, uptime

### dashboard/auth.rs
- HMAC-SHA256 request signing
- Replay protection: nonce store with TTL + LRU eviction
- Per-key tracking: key_id → HMAC key mapping
- Session management: login → token → validate
