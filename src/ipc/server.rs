use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    time::{timeout, Duration, Instant},
};
use tracing::{debug, error, info, warn};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::detection::ConnectionEvent;
use crate::engine::Engine;
use crate::storage::Store;
use super::{Request, Response};

/// Connection handling configuration
#[derive(Clone)]
struct ConnectionConfig {
    max_bytes: usize,
    read_timeout: Duration,
    write_timeout: Duration,
    idle_timeout: Duration,
}

impl ConnectionConfig {
    fn from_server(server: &IpcServer) -> Self {
        Self {
            max_bytes: server.max_connection_bytes,
            read_timeout: Duration::from_millis(server.read_timeout_ms),
            write_timeout: Duration::from_millis(server.write_timeout_ms),
            idle_timeout: Duration::from_millis(server.connection_idle_timeout_ms),
        }
    }
}

// Tunable constants or defaults (can be moved to config.rs)
const DEFAULT_MAX_CONNECTION_BYTES: usize = 1_048_576; // 1MB per connection
const DEFAULT_READ_TIMEOUT_MS: u64 = 5000;
const DEFAULT_WRITE_TIMEOUT_MS: u64 = 5000;
const BATCH_MAX: usize = 4096;
const MAX_LINE_LENGTH: usize = 1_048_576; // 1MB max single line
const CONNECTION_IDLE_TIMEOUT_MS: u64 = 30_000; // 30s idle

pub struct IpcServer {
    listener: TcpListener,
    engine: Arc<Engine>,
    event_tx: Sender<ConnectionEvent>,
    store: Arc<Store>,
    semaphore: Arc<Semaphore>,
    max_connections: usize,
    max_connection_bytes: usize,
    read_timeout_ms: u64,
    write_timeout_ms: u64,
    connection_idle_timeout_ms: u64,
    total_connections: Arc<AtomicU64>,
    active_connections: Arc<AtomicU64>,
    rejected_connections: Arc<AtomicU64>,
    dropped_events: Arc<AtomicU64>,
}

impl IpcServer {
    pub async fn bind(
        config: &Config,
        engine: Arc<Engine>,
        event_tx: Sender<ConnectionEvent>,
        store: Arc<Store>,
    ) -> std::io::Result<Self> {
        let addr = config.ipc.tcp_addr.clone();
        info!("IPC server binding to {}", addr);
        let listener = TcpListener::bind(&addr).await?;
        info!("IPC server bound to {}", addr);

        let max_connections = config.ipc.max_connections.max(1);
        let max_connection_bytes = config.ipc.max_connection_bytes.unwrap_or(DEFAULT_MAX_CONNECTION_BYTES);
        let read_timeout_ms = config.ipc.read_timeout_ms.unwrap_or(DEFAULT_READ_TIMEOUT_MS);
        let write_timeout_ms = config.ipc.write_timeout_ms.unwrap_or(DEFAULT_WRITE_TIMEOUT_MS);
        let connection_idle_timeout_ms = config.ipc.connection_idle_timeout_ms.unwrap_or(CONNECTION_IDLE_TIMEOUT_MS);

        Ok(Self {
            listener,
            engine,
            event_tx,
            store,
            semaphore: Arc::new(Semaphore::new(max_connections)),
            max_connections,
            max_connection_bytes,
            read_timeout_ms,
            write_timeout_ms,
            connection_idle_timeout_ms,
            total_connections: Arc::new(AtomicU64::new(0)),
            active_connections: Arc::new(AtomicU64::new(0)),
            rejected_connections: Arc::new(AtomicU64::new(0)),
            dropped_events: Arc::new(AtomicU64::new(0)),
        })
    }

    pub async fn start(&self) {
        info!("IPC server listening (max_connections={}, max_bytes/conn={})", self.max_connections, self.max_connection_bytes);
        let mut backoff = Duration::from_millis(100);
        loop {
            if self.engine.is_shutting_down() {
                info!("IPC server initiating graceful shutdown");
                self.drain_connections().await;
                info!("IPC server shut down complete");
                break;
            }
            let accept = timeout(
                Duration::from_secs(1),
                self.listener.accept(),
            )
            .await;
            let (mut socket, remote) = match accept {
                Ok(Ok(pair)) => pair,
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
                    warn!("Connection rejected from {}: semaphore exhausted ({})", remote, self.max_connections);
                    let _ = socket.shutdown().await;
                    continue;
                }
            };

            self.total_connections.fetch_add(1, Ordering::Relaxed);
            self.active_connections.fetch_add(1, Ordering::Relaxed);

            let engine = self.engine.clone();
            let event_tx = self.event_tx.clone();
            let store = self.store.clone();
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
                    config,
                    dropped,
                ).await {
                    debug!("conn {} closed: {}", remote, e);
                }
                active.fetch_sub(1, Ordering::Relaxed);
            });
        }
    }

    async fn drain_connections(&self) {
        let start = Instant::now();
        while self.active_connections.load(Ordering::Relaxed) > 0 {
            if start.elapsed() > Duration::from_secs(30) {
                warn!("Shutdown timeout: {} connections still active", self.active_connections.load(Ordering::Relaxed));
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
}

async fn handle_connection(
    mut socket: TcpStream,
    engine: Arc<Engine>,
    event_tx: Sender<ConnectionEvent>,
    store: Arc<Store>,
    config: ConnectionConfig,
    dropped_events: Arc<AtomicU64>,
) -> Result<(), std::io::Error> {
    let mut buf = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    let mut total_bytes_read = 0usize;
    let mut last_activity = Instant::now();

    loop {
        if last_activity.elapsed() > config.idle_timeout {
            debug!("Connection idle timeout");
            return Ok(());
        }

        if total_bytes_read >= config.max_bytes {
            debug!("Connection exceeded max bytes ({})", config.max_bytes);
            return Ok(());
        }

        let n = match timeout(config.read_timeout, socket.read(&mut chunk)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                debug!("Read timeout");
                return Ok(());
            }
        };

        if n == 0 { return Ok(()); }

        total_bytes_read += n;
        last_activity = Instant::now();
        buf.extend_from_slice(&chunk[..n]);

        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            if pos > MAX_LINE_LENGTH {
                warn!("Line exceeds max length ({}), dropping connection", MAX_LINE_LENGTH);
                return Ok(());
            }
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let req: Request = match serde_json::from_slice(&line) {
                Ok(r) => r,
                Err(e) => {
                    let resp = Response::Error { code: 1, message: format!("parse: {}", e) };
                    if timeout(config.write_timeout, write_resp(&mut socket, &resp)).await.is_err() {
                        debug!("Write timeout on error response");
                        return Ok(());
                    }
                    continue;
                }
            };

            engine.metrics.inc_requests();
            let resp = process_request(req, &event_tx, &store, dropped_events.clone());
            if timeout(config.write_timeout, write_resp(&mut socket, &resp)).await.is_err() {
                debug!("Write timeout");
                return Ok(());
            }
        }
    }
}

async fn write_resp(socket: &mut TcpStream, resp: &Response) -> Result<(), std::io::Error> {
    let bytes = serde_json::to_vec(resp).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e)
    })?;
    socket.write_all(&bytes).await?;
    socket.write_all(b"\n").await?;
    Ok(())
}

fn epoch_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn process_request(
    req: Request,
    event_tx: &Sender<ConnectionEvent>,
    store: &Store,
    dropped_events: Arc<AtomicU64>,
) -> Response {
    match req {
        Request::CheckIp { ip } => {
                let ip_addr = match ip.parse() {
        Ok(addr) => addr,
        Err(_) => return Response::Error { code: 400, message: format!("invalid ip address: {}", ip) },
    };
    
            let status = store.get(&ip_addr);
            Response::IpStatus {
                ip,
                blocked: status.is_some_and(|v| matches!(v, crate::storage::Value::IpRecord(rec) if rec.block_state != crate::storage::BlockState::Clean)),
                threat: 0.0,
                ewma_rps: 0.0,
                reason: None,
            }
        },
        Request::BlockIp { ip, reason, ttl_secs } => {
                let ip_addr = match ip.parse() {
        Ok(addr) => addr,
        Err(_) => return Response::Error { code: 400, message: format!("invalid ip address: {}", ip) },
    };
    
            match store.get(&ip_addr) {
                Some(crate::storage::Value::IpRecord(rec)) => {
                    let state = if let Some(_ttl) = ttl_secs {
                        crate::storage::BlockState::Blocked {
                            reason: crate::storage::BlockReason::ManualBlock,
                            since_ns: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_nanos() as u64)
                                .unwrap_or(0),
                        }
                    } else {
                        crate::storage::BlockState::Blocked {
                            reason: crate::storage::BlockReason::ManualBlock,
                            since_ns: rec.block_state.since_ns(),
                        }
                    };
                    store.insert(ip_addr, crate::storage::Value::IpRecord(crate::storage::IpRecord {
                        block_state: state.clone(),
                        ..rec.clone()
                    }), None, usize::MAX).ok();
                    Response::Ok { message: format!("blocked {} ttl={:?} reason={}", ip, ttl_secs, reason), state: Some(format!("{:?}", state)) }
                }
                _ => Response::Error { code: 2, message: format!("unknown ip {}", ip) },
            }
        },
        Request::UnblockIp { ip } => {
                let ip_addr = match ip.parse() {
        Ok(addr) => addr,
        Err(_) => return Response::Error { code: 400, message: format!("invalid ip address: {}", ip) },
    };
    
            match store.get(&ip_addr) {
                Some(crate::storage::Value::IpRecord(rec)) => {
                    let mut updated = rec.clone();
                    updated.block_state = crate::storage::BlockState::Clean;
                    store.insert(ip_addr, crate::storage::Value::IpRecord(updated), None, usize::MAX).ok();
                    Response::Ok { message: format!("unblocked {}", ip), state: Some("clean".into()) }
                }
                _ => Response::Error { code: 2, message: format!("unknown ip {}", ip) },
            }
        },
        Request::GetIpStats { ip } => {
                let ip_addr = match ip.parse() {
        Ok(addr) => addr,
        Err(_) => return Response::Error { code: 400, message: format!("invalid ip address: {}", ip) },
    };
    
            if let Some(crate::storage::Value::IpRecord(rec)) = store.get(&ip_addr) {
                Response::IpDetail(crate::ipc::IpDetail {
                    ip,
                    count: rec.request_count,
                    ewma_rps: rec.ewma_rps,
                    threat: rec.threat_score,
                    state: format!("{:?}", rec.block_state),
                    bytes_in: rec.bytes_in,
                    first_seen_s: rec.first_seen_ns / 1_000_000_000,
                    last_seen_s: rec.last_seen_ns / 1_000_000_000,
                })
            } else {
                Response::IpDetail(crate::ipc::IpDetail {
                    ip,
                    count: 0,
                    ewma_rps: 0.0,
                    threat: 0.0,
                    state: "not_tracked".into(),
                    bytes_in: 0,
                    first_seen_s: 0,
                    last_seen_s: 0,
                })
            }
        },
        Request::GetStats => {
            let stats = store.get_stats();
            Response::Stats(crate::ipc::Stats {
                ips_tracked: stats.ips_tracked,
                blocked: stats.blocked,
                ram_bytes: stats.ram_bytes,
                ram_limit_mb: stats.ram_limit_mb,
                uptime_secs: stats.uptime_secs,
                evictions: stats.evictions,
            })
        },
        Request::GetStatus => Response::Ok { message: "ok".into(), state: None },
        Request::ReportConnection { ip, bytes, status_code, proto_fp } => {
            let ev = ConnectionEvent {
                            ip: match ip.parse() {
                Ok(addr) => addr,
                Err(_) => return Response::Error { code: 400, message: format!("invalid ip address: {}", ip) },
            },
                timestamp_ns: epoch_ns(),
                bytes,
                status_code,
                proto_fingerprint: proto_fp,
            };
            match event_tx.try_send(ev) {
                Ok(()) => Response::Ok { message: "accepted".into(), state: None },
                Err(_) => {
                    dropped_events.fetch_add(1, Ordering::Relaxed);
                    Response::BatchOk { accepted: 0, rejected: 1 }
                }
            }
        },
        Request::ReportConnections { events } => {
            let now = epoch_ns();
            let mut accepted = 0u32;
            let mut rejected = 0u32;
            for cr in events {
                let ev = ConnectionEvent {
                                        ip: match cr.ip.parse() {
                        Ok(addr) => addr,
                        Err(_) => {
                            rejected += 1;
                            continue;
                        }
                    },
                    timestamp_ns: now,
                    bytes: cr.bytes,
                    status_code: cr.status_code,
                    proto_fingerprint: cr.proto_fp,
                };
                match event_tx.try_send(ev) {
                    Ok(()) => accepted += 1,
                    Err(e) => {
                        rejected += 1;
                        dropped_events.fetch_add(1, Ordering::Relaxed);
                        debug!("tx full: {:?}", e);
                    }
                }
                if accepted + rejected >= BATCH_MAX as u32 {
                    break;
                }
            }
            debug!("report_connections: accepted={} rejected={}", accepted, rejected);
            Response::BatchOk { accepted, rejected }
        },
        Request::Flush => Response::Ok { message: "flushed".into(), state: None },
    }
}