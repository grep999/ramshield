use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};
use tracing::{debug, error, info};
use std::sync::Arc;
use crossbeam_channel::Sender;

use crate::config::Config;
use crate::detection::ConnectionEvent;
use crate::engine::Engine;
use super::{Request, Response};

const MAX_CONNECTIONS: usize = 1024;
const BATCH_MAX: usize = 4096;

pub struct IpcServer {
    listener: TcpListener,
    engine: Arc<Engine>,
    event_tx: Sender<ConnectionEvent>,
    semaphore: Arc<Semaphore>,
}

impl IpcServer {
    pub async fn bind(
        config: &Config,
        engine: Arc<Engine>,
        event_tx: Sender<ConnectionEvent>,
    ) -> std::io::Result<Self> {
        let addr = config.ipc.tcp_addr.clone();
        info!("IPC server binding to {}", addr);
        let listener = TcpListener::bind(&addr).await?;
        info!("IPC server bound to {}", addr);
        Ok(Self {
            listener,
            engine,
            event_tx,
            semaphore: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
        })
    }

    pub async fn start(&self) {
        info!("IPC server listening");
        loop {
            if self.engine.is_shutting_down() {
                info!("IPC server shutting down");
                break;
            }
            let accept = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                self.listener.accept(),
            )
            .await;
            let (socket, remote) = match accept {
                Ok(Ok(pair)) => pair,
                Ok(Err(e)) => {
                    error!("accept error: {}", e);
                    continue;
                }
                Err(_) => continue,
            };
            let permit = self.semaphore.clone().acquire_owned().await;
            let engine = self.engine.clone();
            let event_tx = self.event_tx.clone();
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(e) = handle_connection(socket, engine, event_tx).await {
                    debug!("conn {} closed: {}", remote, e);
                }
            });
        }
    }
}

async fn handle_connection(
    mut socket: TcpStream,
    engine: Arc<Engine>,
    event_tx: Sender<ConnectionEvent>,
) -> Result<(), std::io::Error> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        let n = socket.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let req: Request = match serde_json::from_slice(&line) {
                Ok(r) => r,
                Err(e) => {
                    let resp = Response::Error {
                        code: 1,
                        message: format!("parse: {}", e),
                    };
                    write_resp(&mut socket, &resp).await?;
                    continue;
                }
            };
            let resp = process_request(req, &engine, &event_tx);
            write_resp(&mut socket, &resp).await?;
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

fn process_request(
    req: Request,
    _engine: &Engine,
    event_tx: &Sender<ConnectionEvent>,
) -> Response {
    match req {
        Request::CheckIp { ip } => Response::IpStatus {
            ip,
            blocked: false,
            threat: 0.0,
            ewma_rps: 0.0,
            reason: None,
        },
        Request::BlockIp { ip, reason, ttl_secs } => Response::Ok {
            message: format!("blocked {} ttl={:?} reason={}", ip, ttl_secs, reason),
        },
        Request::UnblockIp { ip } => Response::Ok {
            message: format!("unblocked {}", ip),
        },
        Request::GetIpStats { ip } => Response::IpDetail(crate::ipc::IpDetail {
            ip: ip,
            count: 0,
            ewma_rps: 0.0,
            threat: 0.0,
            state: "unknown".into(),
            bytes_in: 0,
            first_seen_s: 0,
            last_seen_s: 0,
        }),
        Request::GetStats => Response::Stats(crate::ipc::Stats {
            ips_tracked: 0,
            blocked: 0,
            ram_bytes: 0,
            ram_limit_mb: 0,
            uptime_secs: 0,
            evictions: 0,
        }),
        Request::GetStatus => Response::Ok { message: "ok".into() },
        Request::ReportConnection { ip, bytes, status_code, proto_fp } => {
            let ev = ConnectionEvent {
                ip: ip.parse().unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)),
                timestamp_ns: epoch_ns(),
                bytes,
                status_code,
                proto_fingerprint: proto_fp,
            };
            let accepted = event_tx.send(ev).is_ok();
            if accepted {
                Response::Ok { message: "accepted".into() }
            } else {
                Response::BatchOk { accepted: 0, rejected: 1 }
            }
        },
        Request::ReportConnections { events } => {
            let now = epoch_ns();
            let mut accepted = 0u32;
            let mut rejected = 0u32;
            for cr in events {
                let ev = ConnectionEvent {
                    ip: cr.ip.parse().unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)),
                    timestamp_ns: now,
                    bytes: cr.bytes,
                    status_code: cr.status_code,
                    proto_fingerprint: cr.proto_fp,
                };
                match event_tx.try_send(ev) {
                    Ok(()) => accepted += 1,
                    Err(_) => rejected += 1,
                }
                if accepted + rejected >= BATCH_MAX as u32 {
                    break;
                }
            }
            Response::BatchOk { accepted, rejected }
        },
        Request::Flush => Response::Ok { message: "flushed".into() },
    }
}

fn epoch_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
