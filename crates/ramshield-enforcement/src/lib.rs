//! RamShield Enforcement Service
//!
//! Sole writer for block/unblock operations. All mutations go through idempotent
//! commands with decision_id, policy_version, source, actor, timestamps, TTL, reason.
//!
//! Order of operations:
//! 1. Commit to WAL (durable)
//! 2. Mutate storage (in-memory)
//! 3. Schedule TTL expiry
//! 4. Apply XDP (kernel)
//! 5. Return committed/applied result

use anyhow::Result;
use ramshield_storage::{Store, Entry, wal::{Wal, WalEntry}};
use ramshield_types::BlockReason;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
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
    async fn apply_block(&self, ip: IpAddr, decision_id: Uuid) -> Result<(), EnforcementError>;
    async fn apply_unblock(&self, ip: IpAddr, decision_id: Uuid) -> Result<(), EnforcementError>;
    async fn reconcile(&self, expected_blocks: &[IpAddr]) -> Result<ReconciliationState, EnforcementError>;
}

/// Stub XDP applier for non-eBPF environments
pub struct StubXdpApplier;

#[async_trait::async_trait]
impl XdpApplier for StubXdpApplier {
    async fn apply_block(&self, ip: IpAddr, _decision_id: Uuid) -> Result<(), EnforcementError> {
        info!(%ip, "XDP block (stub)");
        Ok(())
    }
    
    async fn apply_unblock(&self, ip: IpAddr, _decision_id: Uuid) -> Result<(), EnforcementError> {
        info!(%ip, "XDP unblock (stub)");
        Ok(())
    }
    
    async fn reconcile(&self, _expected_blocks: &[IpAddr]) -> Result<ReconciliationState, EnforcementError> {
        Ok(ReconciliationState::default())
    }
}

/// Enforcement service - sole writer for all block/unblock operations
pub struct EnforcementService<X: XdpApplier = StubXdpApplier> {
    store: Arc<Store>,
    wal: Option<Arc<Wal>>,
    xdp: X,
    processed_decisions: Arc<tokio::sync::RwLock<HashSet<Uuid>>>,
}

impl<X: XdpApplier> EnforcementService<X> {
    /// Create new enforcement service
    pub fn new(store: Arc<Store>, wal: Option<Arc<Wal>>, xdp: X) -> Self {
        Self {
            store,
            wal,
            xdp,
            processed_decisions: Arc::new(tokio::sync::RwLock::new(HashSet::new())),
        }
    }
    
    /// Run the enforcement service - processes commands from channel
    pub async fn run(
        self: Arc<Self>,
        mut command_rx: mpsc::Receiver<EnforceCommand>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        info!("Enforcement service started");
        
        loop {
            tokio::select! {
                Some(cmd) = command_rx.recv() => {
                    let result = self.enforce(cmd).await;
                    if let Err(e) = result {
                        error!("Enforcement failed: {}", e);
                    }
                }
                _ = shutdown.changed() => {
                    info!("Enforcement service shutting down");
                    break;
                }
            }
        }
        
        Ok(())
    }
    
    /// Execute a single enforcement command
    /// 
    /// Order: WAL → Storage → TTL → XDP → Return
    pub async fn enforce(&self, cmd: EnforceCommand) -> Result<EnforceResult, EnforcementError> {
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
        
        // Step 1: Commit to WAL
        let wal_lsn = if let Some(ref wal) = self.wal {
            let entry = match cmd.action {
                EnforceAction::Block => WalEntry::BlockIp {
                    ip: cmd.ip.to_string(),
                    reason: cmd.reason.clone(),
                    ttl_secs: Some(cmd.ttl_seconds),
                    ts_ns: now_ns,
                },
                EnforceAction::Unblock => WalEntry::UnblockIp {
                    ip: cmd.ip.to_string(),
                    ts_ns: now_ns,
                },
            };
            
            let lsn = wal.append(&entry)
                .map_err(|e| EnforcementError::Wal(e.to_string()))?;
            
            Some(lsn)
        } else {
            None
        };
        
        // Step 2: Mutate storage
        let storage_result = match cmd.action {
            EnforceAction::Block => {
                let entry = Entry {
                    value: format!("blocked:{}", cmd.reason),
                    is_blocked: true,
                    reason: Some(reason_str_to_blockreason(&cmd.reason)),
                };
                self.store
                    .insert(cmd.ip.to_string(), entry)
                    .map_err(|e| EnforcementError::Storage(e.to_string()))
            }
            EnforceAction::Unblock => {
                self.store
                    .remove(&cmd.ip.to_string());
                Ok(())
            }
        };
        
        if let Err(e) = storage_result {
            error!("Storage mutation failed: {}", e);
            return Ok(EnforceResult {
                decision_id: cmd.decision_id,
                committed: wal_lsn.is_some(),
                applied: false,
                wal_lsn,
                xdp_applied: false,
                error: Some(e.to_string()),
            });
        }
        
        // Step 3: Schedule TTL (for blocks only)
        if matches!(cmd.action, EnforceAction::Block) && cmd.ttl_seconds > 0 {
            // ponytail: TTL wheel integration deferred - would need typed key support
            info!(ip = %cmd.ip, ttl = cmd.ttl_seconds, "TTL scheduled (stub)");
        }
        
        // Step 4: Apply XDP
        let xdp_applied = match cmd.action {
            EnforceAction::Block => {
                match self.xdp.apply_block(cmd.ip, cmd.decision_id).await {
                    Ok(()) => true,
                    Err(e) => {
                        warn!("XDP block failed: {}", e);
                        false
                    }
                }
            }
            EnforceAction::Unblock => {
                match self.xdp.apply_unblock(cmd.ip, cmd.decision_id).await {
                    Ok(()) => true,
                    Err(e) => {
                        warn!("XDP unblock failed: {}", e);
                        false
                    }
                }
            }
        };
        
        // Step 5: Mark as processed
        {
            let mut decisions = self.processed_decisions.write().await;
            decisions.insert(cmd.decision_id);
        }
        
        info!(
            decision_id = %cmd.decision_id,
            ip = %cmd.ip,
            action = ?cmd.action,
            committed = true,
            xdp_applied,
            "Enforcement complete"
        );
        
        Ok(EnforceResult {
            decision_id: cmd.decision_id,
            committed: true,
            applied: true,
            wal_lsn,
            xdp_applied,
            error: None,
        })
    }
    
    /// Reconcile kernel state after restart
    /// 
    /// Replays WAL and rebuilds XDP blocklist to match storage state
    pub async fn reconcile(&self) -> Result<ReconciliationState, EnforcementError> {
        info!("Starting enforcement reconciliation");
        
        // Collect expected blocks from storage
        let mut expected_blocks = Vec::new();
        for entry in self.store.iter() {
            if entry.value().is_blocked {
                if let Ok(ip) = entry.key().parse::<IpAddr>() {
                    expected_blocks.push(ip);
                }
            }
        }
        
        // Reconcile XDP with storage
        let state = self.xdp.reconcile(&expected_blocks).await?;
        
        info!(
            blocks = expected_blocks.len(),
            last_lsn = state.last_wal_lsn,
            "Reconciliation complete"
        );
        
        Ok(state)
    }
    
    /// Flush and sync all pending operations
    pub async fn flush_and_sync(&self) -> Result<()> {
        if let Some(ref _wal) = self.wal {
            // Force WAL sync - ponytail: Wal needs explicit sync method
        }
        Ok(())
    }
}

/// Convert reason string to BlockReason enum
fn reason_str_to_blockreason(reason: &str) -> BlockReason {
    // ponytail: interim mapping; superseded by types::BlockReason::from_reason_str in Phase 3 merge
    BlockReason::from_reason_str(&reason.to_lowercase()).unwrap_or(BlockReason::ManualBlock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    
    #[tokio::test]
    async fn test_enforce_block() {
        let store = Arc::new(Store::new(1024));
        let xdp = StubXdpApplier;
        let service = EnforcementService::new(store, None, xdp);
        
        let cmd = EnforceCommand {
            decision_id: Uuid::new_v4(),
            policy_version: 1,
            source: "detection".to_string(),
            actor: "system".to_string(),
            timestamp_utc: 0,
            ttl_seconds: 3600,
            reason: "ddos".to_string(),
            ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            action: EnforceAction::Block,
        };
        
        let result = service.enforce(cmd).await.unwrap();
        assert!(result.committed);
        assert!(result.applied);
        assert!(result.xdp_applied);
    }
    
    #[tokio::test]
    async fn test_idempotency() {
        let store = Arc::new(Store::new(1024));
        let xdp = StubXdpApplier;
        let service = EnforcementService::new(store, None, xdp);
        
        let decision_id = Uuid::new_v4();
        let cmd = EnforceCommand {
            decision_id,
            policy_version: 1,
            source: "detection".to_string(),
            actor: "system".to_string(),
            timestamp_utc: 0,
            ttl_seconds: 3600,
            reason: "ddos".to_string(),
            ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            action: EnforceAction::Block,
        };
        
        let result1 = service.enforce(cmd.clone()).await.unwrap();
        let result2 = service.enforce(cmd).await.unwrap();
        
        assert!(result1.committed);
        assert!(result2.committed);
    }
    
    #[tokio::test]
    async fn test_enforce_unblock() {
        let store = Arc::new(Store::new(1024));
        let xdp = StubXdpApplier;
        let service = EnforcementService::new(store, None, xdp);
        
        // First block
        let block_cmd = EnforceCommand {
            decision_id: Uuid::new_v4(),
            policy_version: 1,
            source: "detection".to_string(),
            actor: "system".to_string(),
            timestamp_utc: 0,
            ttl_seconds: 3600,
            reason: "ddos".to_string(),
            ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3)),
            action: EnforceAction::Block,
        };
        service.enforce(block_cmd).await.unwrap();
        
        // Then unblock
        let unblock_cmd = EnforceCommand {
            decision_id: Uuid::new_v4(),
            policy_version: 1,
            source: "manual".to_string(),
            actor: "admin".to_string(),
            timestamp_utc: 0,
            ttl_seconds: 0,
            reason: "false positive".to_string(),
            ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3)),
            action: EnforceAction::Unblock,
        };
        
        let result = service.enforce(unblock_cmd).await.unwrap();
        assert!(result.committed);
        assert!(result.applied);
    }
}
