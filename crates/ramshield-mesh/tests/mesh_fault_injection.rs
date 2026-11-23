//! Multi-process fault-injection proof for the mesh transport.
//!
//! The parent process sends durable-ack WAL batches to a child mesh node,
//! kills the child between applying a batch and acknowledging it, restarts the
//! child from its persisted CRDT snapshot, and proves the retry converges.
//! This covers transport cursor discipline, receiver-confirmed recovery, and
//! cross-process duplicate/reorder convergence; the enforcement WAL cursor is
//! exercised by the enforcement mesh tests.

use ramshield_mesh::{
    aworset::{
        AworsetBlocklist, AworsetSnapshot, ClusterBlockDelta, ClusterDot, ClusterUnblockDelta,
    },
    transport::{MeshHandle, MeshMessage, MeshMutationKind, MeshPeer, ReplicatedMeshMutation},
};
use std::{
    env, fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};
use uuid::Uuid;

const SENDER_KEY: &[u8] = b"fault-sender-auth-key-at-least-32-bytes";
const RECEIVER_KEY: &[u8] = b"fault-receiver-auth-key-at-least-32-bytes";
const SLOW_LSN: u64 = 20;
const CHILD_ENV: &str = "RAMSHIELD_MESH_FAULT_CHILD";

#[derive(Clone)]
struct ChildConfig {
    listen: SocketAddr,
    sender: SocketAddr,
    snapshot: PathBuf,
    slow_lsn: u64,
}

impl ChildConfig {
    fn from_env() -> Option<Self> {
        env::var_os(CHILD_ENV)?;
        Some(Self {
            listen: env::var("RAMSHIELD_MESH_FAULT_LISTEN").ok()?.parse().ok()?,
            sender: env::var("RAMSHIELD_MESH_FAULT_SENDER").ok()?.parse().ok()?,
            snapshot: PathBuf::from(env::var("RAMSHIELD_MESH_FAULT_SNAPSHOT").ok()?),
            slow_lsn: env::var("RAMSHIELD_MESH_FAULT_SLOW_LSN")
                .ok()?
                .parse()
                .ok()?,
        })
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

async fn free_addr() -> SocketAddr {
    TcpListener::bind("127.0.0.1:0")
        .await
        .expect("free loopback port")
        .local_addr()
        .expect("local address")
}

fn temp_snapshot_path() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    env::temp_dir().join(format!("ramshield-mesh-fault-{stamp}.json"))
}

fn spawn_child(config: &ChildConfig) -> Child {
    Command::new(env::current_exe().expect("current test binary"))
        .args(["--exact", "mesh_fault_child", "--nocapture"])
        .env(CHILD_ENV, "1")
        .env("RAMSHIELD_MESH_FAULT_LISTEN", config.listen.to_string())
        .env("RAMSHIELD_MESH_FAULT_SENDER", config.sender.to_string())
        .env("RAMSHIELD_MESH_FAULT_SNAPSHOT", &config.snapshot)
        .env("RAMSHIELD_MESH_FAULT_SLOW_LSN", config.slow_lsn.to_string())
        .spawn()
        .expect("spawn mesh fault child")
}

async fn wait_for_listener(address: SocketAddr) {
    timeout(Duration::from_secs(5), async {
        loop {
            if TcpStream::connect(address).await.is_ok() {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("child mesh listener should start");
}

fn wal_batch(lsn: u64, ip: IpAddr, counter: u64) -> MeshMessage {
    wal_batch_with_incarnation(lsn, ip, counter, Uuid::new_v4())
}

fn wal_batch_with_incarnation(
    lsn: u64,
    ip: IpAddr,
    counter: u64,
    origin_incarnation: Uuid,
) -> MeshMessage {
    let created = now_ms();
    MeshMessage::WalBatch {
        source_node_id: 1,
        start_lsn: lsn,
        end_lsn: lsn,
        mutations: vec![ReplicatedMeshMutation {
            source_lsn: lsn,
            origin_node_id: 1,
            origin_incarnation: origin_incarnation.to_string(),
            origin_counter: counter,
            ip,
            action: MeshMutationKind::Block,
            created_at_ms: created,
            expires_at_ms: Some(created.saturating_add(60_000)),
            tier: 2,
        }],
    }
}

fn apply_mutation(state: &AworsetBlocklist, mutation: &ReplicatedMeshMutation) {
    let dot = ClusterDot {
        node_id: mutation.origin_node_id,
        incarnation: Uuid::parse_str(&mutation.origin_incarnation).expect("valid origin id"),
        counter: mutation.origin_counter,
    };
    match mutation.action {
        MeshMutationKind::Block => {
            state.merge_delta(&ClusterBlockDelta {
                ip: mutation.ip,
                dot,
                created_at_ms: mutation.created_at_ms,
                expires_at_ms: mutation.expires_at_ms.unwrap_or(u64::MAX),
                tier: mutation.tier,
            });
        }
        MeshMutationKind::Unblock => {
            state.merge_unblock_delta(&ClusterUnblockDelta {
                ip: mutation.ip,
                dot,
            });
        }
    }
}

fn persist_snapshot(path: &Path, state: &AworsetBlocklist) {
    let bytes = serde_json::to_vec(&state.export_snapshot()).expect("serialize snapshot");
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes).expect("write snapshot temp file");
    fs::rename(&temporary, path).expect("publish snapshot");
}

fn snapshot_has_ip(path: &Path, ip: IpAddr) -> bool {
    snapshot_block_count(path, ip) > 0
}

fn snapshot_block_count(path: &Path, ip: IpAddr) -> usize {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<AworsetSnapshot>(&bytes).ok())
        .map(|snapshot| {
            snapshot
                .blocks
                .iter()
                .filter(|block| block.ip == ip)
                .count()
        })
        .unwrap_or(0)
}

async fn wait_for_snapshot(path: &Path, ip: IpAddr, expected: bool) {
    timeout(Duration::from_secs(3), async {
        while snapshot_has_ip(path, ip) != expected {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("snapshot should reach expected state before timeout");
}

async fn run_child(config: ChildConfig) -> ! {
    let state = Arc::new(AworsetBlocklist::new(2));
    if let Ok(bytes) = fs::read(&config.snapshot)
        && let Ok(snapshot) = serde_json::from_slice::<AworsetSnapshot>(&bytes)
        && snapshot.validate().is_ok()
    {
        state.restore_snapshot(&snapshot);
    }
    let handle = MeshHandle::bind(
        2,
        state.clone(),
        config.listen,
        vec![MeshPeer::new(1, config.sender, SENDER_KEY.to_vec())],
        RECEIVER_KEY.to_vec(),
    )
    .await
    .expect("child mesh listener");

    loop {
        for frame in handle.drain_with_ack().await {
            let mut end_lsn = None;
            let mut changed = false;
            match &frame.message {
                MeshMessage::WalBatch {
                    end_lsn: lsn,
                    mutations,
                    ..
                } => {
                    end_lsn = Some(*lsn);
                    for mutation in mutations {
                        apply_mutation(&state, mutation);
                    }
                    changed = true;
                }
                MeshMessage::Sync { blocks, unblocks } => {
                    for delta in blocks {
                        state.merge_delta(delta);
                    }
                    for delta in unblocks {
                        state.merge_unblock_delta(delta);
                    }
                    changed = true;
                }
                _ => {}
            }
            if end_lsn == Some(config.slow_lsn) {
                // ponytail: deterministic kill window; no production delay.
                sleep(Duration::from_secs(1)).await;
            }
            if changed {
                persist_snapshot(&config.snapshot, &state);
            }
            frame.acknowledge_with_lsn(true, end_lsn);
        }
        sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn mesh_fault_child() {
    if let Some(config) = ChildConfig::from_env() {
        run_child(config).await;
    }
}

#[tokio::test]
async fn crash_before_ack_restarts_and_converges_without_cursor_regression() {
    if ChildConfig::from_env().is_some() {
        return;
    }

    let sender_address = free_addr().await;
    let receiver_address = free_addr().await;
    let snapshot_path = temp_snapshot_path();
    let _ = fs::remove_file(&snapshot_path);
    let config = ChildConfig {
        listen: receiver_address,
        sender: sender_address,
        snapshot: snapshot_path.clone(),
        slow_lsn: SLOW_LSN,
    };

    let sender_state = Arc::new(AworsetBlocklist::new(1));
    let sender = MeshHandle::bind(
        1,
        sender_state,
        sender_address,
        vec![MeshPeer::new(2, receiver_address, RECEIVER_KEY.to_vec())],
        SENDER_KEY.to_vec(),
    )
    .await
    .expect("sender mesh listener");

    let mut child = spawn_child(&config);
    wait_for_listener(receiver_address).await;

    let first_ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 80));
    let second_ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 81));
    let first_ack = timeout(
        Duration::from_secs(5),
        sender.send_wal_batch(receiver_address, wal_batch(10, first_ip, 1)),
    )
    .await
    .expect("first batch ACK")
    .expect("first batch send")
    .expect("first batch durable LSN");
    assert_eq!(first_ack, 10);
    wait_for_snapshot(&snapshot_path, first_ip, true).await;

    let mut cursor = first_ack;
    let in_flight = tokio::spawn({
        let sender = sender.clone();
        async move {
            sender
                .send_wal_batch(receiver_address, wal_batch(SLOW_LSN, second_ip, 2))
                .await
        }
    });
    sleep(Duration::from_millis(100)).await;
    child.kill().expect("kill child mid-burst");
    child.wait().expect("reap killed child");
    let interrupted = timeout(Duration::from_secs(5), in_flight)
        .await
        .expect("in-flight send should settle after child death");
    assert!(
        matches!(interrupted, Ok(Ok(None)) | Ok(Err(_)) | Err(_)),
        "crash must not yield a durable LSN: {interrupted:?}"
    );
    assert_eq!(cursor, 10, "cursor must not advance before ACK");
    assert!(
        !snapshot_has_ip(&snapshot_path, second_ip),
        "unacknowledged mutation must not be durable"
    );

    let mut child = spawn_child(&config);
    wait_for_listener(receiver_address).await;
    let retry_ack = timeout(
        Duration::from_secs(5),
        sender.send_wal_batch(receiver_address, wal_batch(SLOW_LSN, second_ip, 2)),
    )
    .await
    .expect("retry ACK")
    .expect("retry send")
    .expect("retry durable LSN");
    assert_eq!(retry_ack, SLOW_LSN);
    assert!(retry_ack > cursor, "cursor must advance, not regress");
    cursor = retry_ack;
    wait_for_snapshot(&snapshot_path, first_ip, true).await;
    wait_for_snapshot(&snapshot_path, second_ip, true).await;
    assert_eq!(cursor, 20);

    child.kill().expect("kill restarted child");
    child.wait().expect("reap restarted child");
    sender.shutdown();
    let _ = fs::remove_file(&snapshot_path);
    let _ = fs::remove_file(snapshot_path.with_extension("tmp"));
}

#[tokio::test]
async fn recovery_gap_requires_receiver_proof_before_cursor_release() {
    if ChildConfig::from_env().is_some() {
        return;
    }

    let sender_address = free_addr().await;
    let receiver_address = free_addr().await;
    let snapshot_path = temp_snapshot_path();
    let _ = fs::remove_file(&snapshot_path);
    let config = ChildConfig {
        listen: receiver_address,
        sender: sender_address,
        snapshot: snapshot_path.clone(),
        slow_lsn: 0,
    };

    let sender_state = Arc::new(AworsetBlocklist::new(1));
    let ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 90));
    sender_state.merge_delta(&ClusterBlockDelta {
        ip,
        dot: ClusterDot {
            node_id: 1,
            incarnation: Uuid::new_v4(),
            counter: 1,
        },
        created_at_ms: now_ms(),
        expires_at_ms: u64::MAX,
        tier: 2,
    });
    let sender = MeshHandle::bind(
        1,
        sender_state.clone(),
        sender_address,
        vec![MeshPeer::new(2, receiver_address, RECEIVER_KEY.to_vec())],
        SENDER_KEY.to_vec(),
    )
    .await
    .expect("sender mesh listener");

    let mut child = spawn_child(&config);
    wait_for_listener(receiver_address).await;

    sender
        .request_full_sync(receiver_address, 42, sender_state.export_snapshot())
        .await;
    let (generation, boundary_lsn) = timeout(Duration::from_secs(5), async {
        loop {
            if let Some(proof) = sender.completed_recovery(receiver_address).await {
                break proof;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("receiver recovery proof");
    assert_eq!(boundary_lsn, 42, "proof must bind the requested boundary");
    wait_for_snapshot(&snapshot_path, ip, true).await;

    sender
        .acknowledge_recovery_cursor(receiver_address, generation, boundary_lsn.saturating_add(1))
        .await;
    assert!(
        sender.recovery_completion_pending(receiver_address).await,
        "wrong boundary must not release the cursor gate"
    );

    sender
        .acknowledge_recovery_cursor(receiver_address, generation, boundary_lsn)
        .await;
    assert!(
        !sender.recovery_completion_pending(receiver_address).await,
        "acknowledged recovery proof must release the cursor gate"
    );

    child.kill().expect("kill recovery child");
    child.wait().expect("reap recovery child");
    sender.shutdown();
    let _ = fs::remove_file(&snapshot_path);
    let _ = fs::remove_file(snapshot_path.with_extension("tmp"));
}

#[tokio::test]
async fn duplicate_and_reordered_batches_converge_without_new_ack_lsn() {
    if ChildConfig::from_env().is_some() {
        return;
    }

    let sender_address = free_addr().await;
    let receiver_address = free_addr().await;
    let snapshot_path = temp_snapshot_path();
    let _ = fs::remove_file(&snapshot_path);
    let config = ChildConfig {
        listen: receiver_address,
        sender: sender_address,
        snapshot: snapshot_path.clone(),
        slow_lsn: 0,
    };

    let sender_state = Arc::new(AworsetBlocklist::new(1));
    let sender = MeshHandle::bind(
        1,
        sender_state,
        sender_address,
        vec![MeshPeer::new(2, receiver_address, RECEIVER_KEY.to_vec())],
        SENDER_KEY.to_vec(),
    )
    .await
    .expect("sender mesh listener");

    let mut child = spawn_child(&config);
    wait_for_listener(receiver_address).await;

    let ip1 = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 100));
    let ip2 = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 101));
    let ip3 = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 102));
    let incarnation1 = Uuid::new_v4();
    let incarnation2 = Uuid::new_v4();
    let incarnation3 = Uuid::new_v4();

    let first = wal_batch_with_incarnation(10, ip1, 1, incarnation1);
    let first_duplicate = wal_batch_with_incarnation(10, ip1, 1, incarnation1);
    assert_eq!(
        sender
            .send_wal_batch(receiver_address, first)
            .await
            .expect("send first")
            .expect("ack first"),
        10
    );
    assert_eq!(
        sender
            .send_wal_batch(receiver_address, first_duplicate)
            .await
            .expect("send duplicate")
            .expect("ack duplicate"),
        10,
        "duplicate batch must not be acked as a new LSN"
    );
    wait_for_snapshot(&snapshot_path, ip1, true).await;
    assert_eq!(
        snapshot_block_count(&snapshot_path, ip1),
        1,
        "duplicate batch must converge to one block"
    );

    let higher = wal_batch_with_incarnation(20, ip2, 2, incarnation2);
    let lower = wal_batch_with_incarnation(15, ip3, 3, incarnation3);
    assert_eq!(
        sender
            .send_wal_batch(receiver_address, higher)
            .await
            .expect("send higher")
            .expect("ack higher"),
        20
    );
    assert_eq!(
        sender
            .send_wal_batch(receiver_address, lower)
            .await
            .expect("send lower")
            .expect("ack lower"),
        15,
        "reordered lower LSN must be acked with its own boundary"
    );
    wait_for_snapshot(&snapshot_path, ip2, true).await;
    wait_for_snapshot(&snapshot_path, ip3, true).await;

    let duplicate_higher = wal_batch_with_incarnation(20, ip2, 2, incarnation2);
    assert_eq!(
        sender
            .send_wal_batch(receiver_address, duplicate_higher)
            .await
            .expect("send duplicate higher")
            .expect("ack duplicate higher"),
        20
    );
    assert_eq!(snapshot_block_count(&snapshot_path, ip2), 1);
    assert_eq!(snapshot_block_count(&snapshot_path, ip3), 1);

    child.kill().expect("kill duplicate child");
    child.wait().expect("reap duplicate child");
    sender.shutdown();
    let _ = fs::remove_file(&snapshot_path);
    let _ = fs::remove_file(snapshot_path.with_extension("tmp"));
}
