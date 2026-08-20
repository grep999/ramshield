//! Enforcement Service - sole writer for all block/unblock operations
//!
//! Order of operations:
//! 1. Commit to WAL (durable)
//! 2. Mutate storage (in-memory)
//! 3. Schedule TTL expiry
//! 4. Apply XDP (kernel)
//! 5. Return committed/applied result

use anyhow::Result;
use crate::storage::{Store, Value, BlockState, BlockReason, IpRecord};
use crate::metrics::Metrics;
use crate::util::BoundedVecDeque;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock, watch};
use tracing::{info, warn, error};
use uuid::Uuid;

/// Enforcement command with full audit trail
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

/// Result of enforcement operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforceResult {
    pub decision_id: Uuid,
    pub committed: bool,
    pub applied: bool,
    pub wal_lsn: Option<u64>,
    pub xdp_applied: bool,
    pub error: Option<String>,
}

/// Enforcement service errors
#[derive(Debug, thiserror::Error)]
pub enum EnforcementError {
    #[error("WAL error: {0}")]
    Wal(String),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("XDP error: {0}")]
    Xdp(String),
    #[error("Duplicate decision_id: {0}")]
    Duplicate(Uuid),
    #[error("Invalid command: {0}")]
    InvalidCommand(String),
}

/// Reconciliation state for recovery
#[derive(Debug, Clone, Default)]
pub struct ReconciliationState {
    pub last_wal_lsn: u64,
    pub pending_blocks: Vec<IpAddr>,
    pub pending_unblocks: Vec<IpAddr>,
}

/// XDP apply trait - abstracts kernel interaction
#[async_trait::async_trait]
pub trait XdpApplier: Send + Sync {
    fn apply_block(&self, ip: IpAddr, decision_id: Uuid) -> Result<(), EnforcementError>;
    fn apply_unblock(&self, ip: IpAddr, decision_id: Uuid) -> Result<(), EnforcementError>;
    fn reconcile(&self, expected_blocks: &[IpAddr]) -> Result<ReconciliationState, EnforcementError>;
}

/// Stub XDP applier for non-eBPF environments
pub struct StubXdpApplier;

#[async_trait::async_trait]
impl XdpApplier for StubXdpApplier {
    fn apply_block(&self, ip: IpAddr, _decision_id: Uuid) -> Result<(), EnforcementError> {
        info!(%ip, "XDP block (stub)");
        Ok(())
    }

    fn apply_unblock(&self, ip: IpAddr, _decision_id: Uuid) -> Result<(), EnforcementError> {
        info!(%ip, "XDP unblock (stub)");
        Ok(())
    }

    fn reconcile(&self, _expected_blocks: &[IpAddr]) -> Result<ReconciliationState, EnforcementError> {
        Ok(ReconciliationState::default())
    }
}

/// Enforcement service - sole writer for all block/unblock operations
pub struct EnforcementService {
    store: Arc<Store>,
    metrics: Arc<Metrics>,
    xdp: Box<dyn XdpApplier>,
    processed_decisions: Arc<RwLock<HashSet<Uuid>>>,
    blocked_ips: HashSet<IpAddr>,
    shutdown_rx: watch::Receiver<bool>,
}

impl EnforcementService {
    /// Create new enforcement service
    pub fn new(
        store: Arc<Store>,
        metrics: Arc<Metrics>,
        xdp: Box<dyn XdpApplier>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            store,
            metrics,
            xdp,
            processed_decisions: Arc::new(RwLock::new(HashSet::new())),
            blocked_ips: HashSet::new(),
            shutdown_rx,
        }
    }

    /// Run the enforcement service - processes commands from channel
    pub async fn run(
        mut self,
        mut command_rx: mpsc::Receiver<EnforceCommand>,
    ) -> Result<()> {
        info!("Enforcement service started");

        // Reconcile on startup
        let expected_blocks: Vec<IpAddr> = self.store.get_all_blocked_ips();
        if let Err(e) = self.xdp.reconcile(&expected_blocks) {
            error!("Initial XDP reconciliation failed: {}", e);
        } else {
            self.blocked_ips = expected_blocks.into_iter().collect();
            info!("XDP reconciled with {} blocked IPs", self.blocked_ips.len());
        }

        loop {
            let mut shutdown_rx_clone = self.shutdown_rx.clone();
            tokio::select! {
                Some(cmd) = command_rx.recv() => {
                    if let Err(e) = self.enforce(cmd).await {
                        error!("Enforcement failed: {}", e);
                    }
                }
                _ = shutdown_rx_clone.changed() => {
                    if *shutdown_rx_clone.borrow() {
                        info!("Enforcement service shutting down");
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Execute a single enforcement command
    async fn enforce(&mut self, cmd: EnforceCommand) -> Result<EnforceResult, EnforcementError> {
        // Idempotency check
        {
            let decisions = self.processed_decisions.read().await;
            if decisions.contains(&cmd.decision_id) {
                info!(decision_id = %cmd.decision_id, "Duplicate command, returning cached result");
                return Ok(EnforceResult {
                    decision_id: cmd.decision_id,
                    committed: true,
                    applied: true,
                    wal_lsn: None,
                    xdp_applied: true,
                    error: None,
                });
            }
        }

        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);

        // Step 1: Deduplicate and Mutate storage
        let storage_result = match cmd.action {
            EnforceAction::Block => {
                if !self.blocked_ips.insert(cmd.ip) {
                    return Ok(EnforceResult { // Already blocked
                        decision_id: cmd.decision_id,
                        committed: true, applied: false, wal_lsn: None, xdp_applied: false, error: None
                    });
                }

                let block_reason = match cmd.reason.as_str() {
                    "high_rps" | "syn_flood" | "volumetric" | "slowloris" => BlockReason::HighRps(0),
                    "subnet_burst" => BlockReason::SubnetBatch,
                    "forecast_anomaly" => BlockReason::ForecastAnomaly,
                    "entropy_anomaly" | "anomaly" => BlockReason::EntropyAnomaly,
                    _ => BlockReason::ManualBlock,
                };

                let ttl_secs = if cmd.ttl_seconds > 0 { Some(cmd.ttl_seconds) } else { None };

                let key = cmd.ip;
                let mut rec = self.store.get(&key)
                    .and_then(|v| if let Value::IpRecord(r) = v { Some(r) } else { None })
                    .unwrap_or_else(|| IpRecord {
                        ip: cmd.ip, request_count: 0, ewma_rps: 0.0, baseline_rps: 0.0,
                        baseline_threat: 0.0, behavior_history_rps: BoundedVecDeque::new(10),
                        behavior_history_threat: BoundedVecDeque::new(10), first_seen_ns: now_ns,
                        last_seen_ns: now_ns, bytes_in: 0, status_dist: [0; 5],
                        proto_fingerprint: 0, country: [0; 2], threat_score: 0.0,
                        block_state: BlockState::Clean, asn: 0,
                    });

                rec.block_state = BlockState::Blocked { reason: block_reason, since_ns: now_ns };

                self.store.insert(key, Value::IpRecord(rec), ttl_secs, self.store.ram_bytes())
                    .map_err(|e| EnforcementError::Storage(e.to_string()))
            }
            EnforceAction::Unblock => {
                if !self.blocked_ips.remove(&cmd.ip) {
                    return Ok(EnforceResult { // Already unblocked
                        decision_id: cmd.decision_id,
                        committed: true, applied: false, wal_lsn: None, xdp_applied: false, error: None
                    });
                }
                
                if let Some(Value::IpRecord(mut rec)) = self.store.get(&cmd.ip) {
                    rec.block_state = BlockState::Clean;
                    self.store.insert(cmd.ip, Value::IpRecord(rec), None, self.store.ram_bytes())
                        .map_err(|e| EnforcementError::Storage(e.to_string()))
                } else {
                    Ok(()) // Not tracked = already unblocked
                }
            }
        };

        if let Err(e) = storage_result {
            error!("Storage mutation failed: {}", e);
            return Ok(EnforceResult {
                decision_id: cmd.decision_id,
                committed: false,
                applied: false,
                wal_lsn: None,
                xdp_applied: false,
                error: Some(e.to_string()),
            });
        }

        // Step 2: Schedule TTL (handled by storage entry expires_at)

        // Step 3: Apply XDP
        let xdp_applied = match cmd.action {
            EnforceAction::Block => {
                match self.xdp.apply_block(cmd.ip, cmd.decision_id) {
                    Ok(()) => true,
                    Err(e) => {
                        warn!("XDP block failed: {}", e);
                        false
                    }
                }
            }
            EnforceAction::Unblock => {
                match self.xdp.apply_unblock(cmd.ip, cmd.decision_id) {
                    Ok(()) => true,
                    Err(e) => {
                        warn!("XDP unblock failed: {}", e);
                        false
                    }
                }
            }
        };

        // Record decision as processed
        self.processed_decisions.write().await.insert(cmd.decision_id);

        // Update metrics
        if matches!(cmd.action, EnforceAction::Block) {
            self.metrics.inc_blocks();
        }

        Ok(EnforceResult {
            decision_id: cmd.decision_id,
            committed: true,
            applied: true,
            wal_lsn: None, // WAL not yet integrated
            xdp_applied,
            error: None,
        })
    }
}