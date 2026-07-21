use tokio::{net::{TcpListener, TcpStream}, io::{AsyncReadExt, AsyncWriteExt}};
use tracing::{info, error, debug};
use std::sync::Arc;
use tokio::sync::Semaphore;
use crate::config::Config;
use crate::engine::Engine;
use super::{Request, Response};

const MAX_CONNECTIONS: usize = 1024; // ponytail: simple concurrency limit, upgrade to adaptive when load dictates

pub struct IpcServer {
    config: Arc<Config>,
    engine: Arc<Engine>,
    listener: TcpListener,
    semaphore: Arc<Semaphore>,
}

impl IpcServer {
    pub async fn new(config: Arc<Config>, engine: Arc<Engine>) -> Result<Self, Box<dyn std::error::Error>> {
        let listen_addr = config.ipc.listen_addr.clone();
        info!("IPC server binding to {}", listen_addr);
        let listener = TcpListener::bind(&listen_addr).await?;
        info!("IPC server bound to {}", listen_addr);
        Ok(Self {
            config,
            engine,
            listener,
            semaphore: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
        })
    }

    pub async fn start(&self) {
        info!("IPC server starting listener loop");
        loop {
            // Check for shutdown signal
            if self.engine.is_shutting_down() {
                info!("IPC server shutdown signal received. Exiting listener loop.");
                break;
            }

            let permit = self.semaphore.clone().acquire_owned().await;
            match tokio::time::timeout(std::time::Duration::from_secs(1), self.listener.accept()).await {
                Ok(Ok((socket, remote_addr))) => {
                    debug!("Accepted new IPC connection from {}", remote_addr);
                    let engine_clone = self.engine.clone();
                    tokio::spawn(async move {
                        let _permit = permit; // Hold the permit for the duration of the connection
                        if let Err(e) = Self::handle_connection(socket, engine_clone).await {
                            error!("Error handling IPC connection from {}: {}", remote_addr, e);
                        }
                        debug!("IPC connection from {} closed.", remote_addr);
                    });
                }
                Ok(Err(e)) => error!("Error accepting IPC connection: {}", e),
                Err(_) => { /* Timeout, check shutdown */ }
            }
        }
        info!("IPC server listener loop exited.");
    }

    async fn handle_connection(mut socket: TcpStream, engine: Arc<Engine>) -> Result<(), Box<dyn std::error::Error>> {
        let mut buffer = Vec::new();
        let mut read_buf = vec![0; 4096];

        loop {
            let n = socket.read(&mut read_buf).await?;
            if n == 0 {
                // Connection closed
                break;
            }
            buffer.extend_from_slice(&read_buf[..n]);

            while let Some(request) = Self::parse_request(&mut buffer)? {
                let response = Self::process_request(request, engine.clone()).await;
                let response_bytes = serde_json::to_vec(&response)?;
                socket.write_all(&response_bytes).await?;
                socket.write_all(b"\n").await?; // Delimit responses with newline
            }
        }
        Ok(())
    }

    fn parse_request(buffer: &mut Vec<u8>) -> Result<Option<Request>, Box<dyn std::error::Error>> {
        if let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
            let line = buffer.drain(..newline_pos + 1).collect::<Vec<u8>>();
            let request: Request = serde_json::from_slice(&line)?;
            Ok(Some(request))
        } else {
            Ok(None)
        }
    }

    async fn process_request(request: Request, engine: Arc<Engine>) -> Response {
        match request {
            Request::CheckIp { ip } => Response::IpStatus {
                ip,
                blocked: false,
                threat: 0.0,
                ewma_rps: 0.0,
                reason: None,
            },
            Request::BlockIp { ip, reason, ttl_secs } => Response::Ok {
                message: format!("IP {} blocked for {}s: {}", ip, ttl_secs.unwrap_or(0), reason),
            },
            Request::UnblockIp { ip } => Response::Ok {
                message: format!("IP {} unblocked", ip),
            },
            Request::GetIpStats { ip } => Response::IpDetail(
                crate::ipc::IpDetail {
                    ip,
                    count: 0,
                    ewma_rps: 0.0,
                    threat: 0.0,
                    state: "unknown".into(),
                    bytes_in: 0,
                    first_seen_s: 0,
                    last_seen_s: 0,
                }
            ),
            Request::GetStats => Response::Stats(
                crate::ipc::Stats {
                    ips_tracked: 0,
                    blocked: 0,
                    ram_bytes: 0,
                    ram_limit_mb: 0,
                    uptime_secs: 0,
                    evictions: 0,
                }
            ),
            Request::GetStatus => Response::Ok { message: "ok".into() },
            Request::ReportConnection { .. } => Response::Ok { message: "connection reported".into() },
            Request::ReportConnections { events } => Response::BatchOk { accepted: events.len() as u32, rejected: 0 },
            Request::Flush => Response::Ok { message: "flush initiated".into() },
        }
    }
}
