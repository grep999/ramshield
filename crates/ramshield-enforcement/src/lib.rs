//! Enforcement service: the single writer for security state and dataplane changes.
//!
//! All block/unblock requests are serialized through this actor. Callers never
//! mutate Store block state directly. TTL expiry is also converted into an
//! internal unblock command so the same state transition path is used.
//!
//! Durability (crate port): when a WAL is attached, every Block/Unblock is
//! appended to the WAL BEFORE the storage mutation, and the returned LSN is
//! set on `EnforceResult.wal_lsn`. Order: WAL → storage → TTL schedule → XDP.

use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use anyhow::Result;
use ramshield_metrics::Metrics;
use ramshield_storage::{
    BlockState, IpRecord, Store, Value,
    checkpoint_shared::{CheckpointShared, CidrSnapshot, SharedState},
    wal::{Wal, WalEntry},
};
use ramshield_types::{
    BlockReason, EnforceAction, EnforceCommand, EnforceResult, EnforcementError, IpNetwork,
};
use std::collections::{BTreeMap, VecDeque};
use std::net::IpAddr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

/// Fallback expiry horizon when TTL arithmetic overflows (belt-and-suspenders;
/// all entry points clamp, so this should never fire). 24h keeps a runaway
/// block bounded instead of permanent.
const MAX_EXPIRY_FALLBACK_SECS: u64 = 86_400;

#[cfg(feature = "xdp")]
pub mod xdp;

mod applier;
mod replay;
mod service;

pub use applier::{ReconciliationState, StubXdpApplier, XdpApplier, XdpDropEvent};
pub use replay::{
    replay_wal_cidrs, replay_wal_cidrs_from, replay_wal_cidrs_seeded, replay_wal_into_store,
    replay_wal_into_store_seeded,
};

/// Sole writer. The command queue is bounded by the engine and this actor is
/// the only component permitted to mutate BlockState or the XDP dataplane.
pub struct EnforcementService {
    store: Arc<Store>,
    metrics: Arc<Metrics>,
    xdp: Box<dyn XdpApplier>,
    /// Optional durability: append-before-mutate. None = in-memory only.
    wal: Option<Arc<Wal>>,
    /// Idempotency cache: decision_id → ORIGINAL EnforceResult (bounded to
    /// 65_536 by processed_order). A replayed decision_id returns what
    /// happened the first time — including a dataplane failure — instead of
    /// a fabricated fresh success.
    processed_results: HashMap<Uuid, EnforceResult>,
    processed_order: VecDeque<Uuid>,
    blocked_ips: HashSet<IpAddr>,
    /// Per-IP attributed XDP drops since block, keyed to userspace-blocked
    /// IPs ONLY (bounded by |blocked_ips|; cleared on unblock / re-block).
    /// Observability basis: audit counters + zero-drop gauge. Invariant:
    /// this map never feeds detection or the forecaster — a drop is a
    /// consequence of our own block, not independent threat evidence.
    drops_by_blocked: HashMap<IpAddr, u64>,
    /// Userspace mirror of active CIDR blocks lives in `store.active_cidrs`
    /// (single owner = this actor, single reader path = check_ip/dashboard).
    /// ponytail: kernel BLOCKCIDR maps are the authoritative dataplane; this
    /// mirror gives cheap telemetry without per-tick map iteration — upgrade
    /// to LpmTrie::iter if exact kernel entry counts are ever required.
    /// TTL expiry index: IP -> (second-bucket, position in that bucket's Vec).
    /// RAM-for-CPU item 14: was a flat HashMap swept with `retain()` every
    /// 250ms — O(all pending expirations) per tick to usually find zero due.
    /// Now `expire_due` drains only buckets whose second has passed: O(due).
    /// The (bucket,pos) pair makes re-block (TTL refresh) O(1): detach via
    /// swap-remove from the old bucket, attach to the new — no stale cards,
    /// so a refresh storm (unconditional block emissions every flush) cannot
    /// accumulate garbage the way a lazy-generation ring would. BinaryHeap
    /// was rejected earlier (rebuild on re-block); this is the bucketed-PQ
    /// trick with position tracking instead.
    /// Resolution is one second (bucket fires at the first whole second at or
    /// after the deadline — never early, at most ~1s late). ponytail: if
    /// sub-second TTL precision ever matters, switch buckets to a ms-grained
    /// ring over a fixed horizon.
    expirations: HashMap<IpAddr, (u64, usize)>,
    cidr_expirations: HashMap<IpNetwork, Instant>,
    buckets: BTreeMap<u64, Vec<IpAddr>>,
    epoch: Instant,
    shutdown: Arc<AtomicBool>,
    /// Last committed WAL LSN (updated on every enforce() call). None = no WAL.
    last_wal_lsn: Option<u64>,
    /// P2: Cluster blocklist CRDT — companion to `blocked_ips` local mirror.
    /// Local blocks are authoritative; this CRDT absorbs peer deltas and
    /// merges them on the next enforcement tick. None = single-node.
    #[cfg(feature = "mesh")]
    mesh_blocklist: Option<Arc<ramshield_mesh::aworset::AworsetBlocklist>>,
    #[cfg(feature = "mesh")]
    mesh_handle: Option<ramshield_mesh::MeshHandle>,
    #[cfg(feature = "mesh")]
    mesh_applied_ips: HashSet<IpAddr>,
    #[cfg(feature = "mesh")]
    mesh_operator_suppressions: HashSet<IpAddr>,
    /// Checkpoint coordination: barrier + absolute-deadline mirror. None =
    /// engine never attached one (standalone use, tests).
    checkpoint_shared: Option<Arc<CheckpointShared>>,
}

impl EnforcementService {
    pub fn new(
        store: Arc<Store>,
        metrics: Arc<Metrics>,
        xdp: Box<dyn XdpApplier>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self {
            store,
            metrics,
            xdp,
            wal: None,
            processed_results: HashMap::new(),
            processed_order: VecDeque::with_capacity(65_536),
            blocked_ips: HashSet::new(),
            drops_by_blocked: HashMap::new(),
            expirations: HashMap::new(),
            cidr_expirations: HashMap::new(),
            buckets: BTreeMap::new(),
            epoch: Instant::now(),
            shutdown,
            last_wal_lsn: None,
            #[cfg(feature = "mesh")]
            mesh_blocklist: None,
            #[cfg(feature = "mesh")]
            mesh_handle: None,
            #[cfg(feature = "mesh")]
            mesh_applied_ips: HashSet::new(),
            #[cfg(feature = "mesh")]
            mesh_operator_suppressions: HashSet::new(),
            checkpoint_shared: None,
        }
    }

    /// Attach checkpoint coordination (engine calls this during boot).
    pub fn with_checkpoint_shared(mut self, shared: Arc<CheckpointShared>) -> Self {
        self.checkpoint_shared = Some(shared);
        self
    }

    /// Enable cluster CRDT companion (fleet gossip mesh).
    #[cfg(feature = "mesh")]
    pub fn with_mesh_blocklist(
        mut self,
        mesh_blocklist: Arc<ramshield_mesh::aworset::AworsetBlocklist>,
    ) -> Self {
        self.mesh_blocklist = Some(mesh_blocklist);
        self
    }

    /// Attach the authenticated mesh transport.
    #[cfg(feature = "mesh")]
    pub fn with_mesh_handle(mut self, mesh_handle: ramshield_mesh::MeshHandle) -> Self {
        self.mesh_handle = Some(mesh_handle);
        self
    }

    /// Attach WAL for durable enforcement (append-before-mutate ordering).
    pub fn with_wal(mut self, wal: Arc<Wal>) -> Self {
        self.wal = Some(wal);
        self
    }
}

fn reason_to_block_reason(reason: &str) -> BlockReason {
    match BlockReason::from_reason_str(&reason.to_ascii_lowercase()) {
        Some(r) => r,
        None => {
            tracing::warn!(
                reason = %reason,
                "unknown block reason string; defaulting to ManualBlock"
            );
            BlockReason::ManualBlock
        }
    }
}

/// Convert a monotonic `Instant` to an absolute Unix-ns timestamp.
/// `now_unix_ns` = wall clock captured at the same moment `Instant::now()`
/// would be taken. Derivation: wall_target = wall_now - (monotonic_now - at).
/// Monotonic clocks can never go backwards, so `monotonic_now >= at` when
/// `at` is in the past; for future deadlines the subtraction underflows and
/// saturates — the sign is recovered below.
fn unix_ns_from_instant(at: Instant, now_unix_ns: u64) -> u64 {
    let now_mono = Instant::now();
    if at >= now_mono {
        // Future deadline: add the remaining monotonic duration to wall now.
        let ahead_ns = at.duration_since(now_mono).as_nanos() as u64;
        now_unix_ns.saturating_add(ahead_ns)
    } else {
        // Past deadline (rare here): subtract elapsed.
        let behind_ns = now_mono.duration_since(at).as_nanos() as u64;
        now_unix_ns.saturating_sub(behind_ns)
    }
}

fn epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
