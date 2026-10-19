use crate::aworset::{AworsetBlocklist, AworsetSnapshot, ClusterBlockDelta, ClusterUnblockDelta};
use crate::condition_sync::{ConditionSyncPlanner, SyncCondition, SyncPolicy};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::aworset::PeerEpoch;
use std::{
    collections::{HashMap, VecDeque},
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
        /// Maps peer node IDs to their acknowledgment epochs for tombstones.
        tombstone_acks: HashMap<u32, PeerEpoch>,
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
    peers: Arc<Vec<SocketAddr>>,
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
        peers: Vec<SocketAddr>,
        auth_key: Vec<u8>,
    ) -> std::io::Result<Self> {
        validate_bind_args(node_id, &peers, &auth_key)?;
        let sync_states = peers
            .iter()
            .copied()
            .map(|peer| {
                ConditionSyncPlanner::new(SyncPolicy::default())
                    .map(|planner| {
                        (
                            peer,
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
        self.peers.as_ref().clone()
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

        for peer in self.peers.iter().copied() {
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
                        if success {
                            if let Some(ctx) = recovery {
                                state.completed_recovery = Some((ctx.generation, ctx.boundary_lsn));
                                state.recovery_boundary_lsn = None;
                                state.recovery_snapshot = None;
                                state.recovery_id = None;
                            }
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
                    })).await
                }) => match result {
                    Ok(Ok(response)) => response,
                    Ok(Err(error)) => { warn!(peer = %peer, error = %error, "recovery completion rejected"); drop(permit); return (false, None); }
                    Err(error) => { warn!(peer = %peer, error = %error, "recovery completion timed out"); drop(permit); return (false, None); }
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
        let body = body.into();
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
        if ack.node_id == self.node_id
            || now_ms().abs_diff(ack.ts_ms) > MAX_CLOCK_SKEW_MS
            || !verify(&self.auth_key, &ack_payload, &ack.mac)
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
        if now_ms().abs_diff(env.ts_ms) > MAX_CLOCK_SKEW_MS
            || !verify(&self.auth_key, &payload, &env.mac)
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
            }) => (
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
            ),
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
        let mac = match sign(&self.auth_key, &ack_payload) {
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
fn validate_bind_args(node_id: u32, peers: &[SocketAddr], auth_key: &[u8]) -> std::io::Result<()> {
    let invalid = |message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message);
    if node_id == 0 {
        return Err(invalid("mesh node_id must be non-zero"));
    }
    if auth_key.len() < 16 {
        return Err(invalid("mesh authentication key must be at least 16 bytes"));
    }
    if peers.len() > MAX_CONFIGURED_PEERS {
        return Err(invalid("mesh peer list exceeds 256 peers"));
    }
    if peers
        .iter()
        .any(|peer| peer.port() == 0 || peer.ip().is_unspecified())
    {
        return Err(invalid(
            "mesh peers must have a concrete IP address and non-zero port",
        ));
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
                    d.dot.node_id != 0
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
        d.dot.node_id != 0
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
        MeshMutationKind, PeerSendOutcome, ReplicatedMeshMutation, chunk_sync, now_ms,
        push_frame_bytes, read_bounded_frame, read_bounded_frame_with_timeout, valid_message,
        validate_bind_args, with_timeout,
    };
    use crate::aworset::{AworsetBlocklist, ClusterBlockDelta, ClusterDot, ClusterUnblockDelta};
    use std::time::Duration;
    use std::{
        net::{IpAddr, Ipv6Addr, SocketAddr},
        sync::Arc,
    };
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

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
            let a = MeshHandle::bind(1, blocklist_a.clone(), addr_a, vec![addr_b], vec![7; 32])
                .await
                .expect("bind node A");
            let b = MeshHandle::bind(2, blocklist_b.clone(), addr_b, vec![addr_a], vec![7; 32])
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

        let peers: Vec<SocketAddr> = (0..=MAX_CONFIGURED_PEERS)
            .map(|_| "127.0.0.1:1234".parse().expect("peer address"))
            .collect();
        let error = validate_bind_args(1, &peers, &valid_key).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

        let invalid_peer = ["0.0.0.0:1234".parse().expect("unspecified peer")];
        let error = validate_bind_args(1, &invalid_peer, &valid_key).unwrap_err();
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
                vec![unavailable_peer],
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
            let sender_state = Arc::new(AworsetBlocklist::new(1));
            let receiver_state = Arc::new(AworsetBlocklist::new(2));
            let ip = IpAddr::V6(Ipv6Addr::from(0xfeedbeefu128));
            sender_state.record_ban(ip, 60_000, 2);
            let sender = MeshHandle::bind(
                1,
                sender_state.clone(),
                "127.0.0.1:0".parse().unwrap(),
                vec![receiver_addr],
                vec![7; 32],
            )
            .await
            .expect("bind sender");
            let receiver = MeshHandle::bind(
                2,
                receiver_state.clone(),
                receiver_addr,
                Vec::new(),
                vec![7; 32],
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
}
