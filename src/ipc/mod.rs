use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::config::IpcConfig;
use crate::metrics::Metrics;
use crate::detection::DetectionEngine;
use crate::detection::batch::ConnectionEvent;
use crate::error::RamShieldError;
use crate::storage::Store;

use serde::{Deserialize, Serialize};

/// One JSON request per line, one JSON response per line.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcRequest {
    ReportConnection {
        ip: String,
        #[serde(default)]
        bytes: u64,
        #[serde(default)]
        status_code: u16,
        #[serde(default, rename = "proto_fp")]
        proto_fingerprint: u32,
    },
    ReportConnections {
        events: Vec<IpcEvent>,
    },
    CheckIp {
        ip: String,
    },
    BlockIp {
        ip: String,
        #[serde(default = "default_reason")]
        reason: String,
        #[serde(default)]
        ttl_secs: Option<u64>,
    },
    UnblockIp {
        ip: String,
    },
    GetStats,
    GetIpStats {
        ip: String,
    },
    Flush,
}

fn default_reason() -> String {
    "manual".to_string()
}

/// Individual event in a `report_connections` batch.
#[derive(Debug, Serialize, Deserialize)]
pub struct IpcEvent {
    pub ip: String,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub status_code: u16,
    #[serde(default, rename = "proto_fp")]
    pub proto_fingerprint: u32,
}

impl From<IpcEvent> for ConnectionEvent {
    fn from(e: IpcEvent) -> Self {
        ConnectionEvent {
            ip: e.ip,
            timestamp_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
            bytes: e.bytes,
            status_code: e.status_code,
            proto_fingerprint: e.proto_fingerprint,
        }
    }
}

/// IPC response — one JSON object per line.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcResponse {
    Ok {
        message: String,
    },
    BatchOk {
        accepted: usize,
        rejected: usize,
    },
    IpStatus {
        ip: String,
        blocked: bool,
        reason: Option<String>,
    },
    Stats {
        ips_tracked: usize,
        blocked_total: u64,
        events_ingested: u64,
        events_rejected: u64,
        requests_total: u64,
        batches_total: u64,
    },
    IpStats {
        ip: String,
        blocked: bool,
        reason: Option<String>,
        event: Option<serde_json::Value>,
    },
    Flushed,
    Error {
        code: u16,
        message: String,
    },
}

impl IpcResponse {
    fn error(code: u16, msg: impl Into<String>) -> Self {
        IpcResponse::Error {
            code,
            message: msg.into(),
        }
    }
}

pub struct IpcServer {
    config: IpcConfig,
    detection_engine: Arc<DetectionEngine>,
    metrics: Arc<Metrics>,
    store: Arc<Store>,
}

impl IpcServer {
    pub fn new(
        config: IpcConfig,
        detection_engine: Arc<DetectionEngine>,
        metrics: Arc<Metrics>,
        store: Arc<Store>,
    ) -> Self {
        Self {
            config,
            detection_engine,
            metrics,
            store,
        }
    }

    pub async fn start(
        &self,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<(), RamShieldError> {
        let addr = self.config.tcp_addr.clone();
        info!("IPC Server: Starting on {}", addr);

        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| RamShieldError::IpcError(format!("Failed to bind to {}: {}", addr, e)))?;

        info!("IPC Server: Listening on {}", addr);

        let max_conn = self.config.max_connections.max(1);

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("IPC Server: Shutdown signal received.");
                    break;
                }
                accept = listener.accept() => {
                    let (stream, peer) = match accept {
                        Ok(s) => s,
                        Err(e) => {
                            warn!("IPC Server: accept error: {}", e);
                            continue;
                        }
                    };
                    info!("IPC Server: Connection from {}", peer);

                    // Simple connection limit: best-effort, not strict.
                    // Each connection is spawned as its own task.
                    tokio::spawn(Self::handle_connection(
                        stream,
                        peer.to_string(),
                        self.detection_engine.clone(),
                        self.metrics.clone(),
                        self.store.clone(),
                        max_conn,
                    ));
                }
            }
        }

        info!("IPC Server: Shutting down.");
        Ok(())
    }

    async fn handle_connection(
        stream: tokio::net::TcpStream,
        peer: String,
        detection: Arc<DetectionEngine>,
        metrics: Arc<Metrics>,
        store: Arc<Store>,
        _max_conn: usize,
    ) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let response = Self::handle_request(&line, &detection, &metrics, &store);
                    let json = serde_json::to_string(&response).unwrap_or_else(|_| {
                        r#"{"type":"error","code":500,"message":"serialization failed"}"#.to_string()
                    });
                    if writer.write_all(format!("{}\n", json).as_bytes()).await.is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    // Client closed connection
                    break;
                }
                Err(e) => {
                    warn!("IPC Server: read error from {}: {}", peer, e);
                    break;
                }
            }
        }

        info!("IPC Server: Connection from {} closed.", peer);
    }

    fn handle_request(
        line: &str,
        detection: &DetectionEngine,
        metrics: &Metrics,
        store: &Store,
    ) -> IpcResponse {
        let req: IpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                return IpcResponse::error(400, format!("invalid request: {}", e));
            }
        };

        match req {
            IpcRequest::ReportConnection {
                ip,
                bytes,
                status_code,
                proto_fingerprint,
            } => {
                let event = ConnectionEvent {
                    ip,
                    timestamp_ns: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos() as u64)
                        .unwrap_or(0),
                    bytes,
                    status_code,
                    proto_fingerprint,
                };
                match detection.submit_event(event) {
                    Ok(()) => IpcResponse::Ok {
                        message: "accepted".to_string(),
                    },
                    Err(e) => IpcResponse::error(503, format!("channel full: {}", e)),
                }
            }
            IpcRequest::ReportConnections { events } => {
                let mut accepted = 0usize;
                let mut rejected = 0usize;
                for ev in events {
                    let event: ConnectionEvent = ev.into();
                    match detection.submit_event(event) {
                        Ok(()) => accepted += 1,
                        Err(_) => rejected += 1,
                    }
                }
                IpcResponse::BatchOk { accepted, rejected }
            }
            IpcRequest::CheckIp { ip } => {
                let blocked = store.is_blocked(&ip);
                let reason = store.blocks.get(&ip).map(|e| e.reason.clone());
                IpcResponse::IpStatus {
                    ip,
                    blocked,
                    reason,
                }
            }
            IpcRequest::BlockIp {
                ip,
                reason,
                ttl_secs,
            } => {
                if !store.is_blocked(&ip) {
                    store.block_ip(&ip, &reason, ttl_secs);
                    metrics.record_block(&ip, &reason, "manual");
                    metrics.inc_blocks();
                }
                IpcResponse::Ok {
                    message: format!("blocked {}", ip),
                }
            }
            IpcRequest::UnblockIp { ip } => {
                let removed = store.unblock_ip(&ip);
                IpcResponse::Ok {
                    message: if removed {
                        format!("unblocked {}", ip)
                    } else {
                        format!("ip {} was not blocked", ip)
                    },
                }
            }
            IpcRequest::GetStats => {
                use std::sync::atomic::Ordering;
                IpcResponse::Stats {
                    ips_tracked: store.ips_tracked(),
                    blocked_total: store.blocks_lifetime(),
                    events_ingested: metrics.events_ingested.load(Ordering::Relaxed),
                    events_rejected: metrics.events_rejected.load(Ordering::Relaxed),
                    requests_total: metrics.requests_total.load(Ordering::Relaxed),
                    batches_total: metrics.batches_total.load(Ordering::Relaxed),
                }
            }
            IpcRequest::GetIpStats { ip } => {
                let blocked = store.is_blocked(&ip);
                let reason = store.blocks.get(&ip).map(|e| e.reason.clone());
                let event = store.data.get(&ip).map(|e| {
                    serde_json::json!({
                        "ip": e.ip,
                        "timestamp_ns": e.timestamp_ns,
                        "bytes": e.bytes,
                        "status_code": e.status_code,
                        "proto_fingerprint": e.proto_fingerprint,
                    })
                });
                IpcResponse::IpStats {
                    ip,
                    blocked,
                    reason,
                    event,
                }
            }
            IpcRequest::Flush => {
                store.flush();
                IpcResponse::Flushed
            }
        }
    }
}