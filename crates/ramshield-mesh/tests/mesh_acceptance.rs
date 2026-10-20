//! End-to-end tests for the public mesh transport boundary.
//!
//! These tests intentionally use loopback TCP sockets rather than only testing
//! frame helpers, so they exercise listener startup, framing, HMAC validation,
//! and the receive queue together.

use ramshield_mesh::{
    aworset::{AworsetBlocklist, ClusterBlockDelta, ClusterDot},
    transport::{MeshHandle, MeshMessage, MeshPeer},
};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};

const NODE_1_KEY: &[u8] = b"mesh-node-one-auth-key-at-least-32-bytes";
const NODE_2_KEY: &[u8] = b"mesh-node-two-auth-key-at-least-32-bytes";

async fn free_addr() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve loopback port");
    let address = listener.local_addr().expect("read reserved address");
    drop(listener);
    address
}

async fn free_addrs() -> (SocketAddr, SocketAddr) {
    let first = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve first loopback port");
    let second = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve second loopback port");
    let first_address = first.local_addr().expect("read first address");
    let second_address = second.local_addr().expect("read second address");
    drop((first, second));
    (first_address, second_address)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Drain one frame and acknowledge it.
///
/// The transport only reports a peer write as delivered once the receiver has
/// returned an authenticated application ACK, so the receiver has to
/// acknowledge the frame for the sender's broadcast report to settle.
async fn wait_for_one_acked(handle: &MeshHandle) -> Vec<MeshMessage> {
    timeout(Duration::from_secs(2), async {
        loop {
            let received = handle.drain_with_ack().await;
            if !received.is_empty() {
                let messages: Vec<MeshMessage> =
                    received.iter().map(|frame| frame.message.clone()).collect();
                for frame in received {
                    frame.acknowledge(true);
                }
                break messages;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("receiver should queue a valid message before timeout")
}

#[tokio::test]
async fn authenticated_block_crosses_real_tcp_transport() {
    let (receiver_addr, sender_addr) = free_addrs().await;
    let receiver = MeshHandle::bind(
        2,
        Arc::new(AworsetBlocklist::new(2)),
        receiver_addr,
        vec![MeshPeer::new(1, sender_addr, NODE_1_KEY.to_vec())],
        NODE_2_KEY.to_vec(),
    )
    .await
    .expect("bind receiver");

    let sender = MeshHandle::bind(
        1,
        Arc::new(AworsetBlocklist::new(1)),
        sender_addr,
        vec![MeshPeer::new(2, receiver_addr, NODE_2_KEY.to_vec())],
        NODE_1_KEY.to_vec(),
    )
    .await
    .expect("bind sender");

    let created_at_ms = now_ms();
    let delta = ClusterBlockDelta {
        ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 91)),
        dot: ClusterDot {
            node_id: 1,
            incarnation: uuid::Uuid::new_v4(),
            counter: 1,
        },
        created_at_ms,
        expires_at_ms: created_at_ms + 60_000,
        tier: 2,
    };

    // The sender waits for the receiver's authenticated application ACK, so
    // the broadcast runs concurrently with the receiver draining and
    // acknowledging the frame.
    let broadcast = {
        let sender = sender.clone();
        let delta = delta.clone();
        tokio::spawn(async move { sender.broadcast(MeshMessage::Block(delta)).await })
    };

    let received = wait_for_one_acked(&receiver).await;

    let report = broadcast.await.expect("broadcast task must not panic");
    assert_eq!(report.written_count(), 1);
    assert!(report.all_written());

    assert_eq!(received.len(), 1);
    match &received[0] {
        MeshMessage::Block(received_delta) => assert_eq!(received_delta, &delta),
        other => panic!("expected block delta, got {other:?}"),
    }

    sender.shutdown();
    receiver.shutdown();
}

#[tokio::test]
async fn invalid_hmac_frame_never_enters_receive_queue() {
    let receiver_addr = free_addr().await;
    let receiver = MeshHandle::bind(
        2,
        Arc::new(AworsetBlocklist::new(2)),
        receiver_addr,
        vec![MeshPeer::new(1, free_addr().await, NODE_1_KEY.to_vec())],
        NODE_2_KEY.to_vec(),
    )
    .await
    .expect("bind receiver");

    let created_at_ms = now_ms();
    let forged = serde_json::json!({
        "ts_ms": created_at_ms,
        "node_id": 1,
        "body": {
            "Block": {
                "ip": "192.0.2.92",
                "dot": { "node_id": 1, "counter": 1 },
                "created_at_ms": created_at_ms,
                "expires_at_ms": created_at_ms + 60_000,
                "tier": 1
            }
        },
        "mac": "00"
    });

    let mut stream = TcpStream::connect(receiver_addr)
        .await
        .expect("connect forged peer");
    stream
        .write_all(format!("{forged}\n").as_bytes())
        .await
        .expect("write forged frame");
    drop(stream);

    sleep(Duration::from_millis(100)).await;
    assert!(
        receiver.drain().await.is_empty(),
        "unauthenticated frame must not enter the receive queue"
    );
    receiver.shutdown();
}
