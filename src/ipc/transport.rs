//! Transport abstraction layer for IPC connections
//!
//! This module provides a unified transport abstraction that works with
//! both TCP and Unix sockets, similar to jcode's transport layer.
//! It enables cleaner connection handling and better testability.

use anyhow::Result;
use std::path::Path;
use tokio::net::TcpListener;
use tokio::net::TcpStream;

/// Transport trait for different connection types
pub trait Transport: Send + 'static {
    type Listener: Listener<Stream = Self>;
    type Stream: Stream;

    fn connect(addr: &str) -> impl std::future::Future<Output = Result<Self::Stream>> + Send;
    fn listen(addr: &str) -> impl std::future::Future<Output = Result<Self::Listener>> + Send;
}

/// Listener trait for accepting connections
#[async_trait::async_trait]
pub trait Listener {
    type Stream: Stream;
    async fn accept(&mut self) -> Result<(Self::Stream, String)>;
}

/// Stream trait for reading/writing
#[async_trait::async_trait]
pub trait Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {
    fn peer_addr(&self) -> Option<String>;
    async fn shutdown(&mut self) -> Result<()>;
}

/// TCP transport implementation
pub struct TcpTransport;

#[async_trait::async_trait]
impl Transport for TcpTransport {
    type Listener = TcpListener;
    type Stream = TcpStream;

    async fn connect(addr: &str) -> Result<TcpStream> {
        TcpStream::connect(addr).await.map_err(Into::into)
    }

    async fn listen(addr: &str) -> Result<TcpListener> {
        TcpListener::bind(addr).await.map_err(Into::into)
    }
}

#[async_trait::async_trait]
impl Listener for TcpListener {
    type Stream = TcpStream;

    async fn accept(&mut self) -> Result<(TcpStream, String)> {
        let (stream, addr) = self.accept().await?;
        Ok((stream, addr.to_string()))
    }
}

#[async_trait::async_trait]
impl Stream for TcpStream {
    fn peer_addr(&self) -> Option<String> {
        self.peer_addr().ok().map(|a| a.to_string())
    }

    async fn shutdown(&mut self) -> Result<()> {
        self.shutdown().await.map_err(Into::into)
    }
}

/// Configuration for the IPC transport
#[derive(Debug, Clone)]
pub struct TransportConfig {
    pub bind_addr: String,
    pub max_connections: usize,
    pub max_connection_bytes: usize,
    pub read_timeout_ms: u64,
    pub write_timeout_ms: u64,
    pub idle_timeout_ms: u64,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:7890".to_string(),
            max_connections: 1024,
            max_connection_bytes: 1_048_576,
            read_timeout_ms: 5000,
            write_timeout_ms: 5000,
            idle_timeout_ms: 30_000,
        }
    }
}

/// Connection metadata
#[derive(Debug, Clone)]
pub struct ConnectionMeta {
    pub peer_addr: String,
    pub connected_at: std::time::Instant,
    pub bytes_read: u64,
    pub bytes_written: u64,
}

impl ConnectionMeta {
    pub fn new(peer_addr: String) -> Self {
        Self {
            peer_addr,
            connected_at: std::time::Instant::now(),
            bytes_read: 0,
            bytes_written: 0,
        }
    }
}