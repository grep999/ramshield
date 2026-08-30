use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    time::{Duration, Instant, timeout},
};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::{Request, Response};
use crate::config::Config;
use crate::engine::Engine;
use crate::storage::Store;
use ramshield_types::ConnectionEvent;
use ramshield_types::{EnforceAction, EnforceCommand};

/// Connection handling configuration
#[derive(Clone)]
struct ConnectionConfig {
    max_bytes: usize,
    read_timeout: Duration,
    write_timeout: Duration,
    idle_timeout: Duration,
    auth_keys: Arc<Vec<(String, Vec<u8>)>>,
}

impl ConnectionConfig {
    fn from_server(server: &IpcServer) -> Self {
        Self {
            max_bytes: server.max_connection_bytes,
            read_timeout: Duration::from_millis(server.read_timeout_ms),
            write_timeout: Duration::from_millis(server.write_timeout_ms),
            idle_timeout: Duration::from_millis(server.connection_idle_timeout_ms),
            auth_keys: Arc::new(server.auth_keys.clone()),
        }
    }
}

// Tunable constants or defaults (can be moved to config.rs)
const DEFAULT_MAX_CONNECTION_BYTES: usize = 1_048_576; // 1MB per connection
const DEFAULT_READ_TIMEOUT_MS: u64 = 5000;
const DEFAULT_WRITE_TIMEOUT_MS: u64 = 5000;
const BATCH_MAX: usize = 1_000_000; // channel holds 2M; try_send backpressure is the real limiter
const MAX_LINE_LENGTH: usize = 33_554_432; // 32MB max single line (batch reports)
const CONNECTION_IDLE_TIMEOUT_MS: u64 = 30_000; // 30s idle

pub struct IpcServer {
    listener: TcpListener,
    engine: Arc<Engine>,
    /// (key_id, key_bytes) pairs; empty = auth disabled.
    auth_keys: Vec<(String, Vec<u8>)>,
    event_tx: Sender<ConnectionEvent>,
    store: Arc<Store>,
    enforcement_tx: mpsc::Sender<EnforceCommand>,
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
        enforcement_tx: mpsc::Sender<EnforceCommand>,
    ) -> std::io::Result<Self> {
        let addr = config.ipc.tcp_addr.clone();
        info!("IPC server binding to {}", addr);
        let listener = TcpListener::bind(&addr).await?;
        info!("IPC server bound to {}", addr);

        let max_connections = config.ipc.max_connections.max(1);
        let max_connection_bytes = config
            .ipc
            .max_connection_bytes
            .unwrap_or(DEFAULT_MAX_CONNECTION_BYTES);
        let read_timeout_ms = config
            .ipc
            .read_timeout_ms
            .unwrap_or(DEFAULT_READ_TIMEOUT_MS);
        let write_timeout_ms = config
            .ipc
            .write_timeout_ms
            .unwrap_or(DEFAULT_WRITE_TIMEOUT_MS);
        let connection_idle_timeout_ms = config
            .ipc
            .connection_idle_timeout_ms
            .unwrap_or(CONNECTION_IDLE_TIMEOUT_MS);
        let mut auth_keys: Vec<(String, Vec<u8>)> = Vec::new();
        for entry in &config.ipc.auth_keys {
            match entry.split_once(':') {
                Some((id, hexkey)) => match hex::decode(hexkey.trim()) {
                    Ok(k) if !k.is_empty() => auth_keys.push((id.trim().to_string(), k)),
                    _ => warn!("ipc.auth_keys: invalid hex key for '{}', skipped", id),
                },
                None => warn!("ipc.auth_keys: expected 'key_id:hex_key', got '{}'", entry),
            }
        }
        if !auth_keys.is_empty() {
            info!("IPC HMAC auth ENABLED ({} key(s))", auth_keys.len());
        }

        Ok(Self {
            listener,
            engine,
            auth_keys,
            event_tx,
            store,
            enforcement_tx,
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
    enforcement_tx: mpsc::Sender<EnforceCommand>,
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
            // F4: tell the client WHY instead of a bare TCP reset. Best-effort —
            // client may already be gone; do not read past the cap to find a newline.
            let resp = Response::Error {
                code: 413,
                message: format!(
                    "connection exceeded max_connection_bytes ({})",
                    config.max_bytes
                ),
            };
            let _ = timeout(config.write_timeout, write_resp(&mut socket, &resp)).await;
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

        if n == 0 {
            return Ok(());
        }

        total_bytes_read += n;
        last_activity = Instant::now();
        buf.extend_from_slice(&chunk[..n]);

        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            if pos > MAX_LINE_LENGTH {
                warn!(
                    "Line exceeds max length ({}), dropping connection",
                    MAX_LINE_LENGTH
                );
                return Ok(());
            }
            let line: Vec<u8> = buf.drain(..=pos).collect();

            // HMAC auth gate: enforced only when keys configured. The auth
            // object rides OUTSIDE the Request enum so deny_unknown_fields
            // on the wire contract stays intact.
            let mut line = line;
            if !config.auth_keys.is_empty() {
                match verify_frame_auth(&config.auth_keys, &line) {
                    Ok(sanitized) => {
                        // Continue parsing the auth-stripped payload so
                        // Request's deny_unknown_fields never sees `auth`.
                        line = sanitized;
                    }
                    Err(reason) => {
                        warn!("IPC auth rejected: {}", reason);
                        engine.metrics.inc_rejected(1);
                        let resp = Response::Error {
                            code: 401,
                            message: format!("unauthorized: {}", reason),
                        };
                        if timeout(config.write_timeout, write_resp(&mut socket, &resp))
                            .await
                            .is_err()
                        {
                            return Ok(());
                        }
                        continue;
                    }
                }
            }

            let req: Request = match serde_json::from_slice(&line) {
                Ok(r) => r,
                Err(e) => {
                    let resp = Response::Error {
                        code: 1,
                        message: format!("parse: {}", e),
                    };
                    if timeout(config.write_timeout, write_resp(&mut socket, &resp))
                        .await
                        .is_err()
                    {
                        debug!("Write timeout on error response");
                        return Ok(());
                    }
                    continue;
                }
            };

            engine.metrics.inc_requests();
            let resp = process_request(
                req,
                &engine,
                &event_tx,
                &store,
                &enforcement_tx,
                dropped_events.clone(),
            );
            if timeout(config.write_timeout, write_resp(&mut socket, &resp))
                .await
                .is_err()
            {
                debug!("Write timeout");
                return Ok(());
            }
        }
    }
}

async fn write_resp(socket: &mut TcpStream, resp: &Response) -> Result<(), std::io::Error> {
    let bytes = serde_json::to_vec(resp)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
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
    engine: &Arc<Engine>,
    event_tx: &Sender<ConnectionEvent>,
    store: &Store,
    enforcement_tx: &mpsc::Sender<EnforceCommand>,
    dropped_events: Arc<AtomicU64>,
) -> Response {
    match req {
        Request::CheckIp { ip } => {
            let ip_addr = match ip.parse() {
                Ok(addr) => addr,
                Err(_) => {
                    return Response::Error {
                        code: 400,
                        message: format!("invalid ip address: {}", ip),
                    };
                }
            };

            let status = store.get(&ip_addr);
            let (blocked, threat, ewma_rps, reason) = match status {
                Some(crate::storage::Value::IpRecord(rec)) => (
                    rec.block_state != crate::storage::BlockState::Clean,
                    rec.threat_score,
                    rec.ewma_rps,
                    match rec.block_state {
                        crate::storage::BlockState::Blocked { ref reason, .. } => {
                            Some(reason.as_str().to_string())
                        }
                        _ => None,
                    },
                ),
                _ => (false, 0.0, 0.0, None),
            };
            Response::IpStatus {
                ip,
                blocked,
                threat,
                ewma_rps,
                reason,
            }
        }
        Request::BlockIp {
            ip,
            reason,
            ttl_secs,
        } => {
            let ip_addr = match ip.parse() {
                Ok(addr) => addr,
                Err(_) => {
                    return Response::Error {
                        code: 400,
                        message: format!("invalid ip address: {}", ip),
                    };
                }
            };

            let reason_display = if reason.is_empty() {
                "manual_block".to_string()
            } else {
                reason.clone()
            };
            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 1,
                source: "ipc".into(),
                actor: "admin".into(),
                timestamp_utc: epoch_ns() as i64 / 1_000_000_000,
                ttl_seconds: ttl_secs.unwrap_or(0),
                reason,
                ip: ip_addr,
                action: EnforceAction::Block,
            };
            match enforcement_tx.try_send(cmd) {
                Ok(()) => {
                    engine.metrics.record_block(
                        &ip,
                        &reason_display,
                        "ipc",
                    );
                    Response::Ok {
                        message: format!("block queued for {}", ip_addr),
                        state: Some("pending".into()),
                    }
                }
                Err(_) => Response::Error {
                    code: 503,
                    message: "enforcement queue full".into(),
                },
            }
        }
        Request::UnblockIp { ip } => {
            let ip_addr = match ip.parse() {
                Ok(addr) => addr,
                Err(_) => {
                    return Response::Error {
                        code: 400,
                        message: format!("invalid ip address: {}", ip),
                    };
                }
            };

            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 1,
                source: "ipc".into(),
                actor: "admin".into(),
                timestamp_utc: epoch_ns() as i64 / 1_000_000_000,
                ttl_seconds: 0,
                reason: "manual_unblock".into(),
                ip: ip_addr,
                action: EnforceAction::Unblock,
            };
            match enforcement_tx.try_send(cmd) {
                Ok(()) => Response::Ok {
                    message: format!("unblock queued for {}", ip_addr),
                    state: Some("pending".into()),
                },
                Err(_) => Response::Error {
                    code: 503,
                    message: "enforcement queue full".into(),
                },
            }
        }
        Request::GetIpStats { ip } => {
            let ip_addr = match ip.parse() {
                Ok(addr) => addr,
                Err(_) => {
                    return Response::Error {
                        code: 400,
                        message: format!("invalid ip address: {}", ip),
                    };
                }
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
        }
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
        }
        Request::GetStatus => Response::Ok {
            message: "ok".into(),
            state: None,
        },
        Request::ReportConnection {
            ip,
            bytes,
            status_code,
            proto_fp,
        } => {
            let ev = ConnectionEvent {
                ip: match ip.parse() {
                    Ok(addr) => addr,
                    Err(_) => {
                        return Response::Error {
                            code: 400,
                            message: format!("invalid ip address: {}", ip),
                        };
                    }
                },
                timestamp_ns: epoch_ns(),
                bytes,
                status_code,
                proto_fingerprint: proto_fp,
            };
            match event_tx.try_send(ev) {
                Ok(()) => Response::Ok {
                    message: "accepted".into(),
                    state: None,
                },
                Err(_) => {
                    dropped_events.fetch_add(1, Ordering::Relaxed);
                    Response::BatchOk {
                        accepted: 0,
                        rejected: 1,
                    }
                }
            }
        }
        Request::ReportConnections { events } => {
            let now = epoch_ns();
            let total = events.len() as u32;
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
                if accepted + rejected >= BATCH_MAX as u32 && total > accepted + rejected {
                    let dropped = total - accepted - rejected;
                    rejected += dropped;
                    dropped_events.fetch_add(dropped as u64, Ordering::Relaxed);
                    break;
                }
            }
            debug!(
                "report_connections: accepted={} rejected={}",
                accepted, rejected
            );
            Response::BatchOk { accepted, rejected }
        }
        Request::Flush => Response::Ok {
            message: "no-op: flush is automatic (pre_aggs window)".into(),
            state: None,
        },
    }
}

/// Verify the HMAC auth envelope on a raw frame line.
/// Expected shape: `{"auth":{"key_id":..,"ts_ms":..,"sig":..},"type":..,...}`.
/// The signature covers `<ts_ms>.<full frame bytes minus the auth object>` —
/// simplest correct scheme: signer strips `auth` field, signs remaining JSON
/// bytes with ts prefix. Here we sign the RAW LINE as sent by the client
/// including its auth object? No — sig must cover payload WITHOUT auth object,
/// else self-reference. Client signs `ts.payload_without_auth`; server removes
/// the auth object, re-serializes compactly and compares.
fn verify_frame_auth(keys: &[(String, Vec<u8>)], line: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut v: serde_json::Value =
        serde_json::from_slice(line).map_err(|_| "frame is not valid JSON")?;
    let auth = v
        .as_object_mut()
        .ok_or("frame is not an object")?
        .remove("auth")
        .ok_or("missing auth object")?;
    let obj = auth.as_object().ok_or("auth is not an object")?;
    let key_id = obj
        .get("key_id")
        .and_then(|x| x.as_str())
        .ok_or("auth.key_id missing")?;
    let ts_ms = obj
        .get("ts_ms")
        .and_then(|x| x.as_u64())
        .ok_or("auth.ts_ms missing")?;
    let sig = obj
        .get("sig")
        .and_then(|x| x.as_str())
        .ok_or("auth.sig missing")?;

    // Payload = compact serialization of the frame without the auth object.
    let payload = serde_json::to_vec(&v).map_err(|_| "reserialize failed")?;
    ramshield_protocol::auth::verify(keys, key_id, ts_ms, sig, &payload)?;
    Ok(payload)
}
