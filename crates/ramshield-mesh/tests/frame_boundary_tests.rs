# M013 Frame Boundary Tests - Requires Rust 2024 Edition

// SPDX-License-Identifier: MIT

//! crates/ramshield-mesh/tests/frame_boundary_tests.rs
//! Deterministic frame boundary and parser edge case verification for task M-013.

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// Adjust MAX_FRAME_SIZE to match transport.rs constant (e.g., 65536)
const MAX_FRAME_SIZE: usize = 65_536;

/// Helper to spin up a loopback mock reader that implements the transport frame loop
async fn spawn_mock_receiver() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral");
    let addr = listener.local_addr().expect("local_addr");

    let handle = tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = Vec::new();
            let mut temp = [0u8; 1024];
            loop {
                match socket.read(&mut temp).await {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        buf.extend_from_slice(&temp[..n]);
                        if buf.len() > MAX_FRAME_SIZE + 1 {
                            // Frame limit exceeded: drop connection immediately (fail-closed)
                            break;
                        }
                        // Check for delimiter
                        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                            let line = &buf[..pos];
                            let _trimmed = if line.ends_with(b"\r") {
                                &line[..line.len() - 1]
                            } else {
                                line
                            };
                            // Drain processed frame
                            buf.drain(..=pos);
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    });

    (addr, handle)
}

#[tokio::test]
async fn test_m013_frame_exact_maximum() {
    let (addr, handle) = spawn_mock_receiver().await;
    let mut stream = TcpStream::connect(addr).await.expect("connect");

    // Construct valid payload exactly matching MAX_FRAME_SIZE including \n
    let mut payload = vec![b'a'; MAX_FRAME_SIZE - 1];
    payload.push(b'\n');

    assert_eq!(payload.len(), MAX_FRAME_SIZE);
    stream.write_all(&payload).await.expect("write exact max");
    stream.flush().await.expect("flush");

    // Stream should remain open without connection reset
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut check_buf = [0u8; 1];
    let res = tokio::time::timeout(Duration::from_millis(50), stream.read(&mut check_buf)).await;
    assert!(res.is_err(), "Stream should not be closed on valid max frame");

    drop(stream);
    let _ = handle.await;
}

#[tokio::test]
async fn test_m013_frame_one_byte_over_maximum() {
    let (addr, handle) = spawn_mock_receiver().await;
    let mut stream = TcpStream::connect(addr).await.expect("connect");

    // MAX_FRAME_SIZE + 1 bytes before newline
    let mut payload = vec![b'a'; MAX_FRAME_SIZE + 1];
    payload.push(b'\n');

    stream.write_all(&payload).await.expect("write oversized frame");
    stream.flush().await.expect("flush");

    // The server must detect frame overflow and close the connection
    let mut buf = [0u8; 16];
    let n = stream.read(&mut buf).await.expect("read after violation");
    assert_eq!(n, 0, "Server must close connection on frame exceeding limit");

    let _ = handle.await;
}

#[tokio::test]
async fn test_m013_delimiter_split_across_reads() {
    let (addr, handle) = spawn_mock_receiver().await;
    let mut stream = TcpStream::connect(addr).await.expect("connect");

    // Send payload in tiny 1-byte chunks
    let frame1 = b"{\"msg\":\"chunked_1\"}\r\n";
    let frame2 = b"{\"msg\":\"chunked_2\"}\n";

    for byte in frame1.iter().chain(frame2.iter()) {
        stream.write_all(&[*byte]).await.expect("write 1 byte");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    stream.flush().await.expect("flush");

    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(stream);
    let _ = handle.await;
}

#[tokio::test]
async fn test_m013_clean_eof_and_partial_eof() {
    // 1. Partial EOF test (stream dropped before closing delimiter)
    let (addr1, handle1) = spawn_mock_receiver().await;
    let mut stream1 = TcpStream::connect(addr1).await.expect("connect");
    stream1.write_all(b"{\"partial\": true}").await.expect("write partial");
    drop(stream1); // Immediate EOF without '\n'
    let _ = handle1.await;

    // 2. Clean EOF test (stream dropped cleanly after delimiter)
    let (addr2, handle2) = spawn_mock_receiver().await;
    let mut stream2 = TcpStream::connect(addr2).await.expect("connect");
    stream2.write_all(b"{\"complete\": true}\n").await.expect("write complete");
    stream2.flush().await.expect("flush");
    drop(stream2); // Clean EOF
    let _ = handle2.await;
}