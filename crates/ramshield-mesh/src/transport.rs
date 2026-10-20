use crate::aworset::PeerEpoch;
use crate::aworset::{AworsetBlocklist, AworsetSnapshot, ClusterBlockDelta, ClusterUnblockDelta};
use crate::condition_sync::{ConditionSyncPlanner, SyncCondition, SyncPolicy};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    future::Future,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Mutex, Semaphore, oneshot, watch},
    task::JoinSet,
    time::MissedTickBehavior,
};
use tracing::{debug, warn};

type HmacSha256 = Hmac<Sha256>;
const MAX_FRAME: usize = 64 * 1024;
const MAX_CLOCK_SKEW_MS: u64 = 30_000;
const MAX_SYNC_FRAME_ENTRIES: usize = 16;
const MAX_RECOVERY_ENTRIES: usize = 65_536;
const MAX_RECOVERY_TOTAL_ENTRIES: usize = 65_536;
const MAX_RECOVERY_FRAMES: usize = 8_192;
const MAX_RECOVERY_TOTAL_FRAMES: usize = 8_192;
const MAX_INCOMING_QUEUE: usize = 2048;
const MAX_PEER_READERS: usize = 256;
const MAX_CONFIGURED_PEERS: usize = 256;
const MAX_FRAME_READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PEER_WRITERS: usize = 64;
const MAX_PEER_WRITE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_APPLICATION_ACK_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MeshMutationKind {
    Block,
    Unblock,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplicatedMeshMutation {
    /// LSN in the sending node's WAL, not the origin node's CRDT counter.
    pub source_lsn: u64,
    pub origin_node_id: u32,
    pub origin_incarnation: String,
    pub origin_counter: u64,
    pub ip: IpAddr,
    pub action: MeshMutationKind,
    pub created_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub tier: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshMessage {
    Block(ClusterBlockDelta),
    Unblock(ClusterUnblockDelta),
    Sync {
        blocks: Vec<ClusterBlockDelta>,
        unblocks: Vec<ClusterUnblockDelta>,
    },
    WalBatch {
        source_node_id: u32,
        start_lsn: u64,
        end_lsn: u64,
        mutations: Vec<ReplicatedMeshMutation>,
    },
}

/// A configured mesh peer: its node identity, its transport address, and the
/// shared HMAC secret for the undirected edge to that peer.
///
/// Both endpoints of an edge must configure the identical secret. Frames and
/// application ACKs on that edge are signed and verified with this secret.
///
/// A valid MAC proves possession of the edge secret, not the unique identity
/// of the process that produced it: both endpoints of an edge hold the same
/// secret, so HMAC alone cannot attribute a frame to one of them. Peer
/// attribution therefore also depends on the source-address binding enforced
/// on inbound frames and on the confidentiality of the edge secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshPeer {
    /// Stable node identity of the peer.
    pub node_id: u32,
    /// Address the peer listens on.
    pub address: SocketAddr,
    /// Shared HMAC secret for the undirected edge to this peer.
    ///
    /// Must be identical at both ends of the edge, and must differ from the
    /// secret of every other edge configured on this node.
    pub auth_key: Vec<u8>,
}

impl MeshPeer {
    /// Configure a peer by node identity, address, and per-peer key.
    pub fn new(node_id: u32, address: SocketAddr, auth_key: Vec<u8>) -> Self {
        Self {
            node_id,
            address,
            auth_key,
        }
    }
}

/// Result of attempting to deliver a mesh message to one configured peer.
/// `Written` means the receiver returned an authenticated application ACK after
/// durable acceptance (or intentional stale/policy rejection), not just TCP write.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum WireMessage {
    Standard(MeshMessage),
    Recovery(RecoveryWireMessage),
}

impl From<MeshMessage> for WireMessage {
    fn from(message: MeshMessage) -> Self {
        Self::Standard(message)
    }
}

/// One tombstone acknowledgment carried by a recovery completion.
///
/// A list of entries, not a map: JSON object keys are strings, so a map
/// keyed by the peer node ID cannot be deserialized back into integer keys
/// (`serde_json` rejects `"7"` for a `u32` key), and a map has no stable
/// iteration order — the completion body is covered by the frame MAC, so
/// the sender and the receiver must serialize it identically. The list is
/// ordered by `peer_node_id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TombstoneAck {
    /// Node ID of the peer that acknowledged the tombstones.
    pub peer_node_id: u32,
    /// The peer's acknowledgment epoch.
    pub epoch: PeerEpoch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RecoveryWireMessage {
    Frame {
        generation: u64,
        recovery_id: uuid::Uuid,
        boundary_lsn: u64,
        snapshot_digest: String,
        frame_index: u32,
        frame_count: u32,
        blocks: Vec<ClusterBlockDelta>,
        unblocks: Vec<ClusterUnblockDelta>,
    },
    Complete {
        generation: u64,
        recovery_id: uuid::Uuid,
        boundary_lsn: u64,
        snapshot_digest: String,
        frame_count: u32,
        /// Tombstone acknowledgments from the recipient to the sender.
        /// One entry per peer, ordered by `peer_node_id`.
        tombstone_acks: Vec<TombstoneAck>,
    },
}

#[derive(Debug, Clone)]
enum ReceivedRecovery {
    Frame {
        generation: u64,
        recovery_id: uuid::Uuid,
        boundary_lsn: u64,
        snapshot_digest: String,
        frame_index: u32,
        frame_count: u32,
    },
    Complete {
        generation: u64,
        recovery_id: uuid::Uuid,
        boundary_lsn: u64,
        snapshot_digest: String,
        frame_count: u32,
    },
}

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
/// This requires the receiver's authenticated application acknowledgement.
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
    body: WireMessage,
    mac: String,
}
struct PeerSyncState {
    planner: ConditionSyncPlanner,
    recovery_snapshot: Option<Arc<AworsetSnapshot>>,
    delivery_failed: bool,
    generation: u64,
    recovery_boundary_lsn: Option<u64>,
    recovery_id: Option<uuid::Uuid>,
    completed_recovery: Option<(u64, u64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RecoveryProof {
    generation: u64,
    recovery_id: uuid::Uuid,
    boundary_lsn: u64,
    snapshot_digest: String,
    frame_count: u32,
}

#[derive(Debug, Clone)]
struct RecoveryContext {
    generation: u64,
    recovery_id: uuid::Uuid,
    boundary_lsn: u64,
    /// Tombstone acknowledgment epochs published with the recovery completion.
    ///
    /// Ordered by `peer_node_id` for the same reason as the wire field: the
    /// list is part of the MAC-covered completion body.
    tombstone_acks: Vec<TombstoneAck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecoveryFramePayload {
    blocks: Vec<ClusterBlockDelta>,
    unblocks: Vec<ClusterUnblockDelta>,
}

struct RecoveryAssembly {
    generation: u64,
    recovery_id: uuid::Uuid,
    boundary_lsn: u64,
    snapshot_digest: String,
    frame_count: u32,
    frames: Vec<Option<RecoveryFramePayload>>,
    entry_count: usize,
    completed_proof: Option<RecoveryProof>,
}

#[derive(Debug, Clone)]
struct AckResponse {
    accepted: bool,
    ack_lsn: Option<u64>,
    recovery_proof: Option<RecoveryProof>,
}

struct IncomingFrame {
    sender_node_id: u32,
    message: MeshMessage,
    recovery: Option<ReceivedRecovery>,
    ack: oneshot::Sender<AckResponse>,
}

pub struct ReceivedMeshMessage {
    pub message: MeshMessage,
    sender_node_id: u32,
    recovery: Option<ReceivedRecovery>,
    blocklist: Arc<AworsetBlocklist>,
    receiver_recovery: Arc<StdMutex<HashMap<u32, RecoveryAssembly>>>,
    ack: Option<oneshot::Sender<AckResponse>>,
}

impl ReceivedMeshMessage {
    pub fn acknowledge(self, accepted: bool) {
        self.acknowledge_with_lsn(accepted, None);
    }

    /// Acknowledge a received message, optionally reporting the WAL LSN that
    /// makes the accepted state durable.
    ///
    /// Durability contract: the caller must have persisted the message's
    /// deltas (WAL append) and merged them into the CRDT before acking.
    /// A recovery completion proof is derived from this acknowledgement, so
    /// acking before the state is durable would let the sender advance its
    /// per-peer replication cursor while the receiver can still lose the
    /// acknowledged state on restart.
    pub fn acknowledge_with_lsn(mut self, accepted: bool, ack_lsn: Option<u64>) {
        let mut accepted = accepted;
        let mut recovery_proof = None;
        match self.recovery.as_ref() {
            Some(ReceivedRecovery::Frame {
                generation,
                recovery_id,
                boundary_lsn,
                snapshot_digest,
                frame_index,
                frame_count,
            }) if accepted => {
                if let MeshMessage::Sync { blocks, unblocks } = &self.message {
                    let now = now_ms();
                    let crdt_applied = blocks.iter().all(|delta| {
                        delta.expires_at_ms <= now || !self.blocklist.needs_merge_delta(delta)
                    }) && unblocks
                        .iter()
                        .all(|delta| !self.blocklist.needs_merge_unblock(delta));
                    accepted = crdt_applied
                        && record_recovery_frame(
                            &self.receiver_recovery,
                            self.sender_node_id,
                            *generation,
                            *recovery_id,
                            *boundary_lsn,
                            snapshot_digest,
                            *frame_index,
                            *frame_count,
                            blocks,
                            unblocks,
                        );
                } else {
                    accepted = false;
                }
            }
            Some(ReceivedRecovery::Complete {
                generation,
                recovery_id,
                boundary_lsn,
                snapshot_digest,
                frame_count,
            }) if accepted => {
                recovery_proof = complete_recovery(
                    &self.receiver_recovery,
                    self.sender_node_id,
                    *generation,
                    *recovery_id,
                    *boundary_lsn,
                    snapshot_digest,
                    *frame_count,
                );
                accepted = recovery_proof.is_some();
            }
            _ => {}
        }
        if let Some(ack) = self.ack.take() {
            let _ = ack.send(AckResponse {
                accepted,
                ack_lsn: if accepted { ack_lsn } else { None },
                recovery_proof: if accepted { recovery_proof } else { None },
            });
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AckEnvelope {
    ts_ms: u64,
    node_id: u32,
    accepted: bool,
    ack_lsn: Option<u64>,
    #[serde(default)]
    recovery_proof: Option<RecoveryProof>,
    mac: String,
}

#[allow(clippy::too_many_arguments)]
fn record_recovery_frame(
    sessions: &Arc<StdMutex<HashMap<u32, RecoveryAssembly>>>,
    sender_node_id: u32,
    generation: u64,
    recovery_id: uuid::Uuid,
    boundary_lsn: u64,
    snapshot_digest: &str,
    frame_index: u32,
    frame_count: u32,
    blocks: &[ClusterBlockDelta],
    unblocks: &[ClusterUnblockDelta],
) -> bool {
    let mut sessions = sessions.lock().unwrap_or_else(|error| error.into_inner());
    let index = frame_index as usize;
    let frame_count_usize = frame_count as usize;
    let payload = RecoveryFramePayload {
        blocks: blocks.to_vec(),
        unblocks: unblocks.to_vec(),
    };
    let added = blocks.len().saturating_add(unblocks.len());
    let total_entries: usize = sessions.values().map(|session| session.entry_count).sum();
    let total_frames: usize = sessions.values().map(|session| session.frames.len()).sum();

    if let Some(current) = sessions.get_mut(&sender_node_id) {
        if current.generation == generation && current.recovery_id == recovery_id {
            if current.boundary_lsn != boundary_lsn
                || current.snapshot_digest != snapshot_digest
                || current.frame_count != frame_count
                || index >= current.frames.len()
            {
                return false;
            }
            if let Some(existing) = current.frames[index].as_ref() {
                return existing == &payload;
            }
            if current.entry_count.saturating_add(added) > MAX_RECOVERY_ENTRIES
                || total_entries.saturating_add(added) > MAX_RECOVERY_TOTAL_ENTRIES
            {
                return false;
            }
            current.frames[index] = Some(payload);
            current.entry_count += added;
            return true;
        }
        if generation <= current.generation
            || boundary_lsn < current.boundary_lsn
            || frame_index != 0
        {
            return false;
        }
    } else if frame_index != 0 || sessions.len() >= MAX_CONFIGURED_PEERS {
        return false;
    }

    let old_entries = sessions
        .get(&sender_node_id)
        .map(|s| s.entry_count)
        .unwrap_or(0);
    let old_frames = sessions
        .get(&sender_node_id)
        .map(|s| s.frames.len())
        .unwrap_or(0);
    if total_entries
        .saturating_sub(old_entries)
        .saturating_add(added)
        > MAX_RECOVERY_TOTAL_ENTRIES
        || total_frames
            .saturating_sub(old_frames)
            .saturating_add(frame_count_usize)
            > MAX_RECOVERY_TOTAL_FRAMES
    {
        return false;
    }
    let mut frames = vec![None; frame_count_usize];
    frames[index] = Some(payload);
    sessions.insert(
        sender_node_id,
        RecoveryAssembly {
            generation,
            recovery_id,
            boundary_lsn,
            snapshot_digest: snapshot_digest.to_owned(),
            frame_count,
            frames,
            entry_count: added,
            completed_proof: None,
        },
    );
    true
}

fn complete_recovery(
    sessions: &Arc<StdMutex<HashMap<u32, RecoveryAssembly>>>,
    sender_node_id: u32,
    generation: u64,
    recovery_id: uuid::Uuid,
    boundary_lsn: u64,
    snapshot_digest: &str,
    frame_count: u32,
) -> Option<RecoveryProof> {
    let mut sessions = sessions.lock().unwrap_or_else(|error| error.into_inner());
    let session = sessions.get_mut(&sender_node_id)?;
    if session.generation != generation
        || session.recovery_id != recovery_id
        || session.boundary_lsn != boundary_lsn
        || session.snapshot_digest != snapshot_digest
        || session.frame_count != frame_count
    {
        return None;
    }
    if let Some(proof) = session.completed_proof.as_ref() {
        return Some(proof.clone());
    }
    if session.frames.iter().any(Option::is_none) {
        return None;
    }
    let mut snapshot = AworsetSnapshot::default();
    for frame in session.frames.iter().flatten() {
        snapshot.blocks.extend(frame.blocks.iter().cloned());
        snapshot.unblocks.extend(frame.unblocks.iter().cloned());
    }
    let snapshot = canonical_snapshot(snapshot).ok()?;
    let digest = canonical_snapshot_digest(&snapshot).ok()?;
    if digest != snapshot_digest {
        return None;
    }
    let proof = RecoveryProof {
        generation,
        recovery_id,
        boundary_lsn,
        snapshot_digest: digest,
        frame_count,
    };
    session.completed_proof = Some(proof.clone());
    Some(proof)
}

#[derive(Clone)]
pub struct MeshHandle {
    node_id: u32,
    blocklist: Arc<AworsetBlocklist>,
    peers: Arc<Vec<MeshPeer>>,
    /// Edge secret for each configured peer, keyed by peer node ID.
    ///
    /// This is the only key an inbound frame is verified against: the claimed
    /// node ID selects the edge, and the edge secret proves possession of it.
    trusted_keys: Arc<HashMap<u32, Vec<u8>>>,
    /// Retained for API compatibility with `bind`. Mesh frame and application
    /// ACK MACs use the per-edge secrets in `trusted_keys`, not this value.
    #[allow(dead_code)]
    auth_key: Arc<Vec<u8>>,
    incoming: Arc<Mutex<VecDeque<IncomingFrame>>>,
    readers: Arc<Semaphore>,
    writers: Arc<Semaphore>,
    shutdown_tx: watch::Sender<bool>,
    sync_states: Arc<Mutex<HashMap<SocketAddr, PeerSyncState>>>,
    receiver_recovery: Arc<StdMutex<HashMap<u32, RecoveryAssembly>>>,
    sync_epoch: Instant,
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
        let trusted_keys: HashMap<u32, Vec<u8>> = peers
            .iter()
            .map(|peer| (peer.node_id, peer.auth_key.clone()))
            .collect();
        // Tombstone collection waits for an acknowledgment from every
        // configured peer, so the blocklist must know the peer set.
        blocklist.set_expected_peers(peers.iter().map(|peer| peer.node_id));
        let sync_states = peers
            .iter()
            .map(|peer| {
                ConditionSyncPlanner::new(SyncPolicy::default())
                    .map(|planner| {
                        (
                            peer.address,
                            PeerSyncState {
                                planner,
                                recovery_snapshot: None,
                                delivery_failed: false,
                                generation: now_ms(),
                                recovery_boundary_lsn: None,
                                recovery_id: None,
                                completed_recovery: None,
                            },
                        )
                    })
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
            })
            .collect::<std::io::Result<HashMap<_, _>>>()?;
        let listener = TcpListener::bind(listen).await?;
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = Self {
            node_id,
            blocklist,
            peers: Arc::new(peers),
            trusted_keys: Arc::new(trusted_keys),
            auth_key: Arc::new(auth_key),
            incoming: Arc::new(Mutex::new(VecDeque::new())),
            readers: Arc::new(Semaphore::new(MAX_PEER_READERS)),
            writers: Arc::new(Semaphore::new(MAX_PEER_WRITERS)),
            shutdown_tx,
            sync_states: Arc::new(Mutex::new(sync_states)),
            receiver_recovery: Arc::new(StdMutex::new(HashMap::new())),
            sync_epoch: Instant::now(),
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
                        Ok((stream, remote)) => {
                            let h = accept_handle.clone();
                            let Ok(permit) = h.readers.clone().try_acquire_owned() else { continue; };
                            tokio::spawn(async move {
                                h.read_stream(stream, remote).await;
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
            let mut tick = tokio::time::interval(Duration::from_millis(25));
            tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
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

    /// Local node identity carried by authenticated WAL batches.
    pub fn node_id(&self) -> u32 {
        self.node_id
    }

    /// Configured replication targets.
    pub fn peers(&self) -> Vec<SocketAddr> {
        self.peers.iter().map(|peer| peer.address).collect()
    }

    /// Resolve the configured peer behind an address, if any.
    fn peer_by_address(&self, address: SocketAddr) -> Option<&MeshPeer> {
        self.peers.iter().find(|peer| peer.address == address)
    }

    pub async fn request_full_sync(
        &self,
        peer: SocketAddr,
        boundary_lsn: u64,
        snapshot: AworsetSnapshot,
    ) {
        if snapshot
            .blocks
            .len()
            .saturating_add(snapshot.unblocks.len())
            > MAX_RECOVERY_ENTRIES
        {
            warn!(peer = %peer, max_entries = MAX_RECOVERY_ENTRIES, "recovery snapshot exceeds bounded protocol limit; cursor remains pinned");
            return;
        }
        let snapshot = match canonical_snapshot(snapshot) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                warn!(peer = %peer, error, "invalid recovery snapshot");
                return;
            }
        };
        let now = self.sync_epoch.elapsed();
        if let Some(state) = self.sync_states.lock().await.get_mut(&peer)
            && state.recovery_boundary_lsn.is_none()
            && state.completed_recovery.is_none()
        {
            let Some(generation) = state.generation.checked_add(1) else {
                warn!(peer = %peer, "recovery generation overflow; refusing reuse");
                return;
            };
            state.generation = generation;
            state.recovery_id = Some(uuid::Uuid::new_v4());
            state.recovery_snapshot = Some(Arc::new(snapshot));
            state.recovery_boundary_lsn = Some(boundary_lsn);
            state.planner.condition(SyncCondition::LocalMutation, now);
        }
    }

    pub async fn recovery_completion_pending(&self, peer: SocketAddr) -> bool {
        self.sync_states
            .lock()
            .await
            .get(&peer)
            .is_some_and(|state| {
                state.recovery_boundary_lsn.is_some() || state.completed_recovery.is_some()
            })
    }

    pub async fn completed_recovery(&self, peer: SocketAddr) -> Option<(u64, u64)> {
        self.sync_states
            .lock()
            .await
            .get(&peer)
            .and_then(|state| state.completed_recovery)
    }

    pub async fn acknowledge_recovery_cursor(
        &self,
        peer: SocketAddr,
        generation: u64,
        boundary_lsn: u64,
    ) {
        let mut states = self.sync_states.lock().await;
        if let Some(state) = states.get_mut(&peer)
            && state.completed_recovery == Some((generation, boundary_lsn))
        {
            state.completed_recovery = None;
        }
    }

    /// Send a WAL batch and return the sender-local LSN durably acknowledged
    /// by the receiver.
    pub async fn send_wal_batch(
        &self,
        peer: SocketAddr,
        body: MeshMessage,
    ) -> std::io::Result<Option<u64>> {
        let _permit = self
            .writers
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let response = tokio::time::timeout(MAX_PEER_WRITE_TIMEOUT, self.send_to_ack(peer, body))
            .await
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "mesh WAL batch ACK timed out")
            })??;
        Ok(response.ack_lsn)
    }

    /// Send to configured peers and return one explicit outcome per peer.
    ///
    /// A `Written` result now means the peer returned an authenticated
    /// application ACK after processing the frame. Concurrency is bounded
    /// before each task is spawned.
    pub async fn broadcast(&self, body: MeshMessage) -> BroadcastReport {
        self.mark_all_sync(SyncCondition::LocalMutation).await;
        let mut tasks = JoinSet::new();
        let mut report = BroadcastReport::default();

        for peer in self.peers.iter().map(|peer| peer.address) {
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
                        PeerSendOutcome::Written => {
                            self.note_peer_success(peer).await;
                        }
                        PeerSendOutcome::TimedOut => {
                            self.condition_peer(peer, SyncCondition::DeliveryFailed)
                                .await;
                            warn!(peer = %peer, "mesh outbound send timed out");
                        }
                        PeerSendOutcome::Cancelled => {
                            self.condition_peer(peer, SyncCondition::DeliveryFailed)
                                .await;
                            debug!(peer = %peer, "mesh outbound send cancelled");
                        }
                        PeerSendOutcome::Failed { message } => {
                            self.condition_peer(peer, SyncCondition::DeliveryFailed)
                                .await;
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
    /// Legacy drain API. Dropping each acknowledgement responder causes the
    /// sender to retry; production application code should use drain_with_ack.
    pub async fn drain(&self) -> Vec<MeshMessage> {
        let mut q = self.incoming.lock().await;
        q.drain(..).map(|frame| frame.message).collect()
    }

    /// Drain incoming frames while retaining their acknowledgement handles.
    /// A frame is not considered delivered until the caller acknowledges after
    /// durable application.
    pub async fn drain_with_ack(&self) -> Vec<ReceivedMeshMessage> {
        let mut q = self.incoming.lock().await;
        q.drain(..)
            .map(|frame| ReceivedMeshMessage {
                message: frame.message,
                sender_node_id: frame.sender_node_id,
                recovery: frame.recovery,
                blocklist: self.blocklist.clone(),
                receiver_recovery: self.receiver_recovery.clone(),
                ack: Some(frame.ack),
            })
            .collect()
    }

    /// Call after drained remote messages have actually been merged into the
    /// local CRDT. Draining alone is not treated as application success.
    pub async fn notify_remote_state_applied(&self) {
        self.mark_all_sync(SyncCondition::RemoteStateApplied).await;
    }

    async fn mark_all_sync(&self, condition: SyncCondition) {
        let now = self.sync_epoch.elapsed();
        let mut states = self.sync_states.lock().await;
        for state in states.values_mut() {
            state.planner.condition(condition, now);
        }
    }

    async fn condition_peer(&self, peer: SocketAddr, condition: SyncCondition) {
        let now = self.sync_epoch.elapsed();
        if let Some(state) = self.sync_states.lock().await.get_mut(&peer) {
            if condition == SyncCondition::DeliveryFailed {
                state.delivery_failed = true;
            }
            state.planner.condition(condition, now);
        }
    }

    async fn note_peer_success(&self, peer: SocketAddr) {
        let now = self.sync_epoch.elapsed();
        let mut states = self.sync_states.lock().await;
        match states.get_mut(&peer) {
            Some(state) if state.delivery_failed => {
                state.delivery_failed = false;
                state.planner.condition(SyncCondition::PeerReconnected, now);
            }
            _ => {}
        }
    }

    async fn broadcast_sync(&self) {
        let now = self.sync_epoch.elapsed();
        let blocklist = self.blocklist.clone();
        let due: Vec<(
            SocketAddr,
            Option<Arc<AworsetSnapshot>>,
            Option<RecoveryContext>,
        )> = {
            let mut states = self.sync_states.lock().await;
            states
                .iter_mut()
                .filter_map(|(peer, state)| {
                    state.planner.take_due(now).map(|_| {
                        let recovery = state.recovery_boundary_lsn.zip(state.recovery_id).map(
                            |(boundary_lsn, recovery_id)| RecoveryContext {
                                generation: state.generation,
                                recovery_id,
                                boundary_lsn,
                                tombstone_acks: canonical_tombstone_acks(
                                    blocklist.peer_ack_epochs(),
                                ),
                            },
                        );
                        (*peer, state.recovery_snapshot.clone(), recovery)
                    })
                })
                .collect()
        };
        if due.is_empty() {
            return;
        }
        let mut tasks = JoinSet::new();
        for (peer, snapshot, recovery) in due.iter().cloned() {
            let handle = self.clone();
            tasks.spawn(async move {
                let (success, proof) = handle
                    .send_sync_snapshot(peer, snapshot, recovery.clone())
                    .await;
                (peer, success, proof, recovery)
            });
        }
        let mut finished = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok((peer, success, proof, recovery)) => {
                    finished.push(peer);
                    let at = self.sync_epoch.elapsed();
                    let mut states = self.sync_states.lock().await;
                    if let Some(state) = states.get_mut(&peer) {
                        if recovery
                            .as_ref()
                            .is_some_and(|ctx| ctx.generation != state.generation)
                        {
                            state.planner.complete(at, true);
                            state.delivery_failed = false;
                            state.planner.condition(SyncCondition::LocalMutation, at);
                            continue;
                        }
                        let proof_matches = match (recovery.as_ref(), proof.as_ref()) {
                            (None, None) => true,
                            (Some(ctx), Some(proof)) => {
                                proof.generation == ctx.generation
                                    && proof.recovery_id == ctx.recovery_id
                                    && proof.boundary_lsn == ctx.boundary_lsn
                                    && state.recovery_boundary_lsn == Some(ctx.boundary_lsn)
                                    && state.recovery_id == Some(ctx.recovery_id)
                            }
                            _ => false,
                        };
                        let success = success && proof_matches;
                        state.planner.complete(at, success);
                        state.delivery_failed = !success;
                        if success && let Some(ctx) = recovery {
                            state.completed_recovery = Some((ctx.generation, ctx.boundary_lsn));
                            state.recovery_boundary_lsn = None;
                            state.recovery_snapshot = None;
                            state.recovery_id = None;
                        }
                    }
                }
                Err(error) => warn!(error = %error, "condition-driven mesh sync task failed"),
            }
        }
        let at = self.sync_epoch.elapsed();
        let mut states = self.sync_states.lock().await;
        for (peer, _, recovery) in due {
            if finished.contains(&peer) {
                continue;
            }
            let superseded = recovery.as_ref().is_some_and(|ctx| {
                ctx.generation != states.get(&peer).map(|s| s.generation).unwrap_or(0)
            });
            if let Some(state) = states.get_mut(&peer) {
                state.planner.complete(at, superseded);
                state.delivery_failed = !superseded;
                if superseded {
                    state.planner.condition(SyncCondition::LocalMutation, at);
                }
            }
        }
    }

    async fn send_sync_snapshot(
        &self,
        peer: SocketAddr,
        fixed_snapshot: Option<Arc<AworsetSnapshot>>,
        recovery: Option<RecoveryContext>,
    ) -> (bool, Option<RecoveryProof>) {
        let started = Instant::now();
        if recovery.is_some() && fixed_snapshot.is_none() {
            warn!(peer = %peer, "recovery metadata without fixed snapshot");
            return (false, None);
        }
        let is_recovery = recovery.is_some();
        let snapshot = fixed_snapshot.unwrap_or_else(|| Arc::new(self.blocklist.export_snapshot()));
        let blocks_count = snapshot.blocks.len();
        let tombstones_count = snapshot.unblocks.len();
        if is_recovery && blocks_count.saturating_add(tombstones_count) > MAX_RECOVERY_ENTRIES {
            warn!(peer = %peer, blocks_count, tombstones_count, "snapshot exceeds bounded recovery entry limit");
            return (false, None);
        }
        let canonical = match canonical_snapshot(snapshot.as_ref().clone()) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                warn!(peer = %peer, error, "invalid fixed snapshot");
                return (false, None);
            }
        };
        let mut messages = chunk_sync(&canonical.blocks, &canonical.unblocks);
        if messages.is_empty() {
            messages.push(MeshMessage::Sync {
                blocks: Vec::new(),
                unblocks: Vec::new(),
            });
        }
        let (frames, recovery_metadata) = if let Some(ctx) = recovery.as_ref() {
            let digest = match canonical_snapshot_digest(&canonical) {
                Ok(digest) => digest,
                Err(error) => {
                    warn!(peer = %peer, error = %error, "cannot digest recovery snapshot");
                    return (false, None);
                }
            };
            let Ok(frame_count) = u32::try_from(messages.len()) else {
                return (false, None);
            };
            if messages.len() > MAX_RECOVERY_FRAMES {
                return (false, None);
            }
            let frames = messages
                .into_iter()
                .enumerate()
                .map(|(index, message)| {
                    let (blocks, unblocks) = match message {
                        MeshMessage::Sync { blocks, unblocks } => (blocks, unblocks),
                        _ => unreachable!("chunk_sync only returns Sync"),
                    };
                    WireMessage::Recovery(RecoveryWireMessage::Frame {
                        generation: ctx.generation,
                        recovery_id: ctx.recovery_id,
                        boundary_lsn: ctx.boundary_lsn,
                        snapshot_digest: digest.clone(),
                        frame_index: index as u32,
                        frame_count,
                        blocks,
                        unblocks,
                    })
                })
                .collect::<Vec<_>>();
            (frames, Some((ctx.clone(), digest, frame_count)))
        } else {
            (
                messages
                    .into_iter()
                    .map(WireMessage::Standard)
                    .collect::<Vec<_>>(),
                None,
            )
        };
        let frame_total = frames.len();
        let mut shutdown = self.shutdown_tx.subscribe();
        if *shutdown.borrow() {
            return (false, None);
        }
        let permit = tokio::select! {
            permit = self.writers.clone().acquire_owned() => match permit { Ok(p) => p, Err(_) => return (false, None) },
            _ = shutdown.changed() => return (false, None),
        };
        for (frame_index, message) in frames.into_iter().enumerate() {
            let result = tokio::select! {
                result = with_timeout(MAX_PEER_WRITE_TIMEOUT, async { self.send_to_ack(peer, message).await.map(|_| ()) }) => result,
                changed = shutdown.changed() => { let _ = changed; Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "mesh shutdown")) }
            };
            if let Err(error) = result {
                warn!(peer = %peer, frame_index, frame_total, blocks_count, tombstones_count, is_recovery, elapsed_ms = started.elapsed().as_millis() as u64, error = %error, "mesh snapshot frame failed");
                drop(permit);
                return (false, None);
            }
        }
        if let Some((ctx, digest, frame_count)) = recovery_metadata {
            let response = tokio::select! {
                result = with_timeout(MAX_PEER_WRITE_TIMEOUT, async {
                    self.send_to_ack(peer, WireMessage::Recovery(RecoveryWireMessage::Complete {
                        generation: ctx.generation, recovery_id: ctx.recovery_id,
                        boundary_lsn: ctx.boundary_lsn, snapshot_digest: digest.clone(), frame_count,
                        tombstone_acks: ctx.tombstone_acks.clone(),
                    })).await
                }) => match result {
                    Ok(response) => response,
                    Err(error) => {
                        let timed_out = error.kind() == std::io::ErrorKind::TimedOut;
                        warn!(peer = %peer, error = %error, timed_out, "recovery completion rejected");
                        drop(permit);
                        return (false, None);
                    }
                },
                changed = shutdown.changed() => { let _ = changed; drop(permit); return (false, None); }
            };
            let expected = RecoveryProof {
                generation: ctx.generation,
                recovery_id: ctx.recovery_id,
                boundary_lsn: ctx.boundary_lsn,
                snapshot_digest: digest,
                frame_count,
            };
            if response.recovery_proof.as_ref() != Some(&expected) {
                warn!(peer = %peer, generation = ctx.generation, boundary_lsn = ctx.boundary_lsn, "missing/mismatched receiver proof; cursor pinned");
                drop(permit);
                return (false, None);
            }
            // The peer confirmed that it durably applied this node's state up
            // to the recovery boundary, so the local tombstone bookkeeping may
            // record that acknowledgment.
            if let Some(peer_config) = self.peer_by_address(peer) {
                self.blocklist
                    .peer_acknowledged(peer_config.node_id, ctx.boundary_lsn);
            }
            drop(permit);
            debug!(peer = %peer, blocks_count, tombstones_count, frame_count, generation = ctx.generation, boundary_lsn = ctx.boundary_lsn, elapsed_ms = started.elapsed().as_millis() as u64, "receiver-confirmed recovery complete");
            return (true, Some(expected));
        }
        drop(permit);
        debug!(peer = %peer, blocks_count, tombstones_count, frame_total, is_recovery, elapsed_ms = started.elapsed().as_millis() as u64, "condition-driven snapshot acknowledged");
        (true, None)
    }
    async fn send_to(&self, peer: SocketAddr, body: MeshMessage) -> std::io::Result<()> {
        self.send_to_ack(peer, body).await.map(|_| ())
    }

    async fn send_to_ack<B: Into<WireMessage>>(
        &self,
        peer: SocketAddr,
        body: B,
    ) -> std::io::Result<AckResponse> {
        // The peer must be configured: its identity and its edge secret are
        // what the frame is signed with and the application ACK is verified
        // against.
        let peer_cfg = self.peer_by_address(peer).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "mesh peer is not configured",
            )
        })?;
        let peer_node_id = peer_cfg.node_id;
        let edge_key = peer_cfg.auth_key.clone();
        let body = body.into();
        let ts_ms = now_ms();
        let payload =
            serde_json::to_vec(&(ts_ms, self.node_id, &body)).map_err(std::io::Error::other)?;
        // Sign with the edge secret shared with that peer: the receiver
        // verifies the frame with the same secret for this edge.
        let mac = sign(&edge_key, &payload)?;
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

        let ack_frame = read_bounded_frame_with_timeout(&mut stream, MAX_PEER_WRITE_TIMEOUT)
            .await?
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "peer closed connection before application ACK",
                )
            })?;
        let ack_line = ack_frame.strip_suffix(b"\n").unwrap_or(&ack_frame);
        let ack_line = ack_line.strip_suffix(b"\r").unwrap_or(ack_line);
        let ack: AckEnvelope = serde_json::from_slice(ack_line).map_err(std::io::Error::other)?;
        let ack_payload = serde_json::to_vec(&(
            ack.ts_ms,
            ack.node_id,
            ack.accepted,
            ack.ack_lsn,
            &ack.recovery_proof,
        ))
        .map_err(std::io::Error::other)?;
        // The ACK must come from the configured peer itself and be signed
        // with the edge secret shared with that peer: any other secret would
        // let another peer answer on behalf of it.
        if ack.node_id != peer_node_id
            || now_ms().abs_diff(ack.ts_ms) > MAX_CLOCK_SKEW_MS
            || !verify(&edge_key, &ack_payload, &ack.mac)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "peer application ACK authentication failed",
            ));
        }
        if !ack.accepted {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "peer did not durably accept mesh frame; retry required",
            ));
        }
        Ok(AckResponse {
            accepted: ack.accepted,
            ack_lsn: ack.ack_lsn,
            recovery_proof: ack.recovery_proof,
        })
    }
    async fn read_stream(&self, mut stream: TcpStream, remote: SocketAddr) {
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
        if env.node_id == self.node_id {
            return;
        }
        // Bind the claimed identity to the address the frame arrived from: a
        // peer configured at a different address must not be able to speak for
        // this one. Only the IP is compared, never the port — a peer dials
        // from an ephemeral source port, so the configured *listen* port can
        // never equal the remote port. This is defence in depth; the primary
        // binding is the edge secret verified below.
        if let Some(configured) = self.peers.iter().find(|peer| peer.node_id == env.node_id)
            && remote.ip() != configured.address.ip()
        {
            warn!(
                node_id = env.node_id,
                %remote,
                configured = %configured.address,
                "mesh frame rejected: sender address does not match the configured peer"
            );
            return;
        }
        // Only a configured peer may deliver frames, and every frame is
        // verified with the edge secret of the peer it claims to be. A valid
        // MAC proves possession of that edge secret, not the unique identity
        // of the process that produced it: both endpoints of an edge hold the
        // same secret, so peer attribution also relies on the source-address
        // binding above.
        let Some(edge_key) = self.trusted_keys.get(&env.node_id).cloned() else {
            warn!(
                node_id = env.node_id,
                "mesh frame rejected: sender is not a configured peer"
            );
            return;
        };
        if now_ms().abs_diff(env.ts_ms) > MAX_CLOCK_SKEW_MS
            || !verify(&edge_key, &payload, &env.mac)
        {
            warn!(
                node_id = env.node_id,
                "mesh frame rejected: authentication or timestamp invalid"
            );
            return;
        }
        if matches!(
            &env.body,
            WireMessage::Standard(MeshMessage::WalBatch { source_node_id, .. }) if *source_node_id != env.node_id
        ) {
            warn!(
                node_id = env.node_id,
                "mesh WAL batch source identity does not match authenticated envelope"
            );
            return;
        }
        if !valid_wire_message(&env.body, now_ms()) {
            warn!(
                node_id = env.node_id,
                "mesh frame rejected: message fields outside accepted bounds"
            );
            return;
        }
        let (message, recovery) = match env.body {
            WireMessage::Standard(message) => (message, None),
            WireMessage::Recovery(RecoveryWireMessage::Frame {
                generation,
                recovery_id,
                boundary_lsn,
                snapshot_digest,
                frame_index,
                frame_count,
                blocks,
                unblocks,
            }) => (
                MeshMessage::Sync { blocks, unblocks },
                Some(ReceivedRecovery::Frame {
                    generation,
                    recovery_id,
                    boundary_lsn,
                    snapshot_digest,
                    frame_index,
                    frame_count,
                }),
            ),
            WireMessage::Recovery(RecoveryWireMessage::Complete {
                generation,
                recovery_id,
                boundary_lsn,
                snapshot_digest,
                frame_count,
                tombstone_acks,
            }) => {
                if !valid_tombstone_acks(&tombstone_acks) {
                    warn!(
                        node_id = env.node_id,
                        "mesh frame rejected: tombstone acknowledgment map outside accepted bounds"
                    );
                    return;
                }
                // The reported epochs are authenticated by the sending peer,
                // but they are not merged into the local peer epoch table: a
                // peer must not be able to force premature tombstone
                // collection on this node.
                debug!(
                    node_id = env.node_id,
                    peers = tombstone_acks.len(),
                    "recovery completion carried peer tombstone acknowledgments"
                );
                (
                    MeshMessage::Sync {
                        blocks: Vec::new(),
                        unblocks: Vec::new(),
                    },
                    Some(ReceivedRecovery::Complete {
                        generation,
                        recovery_id,
                        boundary_lsn,
                        snapshot_digest,
                        frame_count,
                    }),
                )
            }
        };
        let (ack_tx, ack_rx) = oneshot::channel();
        let mut q = self.incoming.lock().await;
        let overflow = q.len() >= MAX_INCOMING_QUEUE;
        if overflow {
            // Dropping the displaced frame's sender explicitly rejects it; its
            // peer will retry instead of advancing a cursor over a lost event.
            q.pop_front();
        }
        q.push_back(IncomingFrame {
            sender_node_id: env.node_id,
            message,
            recovery,
            ack: ack_tx,
        });
        drop(q);
        if overflow {
            self.mark_all_sync(SyncCondition::QueueOverflow).await;
        }

        let ack_response = tokio::select! {
            result = tokio::time::timeout(MAX_APPLICATION_ACK_WAIT, ack_rx) => {
                result.ok().and_then(Result::ok).unwrap_or(AckResponse {
                    accepted: false,
                    ack_lsn: None,
                    recovery_proof: None,
                })
            }
            changed = shutdown.changed() => {
                let _ = changed;
                return;
            }
        };
        let ts_ms = now_ms();
        let ack_payload = match serde_json::to_vec(&(
            ts_ms,
            self.node_id,
            ack_response.accepted,
            ack_response.ack_lsn,
            &ack_response.recovery_proof,
        )) {
            Ok(payload) => payload,
            Err(_) => return,
        };
        // The application ACK travels back over the same edge, so it is
        // signed with the edge secret the accepted frame was verified under;
        // the sender verifies it with that same secret.
        let mac = match sign(&edge_key, &ack_payload) {
            Ok(mac) => mac,
            Err(_) => return,
        };
        let ack_frame = match serde_json::to_vec(&AckEnvelope {
            ts_ms,
            node_id: self.node_id,
            accepted: ack_response.accepted,
            ack_lsn: ack_response.ack_lsn,
            recovery_proof: ack_response.recovery_proof,
            mac,
        }) {
            Ok(frame) => frame,
            Err(_) => return,
        };
        if stream.write_all(&ack_frame).await.is_ok() {
            let _ = stream.write_all(b"\n").await;
        }
    }
}
/// Validate the public library boundary as well as the higher-level config.
fn validate_bind_args(node_id: u32, peers: &[MeshPeer], auth_key: &[u8]) -> std::io::Result<()> {
    let invalid = |message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message);
    if node_id == 0 {
        return Err(invalid("mesh node_id must be non-zero"));
    }
    // The local key is retained for configuration compatibility only: mesh
    // frames and application ACKs are signed and verified with the per-edge
    // secrets. It may be empty, but a value that is present must still be
    // long enough to be a key.
    if !auth_key.is_empty() && auth_key.len() < 16 {
        return Err(invalid(
            "mesh local authentication key must be empty or at least 16 bytes",
        ));
    }
    if peers.len() > MAX_CONFIGURED_PEERS {
        return Err(invalid("mesh peer list exceeds 256 peers"));
    }
    let mut node_ids = HashSet::new();
    let mut addresses = HashSet::new();
    let mut edge_keys = HashSet::new();
    for peer in peers {
        if peer.node_id == 0 || peer.node_id == node_id {
            return Err(invalid(
                "mesh peer node IDs must be non-zero and distinct from the local node ID",
            ));
        }
        if !node_ids.insert(peer.node_id) {
            return Err(invalid("mesh peer node IDs must be unique"));
        }
        if peer.address.port() == 0 || peer.address.ip().is_unspecified() {
            return Err(invalid(
                "mesh peers must have a concrete IP address and non-zero port",
            ));
        }
        if !addresses.insert(peer.address) {
            return Err(invalid("mesh peer addresses must be unique"));
        }
        if peer.auth_key.len() < 16 {
            return Err(invalid(
                "mesh edge authentication key must be at least 16 bytes",
            ));
        }
        // Two peers must not share an edge secret: traffic authenticated for
        // one edge would then also verify under the other peer's entry.
        if !edge_keys.insert(peer.auth_key.clone()) {
            return Err(invalid(
                "mesh edge authentication keys must be unique per peer",
            ));
        }
    }
    Ok(())
}

/// Reject authenticated-but-invalid CRDT fields before they can poison the HLC
/// or amplify the bounded incoming queue. Zero creation time is accepted for
/// compatibility with peers that predate the created_at_ms field.
fn valid_wire_message(message: &WireMessage, now_ms: u64) -> bool {
    match message {
        WireMessage::Standard(message) => valid_message(message, now_ms),
        WireMessage::Recovery(RecoveryWireMessage::Frame {
            generation,
            recovery_id,
            snapshot_digest,
            frame_index,
            frame_count,
            blocks,
            unblocks,
            ..
        }) => {
            *generation > 0
                && !recovery_id.is_nil()
                && valid_digest(snapshot_digest)
                && *frame_count > 0
                && (*frame_count as usize) <= MAX_RECOVERY_FRAMES
                && *frame_index < *frame_count
                && blocks.len().saturating_add(unblocks.len()) <= MAX_SYNC_FRAME_ENTRIES
                && blocks.iter().all(|d| {
                    !d.ip.is_unspecified()
                        && d.dot.node_id != 0
                        && d.dot.counter != 0
                        && !d.dot.incarnation.is_nil()
                        && (d.created_at_ms == 0
                            || d.created_at_ms <= now_ms.saturating_add(MAX_CLOCK_SKEW_MS))
                        && (d.expires_at_ms == u64::MAX || d.expires_at_ms >= d.created_at_ms)
                })
                && unblocks.iter().all(|d| {
                    !d.ip.is_unspecified()
                        && d.dot.node_id != 0
                        && d.dot.counter != 0
                        && !d.dot.incarnation.is_nil()
                })
        }
        WireMessage::Recovery(RecoveryWireMessage::Complete {
            generation,
            recovery_id,
            snapshot_digest,
            frame_count,
            ..
        }) => {
            *generation > 0
                && !recovery_id.is_nil()
                && valid_digest(snapshot_digest)
                && *frame_count > 0
                && (*frame_count as usize) <= MAX_RECOVERY_FRAMES
        }
    }
}

fn valid_message(message: &MeshMessage, now_ms: u64) -> bool {
    let latest_creation = now_ms.saturating_add(MAX_CLOCK_SKEW_MS);
    let valid_block = |d: &ClusterBlockDelta| {
        !d.ip.is_unspecified()
            && d.dot.node_id != 0
            && d.dot.counter != 0
            && !d.dot.incarnation.is_nil()
            && (d.created_at_ms == 0 || d.created_at_ms <= latest_creation)
            && (d.expires_at_ms == u64::MAX || d.expires_at_ms >= d.created_at_ms)
    };
    let valid_unblock = |d: &ClusterUnblockDelta| {
        !d.ip.is_unspecified()
            && d.dot.node_id != 0
            && d.dot.counter != 0
            && !d.dot.incarnation.is_nil()
    };
    match message {
        MeshMessage::Block(d) => valid_block(d),
        MeshMessage::Unblock(d) => valid_unblock(d),
        MeshMessage::Sync { blocks, unblocks } => {
            blocks.len().saturating_add(unblocks.len()) <= MAX_SYNC_FRAME_ENTRIES
                && blocks.iter().all(valid_block)
                && unblocks.iter().all(valid_unblock)
        }
        MeshMessage::WalBatch {
            source_node_id,
            start_lsn,
            end_lsn,
            mutations,
        } => {
            *source_node_id != 0
                && *start_lsn > 0
                && *end_lsn >= *start_lsn
                && mutations.len() <= MAX_SYNC_FRAME_ENTRIES
                && mutations
                    .windows(2)
                    .all(|pair| pair[0].source_lsn < pair[1].source_lsn)
                && mutations.iter().all(|m| {
                    m.source_lsn >= *start_lsn
                        && m.source_lsn <= *end_lsn
                        && m.origin_node_id != 0
                        && m.origin_counter > 0
                        && uuid::Uuid::parse_str(&m.origin_incarnation).is_ok_and(|id| !id.is_nil())
                        && !m.ip.is_unspecified()
                        && m.created_at_ms <= latest_creation
                        && match m.action {
                            MeshMutationKind::Block => m
                                .expires_at_ms
                                .is_none_or(|expiry| expiry >= m.created_at_ms),
                            MeshMutationKind::Unblock => m.expires_at_ms.is_none(),
                        }
                })
        }
    }
}

/// Build the tombstone acknowledgment list in canonical order.
///
/// The list is part of the recovery completion body that the MAC covers, so
/// its order must not depend on hash iteration order: two nodes that hold the
/// same acknowledgment epochs have to produce identical MAC-covered bytes.
fn canonical_tombstone_acks(
    epochs: impl IntoIterator<Item = (u32, PeerEpoch)>,
) -> Vec<TombstoneAck> {
    let mut acks: Vec<TombstoneAck> = epochs
        .into_iter()
        .map(|(peer_node_id, epoch)| TombstoneAck {
            peer_node_id,
            epoch,
        })
        .collect();
    acks.sort_by_key(|ack| ack.peer_node_id);
    acks
}

/// Bound the tombstone acknowledgment map carried by a recovery completion.
///
/// The map is authenticated by the sending peer but is never merged into the
/// local epoch table, so only its size and internal consistency are checked.
fn valid_tombstone_acks(acks: &[TombstoneAck]) -> bool {
    acks.len() <= MAX_CONFIGURED_PEERS
        && acks.iter().all(|ack| {
            ack.peer_node_id != 0 && ack.epoch.peer_node_id == ack.peer_node_id
        })
        // Canonical order: the acknowledgments are part of the MAC-covered
        // body, so the sender and the receiver must agree on the sequence.
        && acks
            .windows(2)
            .all(|pair| pair[0].peer_node_id < pair[1].peer_node_id)
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn canonical_snapshot(mut snapshot: AworsetSnapshot) -> Result<AworsetSnapshot, &'static str> {
    snapshot
        .blocks
        .sort_by_key(|d| (d.ip, d.dot.node_id, d.dot.incarnation, d.dot.counter));
    snapshot
        .unblocks
        .sort_by_key(|d| (d.ip, d.dot.node_id, d.dot.incarnation, d.dot.counter));
    snapshot.validate()?;
    Ok(snapshot)
}

fn canonical_snapshot_digest(snapshot: &AworsetSnapshot) -> std::io::Result<String> {
    let canonical = canonical_snapshot(snapshot.clone())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let encoded = serde_json::to_vec(&canonical).map_err(std::io::Error::other)?;
    let mut digest = Sha256::new();
    digest.update(b"ramshield/mesh-recovery-snapshot/v1\0");
    digest.update((encoded.len() as u64).to_be_bytes());
    digest.update(encoded);
    Ok(hex::encode(digest.finalize()))
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

async fn with_timeout<F, T>(timeout: Duration, future: F) -> std::io::Result<T>
where
    F: Future<Output = std::io::Result<T>>,
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
        AckEnvelope, Envelope, MAX_CONFIGURED_PEERS, MAX_FRAME, MAX_SYNC_FRAME_ENTRIES, MeshHandle,
        MeshMessage, MeshMutationKind, MeshPeer, PeerSendOutcome, RecoveryWireMessage,
        ReplicatedMeshMutation, TombstoneAck, WireMessage, canonical_tombstone_acks, chunk_sync,
        now_ms, push_frame_bytes, read_bounded_frame, read_bounded_frame_with_timeout, sign,
        valid_message, valid_tombstone_acks, validate_bind_args, verify, with_timeout,
    };
    use crate::aworset::{
        AworsetBlocklist, ClusterBlockDelta, ClusterDot, ClusterUnblockDelta, PeerEpoch,
    };
    use std::time::Duration;
    use std::{
        collections::HashMap,
        net::{IpAddr, Ipv6Addr, SocketAddr},
        sync::Arc,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::timeout;

    /// Per-edge shared secrets: both ends of an edge configure the identical
    /// secret, and a frame or application ACK on that edge is signed and
    /// verified with it.
    const EDGE_AB_KEY: &[u8] = b"mesh-test-edge-a-b-shared-secret-0001";
    const EDGE_BC_KEY: &[u8] = b"mesh-test-edge-b-c-shared-secret-0002";
    const EDGE_CA_KEY: &[u8] = b"mesh-test-edge-c-a-shared-secret-0003";
    /// A secret that no configured edge uses.
    const EDGE_MISMATCH_KEY: &[u8] = b"mesh-test-edge-mismatched-secret-04";
    /// The local key is retained for configuration compatibility and no longer
    /// signs mesh traffic.
    const NODE_A_KEY: &[u8] = b"mesh-test-node-a-auth-key-0001";
    const NODE_B_KEY: &[u8] = b"mesh-test-node-b-auth-key-0002";
    const NODE_C_KEY: &[u8] = b"mesh-test-node-c-auth-key-0003";

    /// An application ACK is only accepted when it comes from the peer the
    /// request was actually sent to: the claimed node id must be that peer,
    /// and the MAC must verify under the edge secret shared with that peer.
    ///
    /// Without that binding, any configured peer could answer on behalf of
    /// another one and let the sender advance state the intended peer never
    /// received.
    #[test]
    fn application_ack_is_bound_to_the_intended_peer() {
        /// Spawn a fake peer that answers one request with an ACK claiming
        /// `claimed_node_id`, signed with `signing_key`.
        async fn fake_peer(claimed_node_id: u32, signing_key: Vec<u8>) -> std::net::SocketAddr {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("fake peer");
            let address = listener.local_addr().expect("fake peer address");
            tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.expect("accept request");
                let mut request = Vec::new();
                let mut chunk = [0u8; 1024];
                while !request.contains(&b'\n') {
                    let read = stream.read(&mut chunk).await.expect("read request");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..read]);
                }
                let ack = AckEnvelope {
                    ts_ms: now_ms(),
                    node_id: claimed_node_id,
                    accepted: true,
                    ack_lsn: Some(7),
                    recovery_proof: None,
                    mac: String::new(),
                };
                let payload = serde_json::to_vec(&(
                    ack.ts_ms,
                    ack.node_id,
                    ack.accepted,
                    ack.ack_lsn,
                    &ack.recovery_proof,
                ))
                .expect("encode ack payload");
                let mac = sign(&signing_key, &payload).expect("sign ack");
                let mut frame =
                    serde_json::to_vec(&AckEnvelope { mac, ..ack }).expect("encode ack");
                frame.push(b'\n');
                let _ = stream.write_all(&frame).await;
            });
            address
        }

        /// Bind a mesh handle whose peer 2 lives at `peer_address` and whose
        /// peer 3 has its own distinct key.
        async fn handle_for(peer_address: std::net::SocketAddr) -> MeshHandle {
            MeshHandle::bind(
                1,
                Arc::new(AworsetBlocklist::new(1)),
                "127.0.0.1:0".parse().expect("listen address"),
                vec![
                    MeshPeer::new(2, peer_address, EDGE_AB_KEY.to_vec()),
                    MeshPeer::new(
                        3,
                        "127.0.0.1:2".parse().expect("peer address"),
                        EDGE_BC_KEY.to_vec(),
                    ),
                ],
                NODE_A_KEY.to_vec(),
            )
            .await
            .expect("bind mesh")
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let body = MeshMessage::Sync {
                blocks: Vec::new(),
                unblocks: Vec::new(),
            };

            // Positive control: the intended peer's own key is accepted.
            let honest = fake_peer(2, EDGE_AB_KEY.to_vec()).await;
            let handle = handle_for(honest).await;
            let acknowledged = handle
                .send_wal_batch(honest, body.clone())
                .await
                .expect("the intended peer's ACK must be accepted");
            assert_eq!(acknowledged, Some(7));
            handle.shutdown();

            // An ACK that claims the intended peer but carries a MAC made
            // with another configured peer's key must be rejected.
            let wrong_key = fake_peer(2, EDGE_BC_KEY.to_vec()).await;
            let handle = handle_for(wrong_key).await;
            let error = handle
                .send_wal_batch(wrong_key, body.clone())
                .await
                .expect_err("an ACK signed with another peer's key must be rejected");
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
            assert!(
                error.to_string().contains("authentication failed"),
                "unexpected error: {error}"
            );
            handle.shutdown();

            // An ACK that is correctly signed by another configured peer but
            // claims that peer's identity must be rejected as well: the ACK
            // has to come from the peer the request was sent to.
            let other_peer = fake_peer(3, EDGE_BC_KEY.to_vec()).await;
            let handle = handle_for(other_peer).await;
            let error = handle
                .send_wal_batch(other_peer, body)
                .await
                .expect_err("an ACK from another peer must be rejected");
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
            assert!(
                error.to_string().contains("authentication failed"),
                "unexpected error: {error}"
            );
            handle.shutdown();
        });
    }

    #[test]
    fn two_peer_condition_sync_converges_after_remote_merge() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let probe_a = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("reserve node A address");
            let addr_a = probe_a.local_addr().expect("node A address");
            drop(probe_a);
            let probe_b = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("reserve node B address");
            let addr_b = probe_b.local_addr().expect("node B address");
            drop(probe_b);

            let blocklist_a = Arc::new(AworsetBlocklist::new(1));
            let blocklist_b = Arc::new(AworsetBlocklist::new(2));
            let a = MeshHandle::bind(
                1,
                blocklist_a.clone(),
                addr_a,
                vec![MeshPeer::new(2, addr_b, EDGE_AB_KEY.to_vec())],
                NODE_A_KEY.to_vec(),
            )
            .await
            .expect("bind node A");
            let b = MeshHandle::bind(
                2,
                blocklist_b.clone(),
                addr_b,
                vec![MeshPeer::new(1, addr_a, EDGE_AB_KEY.to_vec())],
                NODE_B_KEY.to_vec(),
            )
            .await
            .expect("bind node B");

            let ip_a = IpAddr::V6(Ipv6Addr::from(0xabcdu128));
            let delta_a = blocklist_a.record_ban(ip_a, 60_000, 1);
            let sender = {
                let a = a.clone();
                tokio::spawn(async move { a.broadcast(MeshMessage::Block(delta_a)).await })
            };

            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    for received in b.drain_with_ack().await {
                        match received.message.clone() {
                            MeshMessage::Block(delta) => {
                                blocklist_b.merge_delta(&delta);
                            }
                            MeshMessage::Unblock(delta) => {
                                blocklist_b.merge_unblock_delta(&delta);
                            }
                            MeshMessage::Sync { blocks, unblocks } => {
                                for delta in blocks {
                                    blocklist_b.merge_delta(&delta);
                                }
                                for delta in unblocks {
                                    blocklist_b.merge_unblock_delta(&delta);
                                }
                            }
                            _ => {}
                        }
                        received.acknowledge(true);
                    }
                    if blocklist_b.is_blocked(&ip_a, now_ms()) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("node B should apply node A's state");
            let report = sender.await.expect("sender task");
            assert!(
                report.all_written(),
                "node A should receive an application ACK"
            );

            let ip_b = IpAddr::V6(Ipv6Addr::from(0xdef01234u128));
            blocklist_b.record_ban(ip_b, 60_000, 2);
            // The transport must be notified only after the service merges
            // incoming CRDT messages. This condition should schedule repair.
            b.notify_remote_state_applied().await;

            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    for received in a.drain_with_ack().await {
                        match received.message.clone() {
                            MeshMessage::Block(delta) => {
                                blocklist_a.merge_delta(&delta);
                            }
                            MeshMessage::Unblock(delta) => {
                                blocklist_a.merge_unblock_delta(&delta);
                            }
                            MeshMessage::Sync { blocks, unblocks } => {
                                for delta in blocks {
                                    blocklist_a.merge_delta(&delta);
                                }
                                for delta in unblocks {
                                    blocklist_a.merge_unblock_delta(&delta);
                                }
                            }
                            _ => {}
                        }
                        received.acknowledge(true);
                    }
                    if blocklist_a.is_blocked(&ip_b, now_ms()) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("condition-driven repair should converge node A with node B");

            let incarnation = uuid::Uuid::new_v4();
            let wal_batch = MeshMessage::WalBatch {
                source_node_id: 1,
                start_lsn: 10,
                end_lsn: 11,
                mutations: vec![ReplicatedMeshMutation {
                    source_lsn: 11,
                    origin_node_id: 1,
                    origin_incarnation: incarnation.to_string(),
                    origin_counter: 1,
                    ip: IpAddr::V6(Ipv6Addr::from(0xfeed1234u128)),
                    action: MeshMutationKind::Block,
                    created_at_ms: now_ms(),
                    expires_at_ms: None,
                    tier: 2,
                }],
            };
            let sender = {
                let a = a.clone();
                tokio::spawn(async move { a.send_wal_batch(addr_b, wal_batch).await })
            };
            let mut wal_ack_seen = false;
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    for received in b.drain_with_ack().await {
                        match received.message.clone() {
                            MeshMessage::WalBatch { end_lsn, .. } => {
                                received.acknowledge_with_lsn(true, Some(end_lsn));
                                wal_ack_seen = true;
                            }
                            _ => received.acknowledge(true),
                        }
                    }
                    if wal_ack_seen {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("receiver should acknowledge the WAL batch boundary");
            assert_eq!(
                sender.await.expect("WAL sender task").expect("WAL ACK"),
                Some(11)
            );

            let rejected = MeshMessage::WalBatch {
                source_node_id: 1,
                start_lsn: 12,
                end_lsn: 12,
                mutations: Vec::new(),
            };
            let rejected_sender = {
                let a = a.clone();
                tokio::spawn(async move { a.send_wal_batch(addr_b, rejected).await })
            };
            let mut rejection_seen = false;
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    for received in b.drain_with_ack().await {
                        match received.message.clone() {
                            MeshMessage::WalBatch { .. } => {
                                received.acknowledge(false);
                                rejection_seen = true;
                            }
                            _ => received.acknowledge(true),
                        }
                    }
                    if rejection_seen {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("receiver should reject the WAL batch");
            assert!(
                rejected_sender
                    .await
                    .expect("rejected WAL sender task")
                    .is_err(),
                "a negative application ACK must not advance the WAL cursor"
            );

            a.shutdown();
            b.shutdown();
        });
    }

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
            .map(|index| {
                MeshPeer::new(
                    u32::try_from(index + 1).expect("peer node id"),
                    "127.0.0.1:1234".parse().expect("peer address"),
                    vec![9u8; 32],
                )
            })
            .collect();
        let error = validate_bind_args(1, &peers, &valid_key).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

        let invalid_peer = [MeshPeer::new(
            2,
            "0.0.0.0:1234".parse().expect("unspecified peer"),
            vec![9u8; 32],
        )];
        let error = validate_bind_args(1, &invalid_peer, &valid_key).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn bind_rejects_duplicate_peer_identity_and_reused_edge_secrets() {
        let local_key = vec![7u8; 32];
        let edge_key = vec![9u8; 32];
        let address = "127.0.0.1:1234".parse().expect("peer address");

        // A peer may not claim the local node identity.
        let error = validate_bind_args(
            1,
            &[MeshPeer::new(1, address, edge_key.clone())],
            &local_key,
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

        // Peer node identities must be unique.
        let error = validate_bind_args(
            1,
            &[
                MeshPeer::new(2, address, edge_key.clone()),
                MeshPeer::new(
                    2,
                    "127.0.0.1:1235".parse().expect("peer address"),
                    vec![8u8; 32],
                ),
            ],
            &local_key,
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

        // Two peers must not share an edge secret: traffic authenticated for
        // one edge would then also verify under the other peer's entry.
        let error = validate_bind_args(
            1,
            &[
                MeshPeer::new(2, address, edge_key.clone()),
                MeshPeer::new(3, "127.0.0.1:1235".parse().expect("peer address"), edge_key),
            ],
            &local_key,
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

        // The local key is compatibility-only and signs nothing, so an empty
        // one is accepted and one that is present must still be long enough.
        validate_bind_args(1, &[MeshPeer::new(2, address, vec![9u8; 32])], &[])
            .expect("an empty local key must be accepted");
        let error = validate_bind_args(1, &[MeshPeer::new(2, address, vec![9u8; 32])], &[7u8; 15])
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
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
                vec![MeshPeer::new(2, unavailable_peer, vec![9u8; 32])],
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
            body: WireMessage::Standard(message),
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
            let error = with_timeout(
                Duration::from_millis(10),
                std::future::pending::<std::io::Result<()>>(),
            )
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

    #[test]
    fn full_sync_recovery_requires_receiver_digest_proof_before_cursor_release() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let probe = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("reserve receiver");
            let receiver_addr = probe.local_addr().expect("receiver address");
            drop(probe);
            let probe = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("reserve sender");
            let sender_addr = probe.local_addr().expect("sender address");
            drop(probe);
            let sender_state = Arc::new(AworsetBlocklist::new(1));
            let receiver_state = Arc::new(AworsetBlocklist::new(2));
            let ip = IpAddr::V6(Ipv6Addr::from(0xfeedbeefu128));
            sender_state.record_ban(ip, 60_000, 2);
            let sender = MeshHandle::bind(
                1,
                sender_state.clone(),
                sender_addr,
                vec![MeshPeer::new(2, receiver_addr, EDGE_AB_KEY.to_vec())],
                NODE_A_KEY.to_vec(),
            )
            .await
            .expect("bind sender");
            let receiver = MeshHandle::bind(
                2,
                receiver_state.clone(),
                receiver_addr,
                vec![MeshPeer::new(1, sender_addr, EDGE_AB_KEY.to_vec())],
                NODE_B_KEY.to_vec(),
            )
            .await
            .expect("bind receiver");
            sender
                .request_full_sync(receiver_addr, 17, sender_state.export_snapshot())
                .await;
            sender
                .request_full_sync(receiver_addr, 99, sender_state.export_snapshot())
                .await;
            assert!(sender.completed_recovery(receiver_addr).await.is_none());
            let completed = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    for received in receiver.drain_with_ack().await {
                        match received.message.clone() {
                            MeshMessage::Block(d) => {
                                receiver_state.merge_delta(&d);
                            }
                            MeshMessage::Unblock(d) => {
                                receiver_state.merge_unblock_delta(&d);
                            }
                            MeshMessage::Sync { blocks, unblocks } => {
                                for d in blocks {
                                    receiver_state.merge_delta(&d);
                                }
                                for d in unblocks {
                                    receiver_state.merge_unblock_delta(&d);
                                }
                            }
                            _ => {}
                        }
                        received.acknowledge(true);
                    }
                    if let Some(done) = sender.completed_recovery(receiver_addr).await {
                        break done;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("receiver confirms fixed snapshot");
            assert!(completed.0 > 0);
            assert_eq!(completed.1, 17);
            assert!(
                sender.recovery_completion_pending(receiver_addr).await,
                "proof must remain gated until cursor owner consumes boundary"
            );
            assert!(receiver_state.is_blocked(&ip, now_ms()));
            sender
                .acknowledge_recovery_cursor(receiver_addr, completed.0, completed.1)
                .await;
            assert!(!sender.recovery_completion_pending(receiver_addr).await);
            sender.shutdown();
            receiver.shutdown();
        });
    }

    /// A recovery completion body is covered by the frame MAC, and the
    /// receiver re-serializes the deserialized body before verifying it.
    /// The tombstone-ack map must therefore serialize deterministically:
    /// with a hash map the sender and the receiver can compute different
    /// MAC inputs and a valid completion is rejected. The map is also
    /// tamper-evident: changing one acknowledgment must break the MAC.
    #[test]
    fn recovery_completion_mac_is_stable_across_multiple_tombstone_acks() {
        let epoch = |peer_node_id: u32, epoch: u64| PeerEpoch {
            peer_node_id,
            incarnation: uuid::Uuid::from_u128(peer_node_id as u128),
            epoch,
            last_acknowledged_lsn: epoch * 10,
            tombstone_expiry_epoch: epoch + 1,
        };
        let acks = vec![
            TombstoneAck {
                peer_node_id: 3,
                epoch: epoch(3, 9),
            },
            TombstoneAck {
                peer_node_id: 7,
                epoch: epoch(7, 3),
            },
            TombstoneAck {
                peer_node_id: 11,
                epoch: epoch(11, 1),
            },
        ];
        let body = WireMessage::Recovery(RecoveryWireMessage::Complete {
            generation: 4,
            recovery_id: uuid::Uuid::from_u128(0xfeed),
            boundary_lsn: 128,
            snapshot_digest: "a".repeat(64),
            frame_count: 2,
            tombstone_acks: acks.clone(),
        });
        let ts_ms = now_ms();
        let node_id = 2u32;
        // The sender signs the canonical serialization of the body.
        let payload = serde_json::to_vec(&(ts_ms, node_id, &body)).expect("canonical payload");
        let mac = sign(EDGE_AB_KEY, &payload).expect("sign");
        let frame = serde_json::to_vec(&Envelope {
            ts_ms,
            node_id,
            body: body.clone(),
            mac,
        })
        .expect("frame");

        // The receiver parses the frame into fresh instances and re-serializes
        // the body for verification: the MAC must still verify.
        let envelope: Envelope = serde_json::from_slice(&frame).expect("parse envelope");
        let receiver_payload =
            serde_json::to_vec(&(envelope.ts_ms, envelope.node_id, &envelope.body))
                .expect("receiver payload");
        assert_eq!(
            receiver_payload, payload,
            "completion body must serialize identically on both sides"
        );
        assert!(
            verify(EDGE_AB_KEY, &receiver_payload, &envelope.mac),
            "a valid multi-entry completion must pass MAC verification"
        );

        // Tampering with a single acknowledgment must invalidate the MAC.
        let mut tampered = acks.clone();
        tampered[1].epoch = epoch(7, 4);
        assert!(
            valid_tombstone_acks(&tampered),
            "the tampered list is well formed; only the MAC may reject it"
        );
        let tampered_body = WireMessage::Recovery(RecoveryWireMessage::Complete {
            generation: 4,
            recovery_id: uuid::Uuid::from_u128(0xfeed),
            boundary_lsn: 128,
            snapshot_digest: "a".repeat(64),
            frame_count: 2,
            tombstone_acks: tampered,
        });
        let tampered_payload =
            serde_json::to_vec(&(envelope.ts_ms, envelope.node_id, &tampered_body))
                .expect("tampered payload");
        assert!(
            !verify(EDGE_AB_KEY, &tampered_payload, &envelope.mac),
            "a modified tombstone acknowledgment must fail MAC verification"
        );
    }

    #[test]
    fn recovery_completion_tombstone_acks_are_canonically_ordered() {
        let epoch = |peer_node_id: u32, epoch: u64| PeerEpoch {
            peer_node_id,
            incarnation: uuid::Uuid::from_u128(peer_node_id as u128),
            epoch,
            last_acknowledged_lsn: epoch * 10,
            tombstone_expiry_epoch: epoch + 1,
        };
        // The same logical acknowledgments inserted in different orders must
        // produce the same canonical list: the list is part of the MAC-covered
        // completion body, so hash iteration order must not reach the wire.
        let mut first: HashMap<u32, PeerEpoch> = HashMap::new();
        first.insert(11, epoch(11, 1));
        first.insert(3, epoch(3, 9));
        first.insert(7, epoch(7, 3));
        let mut second: HashMap<u32, PeerEpoch> = HashMap::new();
        second.insert(7, epoch(7, 3));
        second.insert(11, epoch(11, 1));
        second.insert(3, epoch(3, 9));

        let canonical_first = canonical_tombstone_acks(first);
        let canonical_second = canonical_tombstone_acks(second);
        assert_eq!(
            canonical_first, canonical_second,
            "acknowledgment order must not depend on hash iteration order"
        );
        assert_eq!(
            canonical_first
                .iter()
                .map(|ack| ack.peer_node_id)
                .collect::<Vec<_>>(),
            vec![3, 7, 11],
            "the MAC-covered list must be ordered by peer node id"
        );

        // Two nodes holding the same acknowledgments sign identical bytes.
        let completion = |acks: Vec<TombstoneAck>| {
            WireMessage::Recovery(RecoveryWireMessage::Complete {
                generation: 4,
                recovery_id: uuid::Uuid::from_u128(0xfeed),
                boundary_lsn: 128,
                snapshot_digest: "a".repeat(64),
                frame_count: 2,
                tombstone_acks: acks,
            })
        };
        assert_eq!(
            serde_json::to_vec(&completion(canonical_first)).expect("body a"),
            serde_json::to_vec(&completion(canonical_second)).expect("body b"),
            "canonical ordering must make the MAC-covered bytes identical"
        );
    }

    // --- F-01: per-edge HMAC secret acceptance and peer attribution ---

    /// Reserve a free loopback port and release it again.
    async fn free_addr() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("probe listener");
        let address = listener.local_addr().expect("probe address");
        drop(listener);
        address
    }

    /// Build the wire bytes of an envelope for `node_id`, signed with `key`.
    fn signed_frame(node_id: u32, key: &[u8], body: WireMessage) -> Vec<u8> {
        let ts_ms = now_ms();
        let payload = serde_json::to_vec(&(ts_ms, node_id, &body)).expect("canonical payload");
        let mac = sign(key, &payload).expect("sign frame");
        let mut frame = serde_json::to_vec(&Envelope {
            ts_ms,
            node_id,
            body,
            mac,
        })
        .expect("encode frame");
        frame.push(b'\n');
        frame
    }

    /// A well-formed but empty sync message.
    fn empty_sync() -> WireMessage {
        WireMessage::Standard(MeshMessage::Sync {
            blocks: Vec::new(),
            unblocks: Vec::new(),
        })
    }

    /// Wait for the receiver to queue exactly one message.
    async fn expect_one_queued(receiver: &MeshHandle) -> usize {
        timeout(Duration::from_secs(2), async {
            loop {
                let drained = receiver.drain_with_ack().await;
                if !drained.is_empty() {
                    return drained.len();
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the frame must reach the incoming queue")
    }

    /// Assert that a rejected frame never reaches the incoming queue.
    async fn expect_nothing_queued(receiver: &MeshHandle) {
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            receiver.drain().await.is_empty(),
            "a rejected frame must not enter the incoming queue"
        );
    }

    /// A configured peer authenticates even though it dials from an ephemeral
    /// source port: the source-address binding compares the IP only, and the
    /// edge secret is the primary protocol binding.
    #[tokio::test]
    async fn configured_peer_authenticates_from_an_ephemeral_source_port() {
        let receiver_addr = free_addr().await;
        let peer_addr = free_addr().await;
        let receiver = MeshHandle::bind(
            2,
            Arc::new(AworsetBlocklist::new(2)),
            receiver_addr,
            vec![MeshPeer::new(1, peer_addr, EDGE_AB_KEY.to_vec())],
            NODE_B_KEY.to_vec(),
        )
        .await
        .expect("bind receiver");

        let mut stream = TcpStream::connect(receiver_addr).await.expect("connect");
        let local = stream.local_addr().expect("local address");
        assert_ne!(
            local.port(),
            receiver_addr.port(),
            "the client must dial from an ephemeral source port"
        );
        stream
            .write_all(&signed_frame(1, EDGE_AB_KEY, empty_sync()))
            .await
            .expect("write frame");

        assert_eq!(expect_one_queued(&receiver).await, 1);
        receiver.shutdown();
    }

    /// A legitimate two-node exchange returns an application ACK that is
    /// authenticated with the edge secret of the edge it travelled on.
    #[tokio::test]
    async fn legitimate_two_node_exchange_returns_an_edge_authenticated_ack() {
        let receiver_addr = free_addr().await;
        let peer_addr = free_addr().await;
        let receiver = MeshHandle::bind(
            2,
            Arc::new(AworsetBlocklist::new(2)),
            receiver_addr,
            vec![MeshPeer::new(1, peer_addr, EDGE_AB_KEY.to_vec())],
            NODE_B_KEY.to_vec(),
        )
        .await
        .expect("bind receiver");

        let mut stream = TcpStream::connect(receiver_addr).await.expect("connect");
        stream
            .write_all(&signed_frame(1, EDGE_AB_KEY, empty_sync()))
            .await
            .expect("write frame");

        let received = timeout(Duration::from_secs(2), async {
            loop {
                let drained = receiver.drain_with_ack().await;
                if !drained.is_empty() {
                    break drained;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the frame must be accepted");
        assert_eq!(received.len(), 1);
        for frame in received {
            frame.acknowledge(true);
        }

        let ack_frame = read_bounded_frame_with_timeout(&mut stream, Duration::from_secs(2))
            .await
            .expect("read ack")
            .expect("ack frame");
        let ack_line = ack_frame.strip_suffix(b"\n").unwrap_or(&ack_frame);
        let ack: AckEnvelope = serde_json::from_slice(ack_line).expect("parse ack");
        assert!(ack.accepted, "the receiver must accept the frame");
        assert_eq!(ack.node_id, 2, "the ACK must claim the receiver identity");
        let ack_payload = serde_json::to_vec(&(
            ack.ts_ms,
            ack.node_id,
            ack.accepted,
            ack.ack_lsn,
            &ack.recovery_proof,
        ))
        .expect("ack payload");
        assert!(
            verify(EDGE_AB_KEY, &ack_payload, &ack.mac),
            "the ACK must be signed with the edge secret of the edge"
        );
        receiver.shutdown();
    }

    /// A frame whose source address is not the configured peer's address is
    /// rejected even when its MAC is valid: the source-address binding is
    /// defence in depth on top of the edge secret.
    #[tokio::test]
    async fn frame_from_an_unconfigured_source_address_is_rejected() {
        let receiver_addr = free_addr().await;
        // The peer is configured at an address the client cannot originate
        // from, so the source-address binding cannot be satisfied.
        let peer_addr: SocketAddr = "192.0.2.10:7900".parse().expect("peer address");
        let receiver = MeshHandle::bind(
            2,
            Arc::new(AworsetBlocklist::new(2)),
            receiver_addr,
            vec![MeshPeer::new(1, peer_addr, EDGE_AB_KEY.to_vec())],
            NODE_B_KEY.to_vec(),
        )
        .await
        .expect("bind receiver");

        let mut stream = TcpStream::connect(receiver_addr).await.expect("connect");
        stream
            .write_all(&signed_frame(1, EDGE_AB_KEY, empty_sync()))
            .await
            .expect("write frame");
        expect_nothing_queued(&receiver).await;
        receiver.shutdown();
    }

    /// Two peers configured on the same IP are still distinguished by their
    /// edge secret: a frame signed with one peer's secret is not accepted
    /// as the other peer.
    #[tokio::test]
    async fn peers_sharing_an_ip_are_not_confused() {
        let receiver_addr = free_addr().await;
        let peer_b_addr = free_addr().await;
        let peer_c_addr = free_addr().await;
        let receiver = MeshHandle::bind(
            2,
            Arc::new(AworsetBlocklist::new(2)),
            receiver_addr,
            vec![
                MeshPeer::new(1, peer_b_addr, EDGE_AB_KEY.to_vec()),
                MeshPeer::new(3, peer_c_addr, EDGE_BC_KEY.to_vec()),
            ],
            NODE_B_KEY.to_vec(),
        )
        .await
        .expect("bind receiver");

        // Peer 1's secret must not authenticate a frame claiming peer 3.
        let mut stream = TcpStream::connect(receiver_addr).await.expect("connect");
        stream
            .write_all(&signed_frame(3, EDGE_AB_KEY, empty_sync()))
            .await
            .expect("write frame");
        expect_nothing_queued(&receiver).await;

        // Peer 3's own secret still authenticates its own identity.
        let mut stream = TcpStream::connect(receiver_addr).await.expect("connect");
        stream
            .write_all(&signed_frame(3, EDGE_BC_KEY, empty_sync()))
            .await
            .expect("write frame");
        assert_eq!(expect_one_queued(&receiver).await, 1);
        receiver.shutdown();
    }

    /// A peer that legitimately holds its own edge secret cannot make a
    /// frame accepted as another configured peer: the claimed identity
    /// selects the edge secret the MAC must verify under.
    #[tokio::test]
    async fn frame_claiming_another_peer_identity_is_rejected() {
        let receiver_addr = free_addr().await;
        let peer_addr = free_addr().await;
        let other_addr = free_addr().await;
        let receiver = MeshHandle::bind(
            2,
            Arc::new(AworsetBlocklist::new(2)),
            receiver_addr,
            vec![
                MeshPeer::new(1, peer_addr, EDGE_AB_KEY.to_vec()),
                MeshPeer::new(3, other_addr, EDGE_BC_KEY.to_vec()),
            ],
            NODE_B_KEY.to_vec(),
        )
        .await
        .expect("bind receiver");

        // The client holds only EDGE_AB_KEY but claims to be peer 3.
        let mut stream = TcpStream::connect(receiver_addr).await.expect("connect");
        stream
            .write_all(&signed_frame(3, EDGE_AB_KEY, empty_sync()))
            .await
            .expect("write frame");
        expect_nothing_queued(&receiver).await;
        receiver.shutdown();
    }

    /// Two nodes that provision different secrets for the same edge cannot
    /// authenticate each other: the frame never reaches the queue.
    #[tokio::test]
    async fn nodes_with_mismatched_edge_secrets_cannot_authenticate() {
        let receiver_addr = free_addr().await;
        let peer_addr = free_addr().await;
        let receiver = MeshHandle::bind(
            2,
            Arc::new(AworsetBlocklist::new(2)),
            receiver_addr,
            vec![MeshPeer::new(1, peer_addr, EDGE_AB_KEY.to_vec())],
            NODE_B_KEY.to_vec(),
        )
        .await
        .expect("bind receiver");

        // The sender provisioned a different secret for the same edge.
        let mut stream = TcpStream::connect(receiver_addr).await.expect("connect");
        stream
            .write_all(&signed_frame(1, EDGE_MISMATCH_KEY, empty_sync()))
            .await
            .expect("write frame");
        expect_nothing_queued(&receiver).await;
        receiver.shutdown();
    }

    /// With three nodes, the secret one node shares with another does not
    /// authenticate a frame in a third node's peer context: the receiver
    /// verifies under the secret it shares with the claimed peer.
    #[tokio::test]
    async fn an_edge_secret_does_not_authenticate_in_another_peers_context() {
        let receiver_addr = free_addr().await;
        let peer_b_addr = free_addr().await;
        let peer_a_addr = free_addr().await;
        // The receiver is node C: it knows the edge secret it shares with B
        // and the one it shares with A, and nothing else.
        let receiver = MeshHandle::bind(
            3,
            Arc::new(AworsetBlocklist::new(3)),
            receiver_addr,
            vec![
                MeshPeer::new(2, peer_b_addr, EDGE_BC_KEY.to_vec()),
                MeshPeer::new(1, peer_a_addr, EDGE_CA_KEY.to_vec()),
            ],
            NODE_C_KEY.to_vec(),
        )
        .await
        .expect("bind receiver");

        // A frame claiming to be B, signed with the secret A shares with C.
        let mut stream = TcpStream::connect(receiver_addr).await.expect("connect");
        stream
            .write_all(&signed_frame(2, EDGE_CA_KEY, empty_sync()))
            .await
            .expect("write frame");
        expect_nothing_queued(&receiver).await;
        receiver.shutdown();
    }
}
