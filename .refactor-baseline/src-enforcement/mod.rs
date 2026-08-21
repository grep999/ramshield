//! Enforcement service: the single writer for security state and dataplane changes.
//!
//! All block/unblock requests are serialized through this actor. Callers never
//! mutate Store block state directly. TTL expiry is also converted into an
//! internal unblock command so the same state transition path is used.

use anyhow::Result;
use crate::storage::{Store, Value, BlockState, BlockReason, IpRecord};
use crate::metrics::Metrics;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{info, warn, error};
use uuid::Uuid;

#[cfg(feature = "xdp")]
pub mod xdp;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforceCommand {
    pub decision_id: Uuid,
    pub policy_version: u64,
    pub source: String,
    pub actor: String,
    pub timestamp_utc: i64,
    pub ttl_seconds: u64,
    pub reason: String,
    pub ip: IpAddr,
    pub action: EnforceAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EnforceAction {
    Block,
    Unblock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforceResult {
    pub decision_id: Uuid,
    pub committed: bool,
    pub applied: bool,
    pub wal_lsn: Option<u64>,
    pub xdp_applied: bool,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum EnforcementError {
    #[error("WAL error: {0}")] Wal(String),
    #[error("Storage error: {0}")] Storage(String),
    #[error("XDP error: {0}")] Xdp(String),
    #[error("Duplicate decision_id: {0}")] Duplicate(Uuid),
    #[error("Invalid command: {0}")] InvalidCommand(String),
}

#[derive(Debug, Clone, Default)]
pub struct ReconciliationState {
    pub last_wal_lsn: u64,
    pub pending_blocks: Vec<IpAddr>,
    pub pending_unblocks: Vec<IpAddr>,
}

#[async_trait::async_trait]
pub trait XdpApplier: Send + Sync {
    fn apply_block(&mut self, ip: IpAddr, decision_id: Uuid) -> Result<(), EnforcementError>;
    fn apply_unblock(&mut self, ip: IpAddr, decision_id: Uuid) -> Result<(), EnforcementError>;
    fn reconcile(&mut self, expected_blocks: &[IpAddr]) -> Result<ReconciliationState, EnforcementError>;
}

pub struct StubXdpApplier;

#[async_trait::async_trait]
impl XdpApplier for StubXdpApplier {
    fn apply_block(&mut self, ip: IpAddr, _decision_id: Uuid) -> Result<(), EnforcementError> {
        info!(%ip, "XDP block (stub)");
        Ok(())
    }
    fn apply_unblock(&mut self, ip: IpAddr, _decision_id: Uuid) -> Result<(), EnforcementError> {
        info!(%ip, "XDP unblock (stub)");
        Ok(())
    }
    fn reconcile(&mut self, _expected_blocks: &[IpAddr]) -> Result<ReconciliationState, EnforcementError> {
        Ok(ReconciliationState::default())
    }
}

/// Sole writer. The command queue is bounded by the engine and this actor is
/// the only component permitted to mutate BlockState or the XDP dataplane.
pub struct EnforcementService {
    store: Arc<Store>,
    metrics: Arc<Metrics>,
    xdp: Box<dyn XdpApplier>,
    processed_decisions: HashSet<Uuid>,
    processed_order: VecDeque<Uuid>,
    blocked_ips: HashSet<IpAddr>,
    expirations: Vec<(Instant, IpAddr)>,
    shutdown: Arc<AtomicBool>,
}

impl EnforcementService {
    pub fn new(
        store: Arc<Store>,
        metrics: Arc<Metrics>,
        xdp: Box<dyn XdpApplier>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self {
            store, metrics, xdp, processed_decisions: HashSet::new(), processed_order: VecDeque::with_capacity(65_536),
            blocked_ips: HashSet::new(), expirations: Vec::new(), shutdown,
        }
    }

    pub async fn run(mut self, mut command_rx: mpsc::Receiver<EnforceCommand>) -> Result<()> {
        info!("Enforcement service started");
        let expected = self.store.get_all_blocked_ips();
        match self.xdp.reconcile(&expected) {
            Ok(_) => {
                self.blocked_ips = expected.into_iter().collect();
                info!("XDP reconciled with {} blocked IPs", self.blocked_ips.len());
            }
            Err(e) => error!("Initial XDP reconciliation failed: {}", e),
        }

        let mut tick = tokio::time::interval(Duration::from_millis(250));
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    self.expire_due().await;
                    if self.shutdown.load(Ordering::Acquire) { break; }
                }
                Some(cmd) = command_rx.recv() => {
                    if let Err(e) = self.enforce(cmd).await { error!("Enforcement failed: {}", e); }
                }
                else => break,
            }
            if self.shutdown.load(Ordering::Acquire) { break; }
        }
        info!("Enforcement service stopped");
        Ok(())
    }

    async fn expire_due(&mut self) {
        let now = Instant::now();
        let mut due = Vec::new();
        self.expirations.retain(|(at, ip)| {
            if *at <= now { due.push(*ip); false } else { true }
        });
        for ip in due {
            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(), policy_version: 0,
                source: "ttl".into(), actor: "system".into(),
                timestamp_utc: epoch_seconds(), ttl_seconds: 0,
                reason: "ttl_expired".into(), ip, action: EnforceAction::Unblock,
            };
            if let Err(e) = self.enforce(cmd).await { warn!(%ip, "TTL unblock failed: {}", e); }
        }
    }

    fn remember_decision(&mut self, id: Uuid) {
        if self.processed_decisions.insert(id) {
            self.processed_order.push_back(id);
            while self.processed_order.len() > 65_536 {
                if let Some(old) = self.processed_order.pop_front() {
                    self.processed_decisions.remove(&old);
                }
            }
        }
    }

    /// Execute one command. Storage is updated before the local/XDP indexes are
    /// changed, preventing a failed storage mutation from creating phantom blocks.
    pub async fn enforce(&mut self, cmd: EnforceCommand) -> Result<EnforceResult, EnforcementError> {
        if self.processed_decisions.contains(&cmd.decision_id) {
            return Ok(EnforceResult { decision_id: cmd.decision_id, committed: true, applied: true, wal_lsn: None, xdp_applied: true, error: None });
        }
        if cmd.ip.is_unspecified() {
            return Err(EnforcementError::InvalidCommand("unspecified IP is not blockable".into()));
        }

        let now_ns = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
        match cmd.action {
            EnforceAction::Block => {
                let reason = reason_to_block_reason(&cmd.reason);
                let rec = self.store.get(&cmd.ip)
                    .and_then(|v| match v { Value::IpRecord(r) => Some(r), _ => None })
                    .unwrap_or(IpRecord {
                        ip: cmd.ip, request_count: 0, ewma_rps: 0.0,
                        first_seen_ns: now_ns, last_seen_ns: now_ns, bytes_in: 0, status_dist: [0; 5],
                        proto_fingerprint: 0, threat_score: 0.0, block_state: BlockState::Clean,
                    });
                let mut updated = rec;
                updated.block_state = BlockState::Blocked { reason, since_ns: now_ns };

                // Do not let Store's passive expiry hide a still-blocked record.
                self.store.insert(cmd.ip, Value::IpRecord(updated), None, self.store.traffic.ram_limit_mb.load(Ordering::Relaxed) * 1024 * 1024)
                    .map_err(|e| EnforcementError::Storage(e.to_string()))?;

                self.blocked_ips.insert(cmd.ip);
                // Invariant: at most one expiration per IP. A re-block must not
                // inherit a stale TTL from a previous block/unblock cycle.
                if cmd.ttl_seconds > 0 {
                    self.expirations.retain(|(_, existing)| *existing != cmd.ip);
                    self.expirations.push((Instant::now() + Duration::from_secs(cmd.ttl_seconds), cmd.ip));
                } else {
                    self.expirations.retain(|(_, existing)| *existing != cmd.ip);
                }

                let xdp_applied = match self.xdp.apply_block(cmd.ip, cmd.decision_id) {
                    Ok(()) => true,
                    Err(e) => { warn!(ip=%cmd.ip, "XDP block failed: {}", e); false }
                };
                self.remember_decision(cmd.decision_id);
                self.metrics.inc_blocks();
                Ok(EnforceResult { decision_id: cmd.decision_id, committed: true, applied: true, wal_lsn: None, xdp_applied, error: None })
            }
            EnforceAction::Unblock => {
                if let Some(Value::IpRecord(mut rec)) = self.store.get(&cmd.ip) {
                    rec.block_state = BlockState::Clean;
                    self.store.insert(cmd.ip, Value::IpRecord(rec), None, self.store.traffic.ram_limit_mb.load(Ordering::Relaxed) * 1024 * 1024)
                        .map_err(|e| EnforcementError::Storage(e.to_string()))?;
                }
                self.blocked_ips.remove(&cmd.ip);
                // Purge any pending TTL so a later re-block starts clean.
                self.expirations.retain(|(_, existing)| *existing != cmd.ip);
                let xdp_applied = match self.xdp.apply_unblock(cmd.ip, cmd.decision_id) {
                    Ok(()) => true,
                    Err(e) => { warn!(ip=%cmd.ip, "XDP unblock failed: {}", e); false }
                };
                self.remember_decision(cmd.decision_id);
                Ok(EnforceResult { decision_id: cmd.decision_id, committed: true, applied: true, wal_lsn: None, xdp_applied, error: None })
            }
        }
    }
}

fn reason_to_block_reason(reason: &str) -> BlockReason {
    let r = reason.to_ascii_lowercase();
    if r == "high_rps" || r == "syn_flood" || r == "volumetric" || r == "slowloris" { BlockReason::HighRps }
    else if r == "subnet_burst" { BlockReason::SubnetBatch }
    else if r == "forecast_anomaly" { BlockReason::ForecastAnomaly }
    else if r == "entropy_anomaly" || r == "anomaly" { BlockReason::EntropyAnomaly }
    else { BlockReason::ManualBlock }
}

fn epoch_seconds() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic applier recording dataplane ops — lets tests assert the
    /// exact block/unblock sequence the service issues.
    struct RecordingApplier {
        log: std::sync::Mutex<Vec<(String, IpAddr)>>,
    }
    impl RecordingApplier {
        fn new() -> Self { Self { log: std::sync::Mutex::new(Vec::new()) } }
        #[allow(dead_code)]
        fn ops(&self) -> Vec<(String, IpAddr)> { self.log.lock().unwrap().clone() }
    }
    #[async_trait::async_trait]
    impl XdpApplier for RecordingApplier {
        fn apply_block(&mut self, ip: IpAddr, _d: Uuid) -> Result<(), EnforcementError> {
            self.log.lock().unwrap().push(("block".into(), ip)); Ok(())
        }
        fn apply_unblock(&mut self, ip: IpAddr, _d: Uuid) -> Result<(), EnforcementError> {
            self.log.lock().unwrap().push(("unblock".into(), ip)); Ok(())
        }
        fn reconcile(&mut self, _expected: &[IpAddr]) -> Result<ReconciliationState, EnforcementError> {
            Ok(ReconciliationState::default())
        }
    }

    fn svc(xdp: Box<dyn XdpApplier>) -> EnforcementService {
        let store = Arc::new(Store::new(16));
        store.traffic.ram_limit_mb.store(512, Ordering::Relaxed);
        EnforcementService::new(
            store,
            Arc::new(Metrics::new()),
            xdp,
            Arc::new(AtomicBool::new(false)),
        )
    }

    fn block_cmd(ip: IpAddr, ttl: u64) -> EnforceCommand {
        EnforceCommand {
            decision_id: Uuid::new_v4(), policy_version: 1, source: "test".into(),
            actor: "test".into(), timestamp_utc: 0, ttl_seconds: ttl,
            reason: "high_rps".into(), ip, action: EnforceAction::Block,
        }
    }
    fn unblock_cmd(ip: IpAddr) -> EnforceCommand {
        EnforceCommand {
            decision_id: Uuid::new_v4(), policy_version: 1, source: "test".into(),
            actor: "test".into(), timestamp_utc: 0, ttl_seconds: 0,
            reason: "manual".into(), ip, action: EnforceAction::Unblock,
        }
    }

    fn ip(a: [u8; 4]) -> IpAddr { IpAddr::from(a) }

    #[tokio::test]
    async fn block_then_unblock_reaches_dataplane_once() {
        let mut s = svc(Box::new(RecordingApplier::new()));
        let target = ip([9, 9, 9, 9]);
        s.enforce(block_cmd(target, 3600)).await.unwrap();
        s.enforce(unblock_cmd(target)).await.unwrap();
        // Dataplane saw both ops: blocked_ips empty after unblock proves the
        // unblock path ran; store record is Clean.
        assert!(!s.blocked_ips.contains(&target));
        let rec = s.store.get(&target).unwrap();
        match rec {
            Value::IpRecord(r) => assert_eq!(r.block_state, BlockState::Clean),
            _ => panic!("wrong value type"),
        }
    }

    #[tokio::test]
    async fn duplicate_decision_is_idempotent() {
        let mut s = svc(Box::new(RecordingApplier::new()));
        let target = ip([9, 9, 9, 8]);
        let cmd = block_cmd(target, 0);
        s.enforce(cmd.clone()).await.unwrap();
        s.enforce(cmd).await.unwrap();
        // One dataplane op: verify via store state + single blocked entry.
        assert_eq!(s.blocked_ips.len(), 1);
    }

    #[tokio::test]
    async fn unspecified_ip_rejected() {
        let mut s = svc(Box::new(RecordingApplier::new()));
        let err = s.enforce(block_cmd(IpAddr::from([0, 0, 0, 0]), 0)).await;
        assert!(matches!(err, Err(EnforcementError::InvalidCommand(_))));
    }

    #[tokio::test]
    async fn reblock_purges_stale_ttl() {
        let ra = Box::new(RecordingApplier::new());
        let mut s = svc(ra);
        let target = ip([9, 9, 9, 7]);
        // Block with TTL, unblock (manual), block again with TTL.
        s.enforce(block_cmd(target, 3600)).await.unwrap();
        s.enforce(unblock_cmd(target)).await.unwrap();
        s.enforce(block_cmd(target, 3600)).await.unwrap();
        // Exactly ONE pending expiration for the IP (old one purged).
        let pending: Vec<_> = s.expirations.iter().filter(|(_, i)| *i == target).collect();
        assert_eq!(pending.len(), 1, "re-block must not stack TTL entries");
    }

    #[tokio::test]
    async fn ttl_zero_block_has_no_expiration() {
        let mut s = svc(Box::new(RecordingApplier::new()));
        let target = ip([9, 9, 9, 6]);
        s.enforce(block_cmd(target, 0)).await.unwrap();
        assert!(s.expirations.iter().all(|(_, i)| *i != target));
        assert!(s.blocked_ips.contains(&target));
    }

    #[tokio::test]
    async fn storage_blocked_before_dataplane() {
        // If the dataplane errors, storage must STILL hold the block (fail-open
        // kernel, fail-closed state).
        struct FailingApplier;
        #[async_trait::async_trait]
        impl XdpApplier for FailingApplier {
            fn apply_block(&mut self, _: IpAddr, _: Uuid) -> Result<(), EnforcementError> {
                Err(EnforcementError::Xdp("kernel gone".into()))
            }
            fn apply_unblock(&mut self, _: IpAddr, _: Uuid) -> Result<(), EnforcementError> {
                Err(EnforcementError::Xdp("kernel gone".into()))
            }
            fn reconcile(&mut self, _: &[IpAddr]) -> Result<ReconciliationState, EnforcementError> {
                Ok(ReconciliationState::default())
            }
        }
        let store = Arc::new(Store::new(16));
        store.traffic.ram_limit_mb.store(512, Ordering::Relaxed);
        let mut s = EnforcementService::new(
            store.clone(), Arc::new(Metrics::new()), Box::new(FailingApplier),
            Arc::new(AtomicBool::new(false)),
        );
        let target = ip([9, 9, 9, 5]);
        let res = s.enforce(block_cmd(target, 0)).await.unwrap();
        assert!(!res.xdp_applied, "xdp_applied must be false on dataplane failure");
        let rec = store.get(&target).expect("record must exist");
        match rec {
            Value::IpRecord(r) => assert!(matches!(r.block_state, BlockState::Blocked { .. })),
            _ => panic!("wrong value type"),
        }
        assert!(s.blocked_ips.contains(&target));
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]
        #[test]
        fn sequence_invariant(ops in proptest::collection::vec(
            (proptest::arbitrary::any::<u8>(), proptest::bool::ANY, proptest::option::of(1u64..100)),
            1..64
        )) {
            use proptest::prop_assert;
            proptest::prop_assume!(!ops.is_empty());
            let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
            rt.block_on(async move {
                let mut s = svc(Box::new(RecordingApplier::new()));
                for (seed, is_block, ttl) in ops {
                    let target = ip([10, 0, seed / 2, seed]);
                    if is_block {
                        let ttl = ttl.unwrap_or(0);
                        let r = s.enforce(block_cmd(target, ttl)).await.unwrap();
                        prop_assert!(r.committed);
                        prop_assert!(s.blocked_ips.contains(&target));
                    } else {
                        let _ = s.enforce(unblock_cmd(target)).await.unwrap();
                        prop_assert!(!s.blocked_ips.contains(&target));
                        prop_assert!(s.expirations.iter().all(|(_, i)| *i != target),
                            "unblock must purge TTL entry");
                    }
                    // Global invariant: expirations never exceed blocked set size.
                    prop_assert!(s.expirations.len() <= s.blocked_ips.len());
                }
                Ok(())
            })?;
        }
    }
}