use crate::aworset::{AworsetBlocklist, ClusterBlockDelta, ClusterUnblockDelta};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    future::Future,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Mutex, Semaphore, watch},
    task::JoinSet,
};
use tracing::{debug, warn};

type HmacSha256 = Hmac<Sha256>;
const MAX_FRAME: usize = 64 * 1024;
const MAX_CLOCK_SKEW_MS: u64 = 30_000;
const ANTI_ENTROPY_MS: u64 = 2_000;
const MAX_SYNC_ENTRIES: usize = 4096;
const MAX_SYNC_FRAME_ENTRIES: usize = 128;
const MAX_INCOMING_QUEUE: usize = 2048;
const MAX_PEER_READERS: usize = 256;
const MAX_CONFIGURED_PEERS: usize = 256;
const MAX_FRAME_READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PEER_WRITERS: usize = 64;
const MAX_PEER_WRITE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshMessage {
    Block(ClusterBlockDelta),
    Unblock(ClusterUnblockDelta),
    Sync {
        blocks: Vec<ClusterBlockDelta>,
        unblocks: Vec<ClusterUnblockDelta>,
    },
}

/// Result of attempting to write a mesh message to one configured peer.
/// `Written` confirms the frame was written to the peer's TCP socket; it does
/// not confirm that the peer authenticated, accepted, or applied the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerSendOutcome {
    Written,
    TimedOut,
    Cancelled,
    Failed { message: String },
}

/// Per-peer outcome from a mesh broadcast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSendResult {
    pub peer: SocketAddr,
    pub outcome: PeerSendOutcome,
}

/// A broadcast is successful only for peers whose outcome is `Written`.
/// Application success is reported separately by the receiving enforcement service.
#[must_use = "inspect per-peer mesh delivery outcomes"]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BroadcastReport {
    pub peers: Vec<PeerSendResult>,
    /// Tasks that terminated unexpectedly before returning a peer outcome.
    pub task_failures: usize,
}

impl BroadcastReport {
    pub fn written_count(&self) -> usize {
        self.peers
            .iter()
            .filter(|p| p.outcome == PeerSendOutcome::Written)
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.peers.len().saturating_sub(self.written_count()) + self.task_failures
    }

    pub fn all_written(&self) -> bool {
        self.task_failures == 0
            && self
                .peers
                .iter()
                .all(|p| p.outcome == PeerSendOutcome::Written)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Envelope {
    ts_ms: u64,
    node_id: u32,
    body: MeshMessage,
    mac: String,
}
/// A configured peer identity. Each configured peer has a distinct HMAC key.
#[derive(Clone)]
pub struct MeshPeer {
    pub node_id: u32,
    pub address: SocketAddr,
    pub auth_key: Vec<u8>,
}

impl MeshPeer {
    pub fn new(node_id: u32, address: SocketAddr, auth_key: Vec<u8>) -> Self {
        Self {
            node_id,
            address,
            auth_key,
        }
    }
}

#[derive(Clone)]
pub struct MeshHandle {
    node_id: u32,
    blocklist: Arc<AworsetBlocklist>,
    peers: Arc<Vec<MeshPeer>>,
    /// This node's own signing key.
    auth_key: Arc<Vec<u8>>,
    /// Keys authorized to authenticate incoming node IDs.
    trusted_keys: Arc<HashMap<u32, Vec<u8>>>,
    incoming: Arc<Mutex<VecDeque<MeshMessage>>>,
    readers: Arc<Semaphore>,
    writers: Arc<Semaphore>,
    shutdown_tx: watch::Sender<bool>,
}

impl MeshHandle {
    pub async fn bind(
        node_id: u32,
        blocklist: Arc<AworsetBlocklist>,
        listen: SocketAddr,
        peers: Vec<MeshPeer>,
        auth_key: Vec<u8>,
    ) -> std::io::Result<Self> {
        validate_bind_args(node_id, &peers, &auth_key)?;
        let listener = TcpListener::bind(listen).await?;
        let trusted_keys = peers
            .iter()
            .map(|peer| (peer.node_id, peer.auth_key.clone()))
            .collect::<HashMap<_, _>>();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = Self {
            node_id,
            blocklist,
            peers: Arc::new(peers),
            auth_key: Arc::new(auth_key),
            trusted_keys: Arc::new(trusted_keys),
            incoming: Arc::new(Mutex::new(VecDeque::new())),
            readers: Arc::new(Semaphore::new(MAX_PEER_READERS)),
            writers: Arc::new(Semaphore::new(MAX_PEER_WRITERS)),
            shutdown_tx,
        };
        let accept_handle = handle.clone();
        let mut listener_shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    changed = listener_shutdown.changed() => {
                        if changed.is_err() || *listener_shutdown.borrow_and_update() {
                            break;
                        }
                    }
                    accepted = listener.accept() => match accepted {
                        Ok((stream, _)) => {
                            let h = accept_handle.clone();
                            let Ok(permit) = h.readers.clone().try_acquire_owned() else { continue; };
                            tokio::spawn(async move {
                                h.read_stream(stream).await;
                                drop(permit);
                            });
                        }
                        Err(error) => {
                            warn!(error = %error, "mesh listener stopped");
                            break;
                        }
                    }
                }
            }
        });
        let sync_handle = handle.clone();
        let mut sync_shutdown = shutdown_rx;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(ANTI_ENTROPY_MS));
            loop {
                tokio::select! {
                    _ = tick.tick() => sync_handle.broadcast_sync().await,
                    changed = sync_shutdown.changed() => {
                        if changed.is_err() || *sync_shutdown.borrow_and_update() {
                            break;
                        }
                    }
                }
            }
        });
        Ok(handle)
    }
    /// Signal all mesh-owned background tasks to stop. Reader tasks also select
    /// on this signal; outbound sends are bounded by their deadline.
    pub fn shutdown(&self) {
        self.shutdown_tx.send_replace(true);
    }

    /// Send to configured peers and return one explicit outcome per peer.
    ///
    /// A `Written` result only confirms that the frame was written to the TCP
    /// socket. It is not an acknowledgement of remote authentication or application.
    /// Concurrency is bounded before each task is spawned.
    pub async fn broadcast(&self, body: MeshMessage) -> BroadcastReport {
        let mut tasks = JoinSet::new();
        let mut report = BroadcastReport::default();

        for peer in self.peers.iter() {
            let peer = peer.address;
            let permit = match self.writers.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(error) => {
                    report.peers.push(PeerSendResult {
                        peer,
                        outcome: PeerSendOutcome::Failed {
                            message: format!("outbound semaphore unavailable: {error}"),
                        },
                    });
                    continue;
                }
            };
            let handle = self.clone();
            let message = body.clone();
            tasks.spawn(async move {
                let _permit = permit;
                let mut shutdown = handle.shutdown_tx.subscribe();
                if *shutdown.borrow() {
                    return (peer, PeerSendOutcome::Cancelled);
                }
                let outcome = tokio::select! {
                    result = with_timeout(MAX_PEER_WRITE_TIMEOUT, handle.send_to(peer, message)) => {
                        match result {
                            Ok(()) => PeerSendOutcome::Written,
                            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
                                PeerSendOutcome::TimedOut
                            }
                            Err(error) => PeerSendOutcome::Failed {
                                message: error.to_string(),
                            },
                        }
                    }
                    _ = shutdown.changed() => PeerSendOutcome::Cancelled,
                };
                (peer, outcome)
            });
        }

        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok((peer, outcome)) => {
                    match &outcome {
                        PeerSendOutcome::Written => {}
                        PeerSendOutcome::TimedOut => {
                            warn!(peer = %peer, "mesh outbound send timed out");
                        }
                        PeerSendOutcome::Cancelled => {
                            debug!(peer = %peer, "mesh outbound send cancelled");
                        }
                        PeerSendOutcome::Failed { message } => {
                            warn!(peer = %peer, error = %message, "mesh outbound send failed");
                        }
                    }
                    report.peers.push(PeerSendResult { peer, outcome });
                }
                Err(error) => {
                    report.task_failures += 1;
                    warn!(error = %error, "mesh outbound task failed before reporting an outcome");
                }
            }
        }

        report
    }
    pub async fn drain(&self) -> Vec<MeshMessage> {
        let mut q = self.incoming.lock().await;
        q.drain(..).collect()
    }
    async fn broadcast_sync(&self) {
        let (blocks, unblocks) = self.blocklist.snapshot(MAX_SYNC_ENTRIES);
        for message in chunk_sync(&blocks, &unblocks) {
            let report = self.broadcast(message).await;
            if !report.all_written() {
                warn!(
                    written = report.written_count(),
                    failed = report.failed_count(),
                    "mesh anti-entropy sync was not delivered to every configured peer"
                );
            }
        }
    }
    async fn send_to(&self, peer: SocketAddr, body: MeshMessage) -> std::io::Result<()> {
        let ts_ms = now_ms();
        let payload =
            serde_json::to_vec(&(ts_ms, self.node_id, &body)).map_err(std::io::Error::other)?;
        let mac = sign(&self.auth_key, &payload)?;
        let frame = serde_json::to_vec(&Envelope {
            ts_ms,
            node_id: self.node_id,
            body,
            mac,
        })
        .map_err(std::io::Error::other)?;
        if frame.len() > MAX_FRAME {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "mesh frame too large",
            ));
        }
        let mut stream = TcpStream::connect(peer).await?;
        stream.write_all(&frame).await?;
        stream.write_all(b"\n").await?;
        Ok(())
    }
    async fn read_stream(&self, mut stream: TcpStream) {
        let mut shutdown = self.shutdown_tx.subscribe();
        if *shutdown.borrow() {
            return;
        }
        let frame_result = tokio::select! {
            result = read_bounded_frame_with_timeout(&mut stream, MAX_FRAME_READ_TIMEOUT) => result,
            _ = shutdown.changed() => return,
        };
        let mut line = match frame_result {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(_) => return,
        };
        while line.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
            line.pop();
        }
        let env: Envelope = match serde_json::from_slice(&line) {
            Ok(v) => v,
            Err(_) => return,
        };
        let payload = match serde_json::to_vec(&(env.ts_ms, env.node_id, &env.body)) {
            Ok(v) => v,
            Err(_) => return,
        };
        let Some(peer_key) = self.trusted_keys.get(&env.node_id) else {
            warn!(
                node_id = env.node_id,
                "mesh frame rejected: node ID is not configured"
            );
            return;
        };
        if now_ms().abs_diff(env.ts_ms) > MAX_CLOCK_SKEW_MS || !verify(peer_key, &payload, &env.mac)
        {
            warn!(
                node_id = env.node_id,
                "mesh frame rejected: authentication or timestamp invalid"
            );
            return;
        }
        if env.node_id == self.node_id {
            return;
        }
        if !valid_message(&env.body, now_ms()) {
            warn!(
                node_id = env.node_id,
                "mesh frame rejected: message fields outside accepted bounds"
            );
            return;
        }
        let mut q = self.incoming.lock().await;
        if q.len() >= MAX_INCOMING_QUEUE {
            // Prefer the newest authenticated state; anti-entropy will recover
            // an older delta if it was displaced. Never allow mesh traffic to
            // consume unbounded memory under a peer flood.
            q.pop_front();
        }
        q.push_back(env.body);
    }
}
/// Validate the public library boundary as well as the higher-level config.
fn validate_bind_args(node_id: u32, peers: &[MeshPeer], auth_key: &[u8]) -> std::io::Result<()> {
    let invalid = |message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message);
    if node_id == 0 {
        return Err(invalid("mesh node_id must be non-zero"));
    }
    if auth_key.len() < 16 {
        return Err(invalid(
            "local mesh authentication key must be at least 16 bytes",
        ));
    }
    if peers.len() > MAX_CONFIGURED_PEERS {
        return Err(invalid("mesh peer list exceeds 256 peers"));
    }
    let mut seen_ids = HashSet::with_capacity(peers.len());
    let mut seen_addresses = HashSet::with_capacity(peers.len());
    let mut seen_keys = HashSet::with_capacity(peers.len() + 1);
    seen_keys.insert(auth_key.to_vec());
    for peer in peers {
        if peer.node_id == 0 || peer.node_id == node_id {
            return Err(invalid("mesh peer node IDs must be non-zero and not self"));
        }
        if peer.auth_key.len() < 16 {
            return Err(invalid("each mesh peer key must be at least 16 bytes"));
        }
        if !seen_keys.insert(peer.auth_key.clone()) {
            return Err(invalid(
                "mesh local and peer authentication keys must be unique",
            ));
        }
        if peer.address.port() == 0 || peer.address.ip().is_unspecified() {
            return Err(invalid(
                "mesh peers must have a concrete IP address and non-zero port",
            ));
        }
        if !seen_ids.insert(peer.node_id) {
            return Err(invalid("mesh peer node IDs must be unique"));
        }
        if !seen_addresses.insert(peer.address) {
            return Err(invalid("mesh peer addresses must be unique"));
        }
    }
    Ok(())
}

/// Reject authenticated-but-invalid CRDT fields before they can poison the HLC
/// or amplify the bounded incoming queue. Zero creation time is accepted for
/// compatibility with peers that predate the created_at_ms field.
fn valid_message(message: &MeshMessage, now_ms: u64) -> bool {
    let latest_creation = now_ms.saturating_add(MAX_CLOCK_SKEW_MS);
    let valid_block = |delta: &ClusterBlockDelta| {
        delta.dot.node_id != 0
            && !delta.dot.incarnation.is_nil()
            && (delta.created_at_ms == 0 || delta.created_at_ms <= latest_creation)
            && (delta.expires_at_ms == u64::MAX || delta.expires_at_ms >= delta.created_at_ms)
    };

    match message {
        MeshMessage::Block(delta) => valid_block(delta),
        MeshMessage::Unblock(delta) => delta.dot.node_id != 0 && !delta.dot.incarnation.is_nil(),
        MeshMessage::Sync { blocks, unblocks } => {
            blocks.len().saturating_add(unblocks.len()) <= MAX_SYNC_FRAME_ENTRIES
                && blocks.iter().all(valid_block)
                && unblocks
                    .iter()
                    .all(|delta| delta.dot.node_id != 0 && !delta.dot.incarnation.is_nil())
        }
    }
}

/// Split anti-entropy state into small independently authenticated frames.
fn chunk_sync(blocks: &[ClusterBlockDelta], unblocks: &[ClusterUnblockDelta]) -> Vec<MeshMessage> {
    let block_chunks = blocks.len().div_ceil(MAX_SYNC_FRAME_ENTRIES);
    let unblock_chunks = unblocks.len().div_ceil(MAX_SYNC_FRAME_ENTRIES);
    let mut messages = Vec::with_capacity(block_chunks + unblock_chunks);

    messages.extend(
        blocks
            .chunks(MAX_SYNC_FRAME_ENTRIES)
            .map(|chunk| MeshMessage::Sync {
                blocks: chunk.to_vec(),
                unblocks: Vec::new(),
            }),
    );
    messages.extend(
        unblocks
            .chunks(MAX_SYNC_FRAME_ENTRIES)
            .map(|chunk| MeshMessage::Sync {
                blocks: Vec::new(),
                unblocks: chunk.to_vec(),
            }),
    );
    messages
}

async fn with_timeout<F>(timeout: Duration, future: F) -> std::io::Result<()>
where
    F: Future<Output = std::io::Result<()>>,
{
    tokio::time::timeout(timeout, future).await.map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::TimedOut, "mesh outbound send timed out")
    })?
}

fn push_frame_bytes(line: &mut Vec<u8>, bytes: &[u8]) -> std::io::Result<bool> {
    if let Some(end) = bytes.iter().position(|byte| *byte == b'\n') {
        if line.len() + end > MAX_FRAME {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "mesh frame too large",
            ));
        }
        line.extend_from_slice(&bytes[..=end]);
        return Ok(true);
    }
    if line.len() + bytes.len() > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "mesh frame too large",
        ));
    }
    line.extend_from_slice(bytes);
    Ok(false)
}

async fn read_bounded_frame_with_timeout<R: AsyncRead + Unpin>(
    reader: &mut R,
    timeout: Duration,
) -> std::io::Result<Option<Vec<u8>>> {
    tokio::time::timeout(timeout, read_bounded_frame(reader))
        .await
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "mesh frame read timed out")
        })?
}

async fn read_bounded_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            if line.is_empty() {
                return Ok(None);
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "incomplete mesh frame",
            ));
        }
        if push_frame_bytes(&mut line, &chunk[..read])? {
            return Ok(Some(line));
        }
    }
}

fn sign(key: &[u8], payload: &[u8]) -> std::io::Result<String> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid mesh key"))?;
    mac.update(payload);
    Ok(hex::encode(mac.finalize().into_bytes()))
}
fn verify(key: &[u8], payload: &[u8], signature: &str) -> bool {
    let bytes = match hex::decode(signature) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut mac = match HmacSha256::new_from_slice(key) {
        Ok(v) => v,
        Err(_) => return false,
    };
    mac.update(payload);
    mac.verify_slice(&bytes).is_ok()
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod frame_tests {
    use super::{
        Envelope, MAX_CONFIGURED_PEERS, MAX_FRAME, MAX_SYNC_FRAME_ENTRIES, MeshHandle, MeshMessage,
        MeshPeer, PeerSendOutcome, chunk_sync, now_ms, push_frame_bytes, read_bounded_frame,
        read_bounded_frame_with_timeout, sign, valid_message, validate_bind_args, with_timeout,
    };
    use crate::aworset::{AworsetBlocklist, ClusterBlockDelta, ClusterDot, ClusterUnblockDelta};
    use std::time::Duration;
    use std::{
        net::{IpAddr, Ipv6Addr, SocketAddr},
        sync::Arc,
    };
    use tokio::io::AsyncWriteExt;
    use tokio::net::{TcpListener, TcpStream};

    #[test]
    fn delimiter_free_oversized_frame_is_rejected_while_reading() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let (mut writer, mut reader) = tokio::io::duplex(MAX_FRAME + 2);
            writer
                .write_all(&vec![b'x'; MAX_FRAME + 1])
                .await
                .expect("write oversized delimiter-free frame");
            let error = read_bounded_frame(&mut reader)
                .await
                .expect_err("oversized frame must be rejected before a delimiter arrives");
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        });
    }

    #[test]
    fn incomplete_peer_frame_times_out() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let (_writer, mut reader) = tokio::io::duplex(8);
            let error = read_bounded_frame_with_timeout(&mut reader, Duration::from_millis(10))
                .await
                .unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        });
    }

    #[test]
    fn bind_rejects_invalid_node_key_and_peer_configuration() {
        let valid_key = [7u8; 32];
        let error = validate_bind_args(0, &[], &valid_key).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

        let error = validate_bind_args(1, &[], &[7u8; 15]).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

        let peers: Vec<MeshPeer> = (0..=MAX_CONFIGURED_PEERS)
            .map(|_| {
                MeshPeer::new(
                    2,
                    "127.0.0.1:1234".parse().expect("peer address"),
                    vec![8; 32],
                )
            })
            .collect();
        let error = validate_bind_args(1, &peers, &valid_key).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

        let invalid_peer = [MeshPeer::new(
            2,
            "0.0.0.0:1234".parse().expect("unspecified peer"),
            vec![8; 32],
        )];
        let error = validate_bind_args(1, &invalid_peer, &valid_key).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn a_peer_key_cannot_impersonate_an_unconfigured_node_id() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let probe = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("probe listener");
            let address = probe.local_addr().expect("probe address");
            drop(probe);

            let trusted_key = vec![1u8; 32];
            let local_key = vec![2u8; 32];
            let handle = MeshHandle::bind(
                2,
                Arc::new(AworsetBlocklist::new(2)),
                address,
                vec![MeshPeer::new(
                    1,
                    "127.0.0.1:1".parse().expect("peer address"),
                    trusted_key.clone(),
                )],
                local_key,
            )
            .await
            .expect("bind receiver");

            let now = now_ms();
            let body = MeshMessage::Block(ClusterBlockDelta {
                ip: IpAddr::V6(Ipv6Addr::LOCALHOST),
                dot: ClusterDot {
                    node_id: 1,
                    incarnation: uuid::Uuid::new_v4(),
                    counter: 1,
                },
                created_at_ms: now,
                expires_at_ms: now.saturating_add(60_000),
                tier: 1,
            });
            // The payload is validly signed with node 1's key, but claims node 3.
            let payload = serde_json::to_vec(&(now, 3u32, &body)).expect("encode payload");
            let envelope = Envelope {
                ts_ms: now,
                node_id: 3,
                body,
                mac: sign(&trusted_key, &payload).expect("sign forged identity"),
            };
            let mut stream = TcpStream::connect(address).await.expect("connect peer");
            stream
                .write_all(format!("{}\n", serde_json::to_string(&envelope).unwrap()).as_bytes())
                .await
                .expect("write signed frame");
            drop(stream);
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert!(
                handle.drain().await.is_empty(),
                "a key trusted for node 1 must not authenticate node 3"
            );
            handle.shutdown();
        });
    }

    #[test]
    fn broadcast_reports_connection_failure_per_peer() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let probe = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("probe listener");
            let unavailable_peer = probe.local_addr().expect("probe address");
            drop(probe);

            let handle = MeshHandle::bind(
                1,
                Arc::new(AworsetBlocklist::new(1)),
                "127.0.0.1:0".parse().expect("listen address"),
                vec![MeshPeer::new(2, unavailable_peer, vec![8; 32])],
                vec![7; 32],
            )
            .await
            .expect("mesh listener");

            let report = handle
                .broadcast(MeshMessage::Sync {
                    blocks: Vec::new(),
                    unblocks: Vec::new(),
                })
                .await;

            assert_eq!(report.peers.len(), 1);
            assert_eq!(report.written_count(), 0);
            assert_eq!(report.failed_count(), 1);
            assert!(!report.all_written());
            assert!(matches!(
                &report.peers[0].outcome,
                PeerSendOutcome::Failed { .. }
            ));
            handle.shutdown();
        });
    }

    #[test]
    fn shutdown_releases_mesh_listener() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let probe = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("probe listener");
            let address = probe.local_addr().expect("probe address");
            drop(probe);

            let handle = MeshHandle::bind(
                1,
                Arc::new(AworsetBlocklist::new(1)),
                address,
                Vec::new(),
                vec![7; 32],
            )
            .await
            .expect("mesh listener");
            handle.shutdown();

            tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    match TcpListener::bind(address).await {
                        Ok(listener) => {
                            drop(listener);
                            break;
                        }
                        Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
                    }
                }
            })
            .await
            .expect("mesh listener should release its socket after shutdown");
        });
    }

    #[test]
    fn rejects_future_creation_time_that_would_poison_hlc() {
        let delta = ClusterBlockDelta {
            ip: IpAddr::V6(Ipv6Addr::LOCALHOST),
            dot: ClusterDot {
                node_id: 7,
                incarnation: uuid::Uuid::new_v4(),
                counter: 1,
            },
            created_at_ms: u64::MAX,
            expires_at_ms: u64::MAX,
            tier: 1,
        };
        assert!(!valid_message(&MeshMessage::Block(delta), 1_000));
    }

    #[test]
    fn rejects_missing_incarnation_in_block_and_unblock_messages() {
        let ip = IpAddr::V6(Ipv6Addr::LOCALHOST);
        let now = now_ms();
        let block = ClusterBlockDelta {
            ip,
            dot: ClusterDot {
                node_id: 7,
                incarnation: uuid::Uuid::nil(),
                counter: 1,
            },
            created_at_ms: now,
            expires_at_ms: now.saturating_add(60_000),
            tier: 1,
        };
        assert!(!valid_message(&MeshMessage::Block(block.clone()), now));
        assert!(!valid_message(
            &MeshMessage::Unblock(ClusterUnblockDelta { ip, dot: block.dot }),
            now
        ));
    }

    #[test]
    fn rejects_sync_messages_over_entry_budget() {
        let blocks: Vec<_> = (0..=MAX_SYNC_FRAME_ENTRIES)
            .map(|counter| ClusterBlockDelta {
                ip: IpAddr::V6(Ipv6Addr::LOCALHOST),
                dot: ClusterDot {
                    node_id: 7,
                    incarnation: uuid::Uuid::new_v4(),
                    counter: counter as u64,
                },
                created_at_ms: 0,
                expires_at_ms: u64::MAX,
                tier: 1,
            })
            .collect();
        assert!(!valid_message(
            &MeshMessage::Sync {
                blocks,
                unblocks: Vec::new()
            },
            1_000,
        ));
    }

    #[test]
    fn sync_chunking_preserves_all_deltas_and_bounds_each_message() {
        let blocks: Vec<_> = (0..300u128)
            .map(|ip| ClusterBlockDelta {
                ip: IpAddr::V6(Ipv6Addr::from(ip)),
                dot: ClusterDot {
                    node_id: 7,
                    incarnation: uuid::Uuid::new_v4(),
                    counter: ip as u64,
                },
                created_at_ms: u64::MAX,
                expires_at_ms: u64::MAX,
                tier: 3,
            })
            .collect();
        let unblocks: Vec<_> = (300..570u128)
            .map(|ip| ClusterUnblockDelta {
                ip: IpAddr::V6(Ipv6Addr::from(ip)),
                dot: ClusterDot {
                    node_id: 8,
                    incarnation: uuid::Uuid::new_v4(),
                    counter: ip as u64,
                },
            })
            .collect();

        let messages = chunk_sync(&blocks, &unblocks);
        let mut block_count = 0;
        let mut unblock_count = 0;
        for message in messages {
            if let MeshMessage::Sync { blocks, unblocks } = message {
                assert!(blocks.len() + unblocks.len() <= MAX_SYNC_FRAME_ENTRIES);
                block_count += blocks.len();
                unblock_count += unblocks.len();
            } else {
                panic!("chunk_sync must emit only Sync messages");
            }
        }
        assert_eq!(block_count, 300);
        assert_eq!(unblock_count, 270);
    }

    #[test]
    fn maximum_sync_chunk_fits_authenticated_wire_frame() {
        let blocks: Vec<_> = (0..MAX_SYNC_FRAME_ENTRIES)
            .map(|_| ClusterBlockDelta {
                ip: IpAddr::V6(Ipv6Addr::from(u128::MAX)),
                dot: ClusterDot {
                    node_id: u32::MAX,
                    incarnation: uuid::Uuid::new_v4(),
                    counter: u64::MAX,
                },
                created_at_ms: u64::MAX,
                expires_at_ms: u64::MAX,
                tier: u8::MAX,
            })
            .collect();
        let message = MeshMessage::Sync {
            blocks,
            unblocks: Vec::new(),
        };
        let envelope = Envelope {
            ts_ms: u64::MAX,
            node_id: u32::MAX,
            body: message,
            mac: "0".repeat(64),
        };
        let encoded = serde_json::to_vec(&envelope).expect("serialize worst-case sync frame");
        assert!(
            encoded.len() <= MAX_FRAME,
            "encoded frame is {} bytes",
            encoded.len()
        );
    }

    #[test]
    fn outbound_send_deadline_releases_stalled_send() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let error = with_timeout(Duration::from_millis(10), std::future::pending())
                .await
                .unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        });
    }

    #[test]
    fn accepts_frame_at_limit_with_delimiter() {
        let mut frame = Vec::new();
        assert!(!push_frame_bytes(&mut frame, &vec![b'x'; MAX_FRAME]).unwrap());
        assert!(push_frame_bytes(&mut frame, b"\n").unwrap());
        assert_eq!(frame.len(), MAX_FRAME + 1);
        assert_eq!(frame.last(), Some(&b'\n'));
    }

    #[test]
    fn rejects_oversized_frame_before_appending_more_bytes() {
        let mut frame = Vec::new();
        assert!(!push_frame_bytes(&mut frame, &vec![b'x'; MAX_FRAME]).unwrap());
        let error = push_frame_bytes(&mut frame, b"x").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(frame.len(), MAX_FRAME);
    }

    #[test]
    fn rejects_oversized_frame_when_delimiter_arrives_late() {
        let mut frame = vec![b'x'; MAX_FRAME];
        let error = push_frame_bytes(&mut frame, b"x\n").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(frame.len(), MAX_FRAME);
    }
}
