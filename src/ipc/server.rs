use bytes::BytesMut;
use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    time::{Duration, Instant, timeout},
};
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

use super::{Request, Response};
use crate::engine::Engine;
use crate::storage::Store;
use ramshield_config::{ConfigHandle, KeyRole};
use ramshield_types::ConnectionEvent;
use ramshield_types::{EnforceAction, EnforceCommand, IpNetwork};

mod auth;
mod connection;
mod handlers;

pub use handlers::is_low_signal;

use auth::{authorize, parse_ipc_keys, verify_frame_auth};
use connection::handle_connection;
use handlers::process_request;

/// Authenticated principal resolved from the IPC auth envelope. Carries the
/// key identity and role so enforcement commands are attributed to the real
/// caller and authorization can be enforced (P2).
#[derive(Clone, Debug)]
struct Principal {
    key_id: String,
    role: KeyRole,
}

/// Connection handling configuration
#[derive(Clone)]
struct ConnectionConfig {
    max_bytes: usize,
    read_timeout: Duration,
    write_timeout: Duration,
    idle_timeout: Duration,
    max_line_length: usize,
    config: ConfigHandle,
    /// Per-key nonce store. Bounded LRU + 10s TTL; lives for the server's
    /// lifetime. Shared by every connection via Arc.
    replay_store: Arc<ramshield_protocol::auth::ReplayStore>,
}

// Tunable constants or defaults (can be moved to config.rs)
const DEFAULT_MAX_CONNECTION_BYTES: usize = 1_048_576; // 1MB per connection
const DEFAULT_READ_TIMEOUT_MS: u64 = 5000;
const DEFAULT_WRITE_TIMEOUT_MS: u64 = 5000;
const BATCH_MAX: usize = 8_192;
/// Upper bound on a block TTL. 1 year in seconds — the panic class here is
/// `Instant::now() + Duration::from_secs(u64::MAX)` overflowing; the clamp
/// keeps every TTL arithmetic in the enforcement task well inside range.
const MAX_TTL_SECS: u64 = 31_536_000;

/// Clamp a block TTL to a sane ceiling. `u64::MAX` used to reach
/// `Instant::now() + Duration` and overflow-panic the enforcement task —
/// the single writer for blocks/expiries. Returns the clamped value.
fn sanitize_ttl(ttl: Option<u64>) -> Result<u64, String> {
    match ttl {
        Some(t) if t > MAX_TTL_SECS => {
            Err(format!("ttl_secs {t} exceeds max {MAX_TTL_SECS} (1 year)"))
        }
        Some(t) => Ok(t),
        None => Ok(0),
    }
}

// Ingest channel capacity — single source of truth: the detection engine's
// bounded channel (64k ConnectionEvents, ~4MB; fills in ~64ms at 1M eps).
pub use ramshield_detection::CHANNEL_CAPACITY;
/// Upper bound on a single line (batch reports). Equals DEFAULT_MAX_CONNECTION_BYTES
/// (1 MB) so a single JSON frame can never allocate beyond the per-connection
/// budget before HMAC auth gates the frame. Prevents OOM-before-auth.
const MAX_LINE_LENGTH: usize = DEFAULT_MAX_CONNECTION_BYTES; // 1 MB
const CONNECTION_IDLE_TIMEOUT_MS: u64 = 30_000; // 30s idle

pub struct IpcServer {
    listener: TcpListener,
    engine: Arc<Engine>,
    /// Live config handle — auth_keys are resolved per-connection (hot-reload).
    config: ConfigHandle,
    event_tx: Sender<ConnectionEvent>,
    store: Arc<Store>,
    enforcement_tx: mpsc::Sender<EnforceCommand>,
    semaphore: Arc<Semaphore>,
    max_connections: usize,
    max_connection_bytes: usize,
    read_timeout_ms: u64,
    write_timeout_ms: u64,
    connection_idle_timeout_ms: u64,
    max_line_length: usize,
    total_connections: Arc<AtomicU64>,
    active_connections: Arc<AtomicU64>,
    rejected_connections: Arc<AtomicU64>,
    dropped_events: Arc<AtomicU64>,
    /// Per-key LRU nonce store for replay protection. Capacity = 256 entries
    /// per key, entries expire after 10s. Sized to cover the max concurrent
    /// IPC clients with headroom; evicted entries leave a brief hole but that
    /// only slightly widens the replay window — acceptable tradeoff vs memory.
    replay_store: Arc<ramshield_protocol::auth::ReplayStore>,
}

impl IpcServer {
    pub async fn bind(
        config: ConfigHandle,
        engine: Arc<Engine>,
        event_tx: Sender<ConnectionEvent>,
        store: Arc<Store>,
        enforcement_tx: mpsc::Sender<EnforceCommand>,
    ) -> std::io::Result<Self> {
        {
            let cfg = config.load();
            cfg.validate().map_err(std::io::Error::other)?;
        }
        let (
            addr,
            max_connections,
            max_connection_bytes,
            read_timeout_ms,
            write_timeout_ms,
            connection_idle_timeout_ms,
            max_line_length,
        ) = {
            let cfg = config.load();
            let addr = cfg.ipc.tcp_addr.clone();
            info!("IPC server binding to {}", addr);
            // Fail closed: malformed keys must not silently disable auth.
            let keys = match parse_ipc_keys(&cfg) {
                Ok(k) => k,
                Err(e) => {
                    return Err(std::io::Error::other(format!(
                        "IPC auth key parse failed at bind: {e}"
                    )));
                }
            };
            let _auth_enabled = !keys.is_empty() || cfg.ipc.require_auth;
            if cfg.ipc.require_auth && keys.is_empty() {
                return Err(std::io::Error::other(
                    "ipc.require_auth=true but no usable auth_keys after parse",
                ));
            }
            if !keys.is_empty() {
                info!("IPC HMAC auth ENABLED ({} keys)", keys.len());
            } else {
                info!("IPC HMAC auth disabled (loopback / no keys)");
            }
            (
                addr,
                cfg.ipc.max_connections.max(1),
                cfg.ipc
                    .max_connection_bytes
                    .unwrap_or(DEFAULT_MAX_CONNECTION_BYTES),
                cfg.ipc.read_timeout_ms.unwrap_or(DEFAULT_READ_TIMEOUT_MS),
                cfg.ipc.write_timeout_ms.unwrap_or(DEFAULT_WRITE_TIMEOUT_MS),
                cfg.ipc
                    .connection_idle_timeout_ms
                    .unwrap_or(CONNECTION_IDLE_TIMEOUT_MS),
                cfg.ipc.max_line_length.unwrap_or(MAX_LINE_LENGTH),
            )
        };
        let listener = TcpListener::bind(&addr).await?;
        info!("IPC server bound to {}", addr);

        Ok(Self {
            listener,
            engine,
            config,
            event_tx,
            store,
            enforcement_tx,
            semaphore: Arc::new(Semaphore::new(max_connections)),
            max_connections,
            max_connection_bytes,
            read_timeout_ms,
            write_timeout_ms,
            connection_idle_timeout_ms,
            max_line_length,
            total_connections: Arc::new(AtomicU64::new(0)),
            active_connections: Arc::new(AtomicU64::new(0)),
            rejected_connections: Arc::new(AtomicU64::new(0)),
            dropped_events: Arc::new(AtomicU64::new(0)),
            // 256 entries × 35s TTL = bounded memory, covers full 30s clock-skew
            // the clock-skew window. Honest comment: cap is per-key not global
            // because the store is shared across keys; tune if one key churns
            // hard. Add global cap when a second key_id lands in prod.
            // P2 fix (replay-hole): a signer clock +30s fast (max tolerated
            // skew) makes a captured frame replayable for skew+TTL = 60s of
            // verifier time, while a 35s store window expired at +35s — a 25s
            // replay gap at max drift. TTL now 2*MAX_CLOCK_SKEW + 5s slack;
            // cap raised to 1024 (32B digests => ~33KB, trivial).
            replay_store: Arc::new(ramshield_protocol::auth::ReplayStore::with_per_key_cap(
                1024, // global cap
                256,  // per-key cap
                Duration::from_secs(65),
            )),
        })
    }

    pub async fn start(&self) {
        info!(
            "IPC server listening (max_connections={}, max_bytes/conn={})",
            self.max_connections, self.max_connection_bytes
        );
        let mut backoff = Duration::from_millis(100);
        loop {
            if self.engine.is_shutting_down() {
                info!("IPC server initiating graceful shutdown");
                self.drain_connections().await;
                info!("IPC server shut down complete");
                break;
            }
            let accept = timeout(Duration::from_secs(1), self.listener.accept()).await;
            let (mut socket, remote) = match accept {
                Ok(Ok((socket, remote))) => {
                    if let Err(e) = socket.set_nodelay(true) {
                        debug!("tcp_nodelay on {remote} failed: {e}");
                    }
                    (socket, remote)
                }
                Ok(Err(e)) => {
                    error!("accept error: {}", e);
                    backoff = Duration::from_secs(1).min(backoff * 2);
                    tokio::time::sleep(backoff).await;
                    continue;
                }
                Err(_) => continue,
            };
            backoff = Duration::from_millis(100);

            let permit = match self.semaphore.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(_) => {
                    self.rejected_connections.fetch_add(1, Ordering::Relaxed);
                    self.engine.metrics.inc_rejected(1);
                    self.engine.metrics.inc_ipc_rejected_connections(1);
                    warn!(
                        "Connection rejected from {}: semaphore exhausted ({})",
                        remote, self.max_connections
                    );
                    let _ = socket.shutdown().await;
                    continue;
                }
            };

            self.total_connections.fetch_add(1, Ordering::Relaxed);
            self.active_connections.fetch_add(1, Ordering::Relaxed);

            let engine = self.engine.clone();
            let event_tx = self.event_tx.clone();
            let store = self.store.clone();
            let enforcement_tx = self.enforcement_tx.clone();
            let config = ConnectionConfig::from_server(self);
            let active = self.active_connections.clone();
            let dropped = self.dropped_events.clone();

            tokio::spawn(async move {
                let _permit = permit;
                if let Err(e) = handle_connection(
                    socket,
                    engine,
                    event_tx,
                    store,
                    enforcement_tx,
                    config,
                    dropped,
                )
                .await
                {
                    // Connection churn is expected under attack (one line per
                    // close is a flood vector): keep every close at TRACE and
                    // sample DEBUG 1/256.
                    static CLOSE_SEQ: AtomicU64 = AtomicU64::new(0);
                    let seq = CLOSE_SEQ.fetch_add(1, Ordering::Relaxed);
                    trace!(remote = %remote, error = %e, close_seq = seq, "connection closed");
                    if seq.is_multiple_of(256) {
                        debug!(remote = %remote, error = %e, close_seq = seq, "connection closed (sampled)");
                    }
                }
                active.fetch_sub(1, Ordering::Relaxed);
            });
        }
    }

    async fn drain_connections(&self) {
        let start = Instant::now();
        while self.active_connections.load(Ordering::Relaxed) > 0 {
            if start.elapsed() > Duration::from_secs(30) {
                warn!(
                    "Shutdown timeout: {} connections still active",
                    self.active_connections.load(Ordering::Relaxed)
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub fn stats(&self) -> IpcServerStats {
        IpcServerStats {
            total_connections: self.total_connections.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            rejected_connections: self.rejected_connections.load(Ordering::Relaxed),
            max_connections: self.max_connections as u64,
            dropped_events: self.dropped_events.load(Ordering::Relaxed),
            channel_capacity: CHANNEL_CAPACITY,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcServerStats {
    pub total_connections: u64,
    pub active_connections: u64,
    pub rejected_connections: u64,
    pub max_connections: u64,
    pub dropped_events: u64,
    /// Bounded channel capacity for connection-event ingest. Equal to the
    /// cap set on the crossbeam channel in DetectionEngine::try_new (64k).
    /// Dashboard can compute utilization = accepted / channel_capacity.
    pub channel_capacity: u64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests;
