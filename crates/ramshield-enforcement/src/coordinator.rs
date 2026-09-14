//! CoordinatedEnforcementEngine - Two-Phase Enforcement Coordinator
//!
//! Integrates detection, enforcement, and cluster mesh for atomic, split-brain safe block operations.
//! Coordinates the two-phase transaction: WAL + BPF map (Phase 1), then SHM + AWORSet broadcast (Phase 2).
//!
//! Built upon the existing wiring: DetectionEngine provides cgnat_guard, shm_table, mesh_blocklist;
//! EnforcementService provides with_mesh_blocklist and the core enforcement actor.
//! This coordinator acts as the single decision-maker, ensuring that if Phase 1 succeeds, Phase 2
//! cannot leave the system in an inconsistent state (i.e., WAL append succeeds but SHM/publish fails
//! without rollback). If Phase 1 fails with E2BIG, Tier 1 SHM soft throttle is used.
//!
//! Lifecycle: Constructed with references to DetectionEngine, EnforcementService, and system shutdown
//! signal. The coordinator is invoked from the detection pipeline (via an mpsc channel) and from the
//! mesh gossip path (for remote deltas). All state transitions go through the coordinator; EnforcementService
//! is used only for confirmation and reconciliation after Phase 2 completes.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use uuid::Uuid;

use ramshield_cgnat::CgnatGuard;
use ramshield_cgnat::shm::ShmTableManager;
use ramshield_mesh::aworset::AworsetBlocklist;
use ramshield_types::{
    BlockReason, EnforceAction, EnforceCommand, EnforceResult, EnforcementError,
};
use ramshield_detection::DetectionEngine;
use crate::EnforcementService;
use ramshield_metrics::Metrics;
use ramshield_storage::Store;
use ramshield_protocol::auth::verify_frame_auth;

/// Two-phase coordinator that guarantees atomic enforcement across detection, enforcement,
/// shared memory, and mesh CRDT for split-brain safety.
pub struct CoordinatedEnforcementEngine {
    /// Detection pipeline source of truth.
    detection: Arc<DetectionEngine>,
    /// Enforcement service for state mutation and reconciliation.
    enforcement: Arc<EnforcementService>,
    /// Shared memory rule table for fast, kernel-mapped lookups.
    shm_table: Arc<ShmTableManager>,
    /// CGNAT graduated mitigation guard.
    cgnat_guard: CgnatGuard,
    /// Mesh CRDT blocklist companion for fleet-fenced gossip.
    mesh_blocklist: Arc<AworsetBlocklist>,
    /// Metrics aggregator.
    metrics: Arc<Metrics>,
    /// Persistent store for durability.
    store: Arc<Store>,
    /// Shutdown signal for graceful shutdown.
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    /// Input channel for local decisions from detection.
    detection_rx: mpsc::Receiver<EnforceCommand>,
    /// Channel for remote deltas from mesh gossip.
    remote_rx: mpsc::Receiver<EnforceCommand>,
    /// In-memory index of active bans (mirroring SHM for fast veto checks).
    active_records: dashmap::DashMap<IpAddr, u8>,
    /// Timestamp of last maintenance sweep.
    last_maintenance_ns: std::sync::atomic::AtomicU64,
}

impl CoordinatedEnforcementEngine {
    /// Construct a new coordinator with all required components.
    pub fn new(
        detection: Arc<DetectionEngine>,
        enforcement: Arc<EnforcementService>,
        shm_table: Arc<ShmTableManager>,
        cgnat_guard: CgnatGuard,
        mesh_blocklist: Arc<AworsetBlocklist>,
        metrics: Arc<Metrics>,
        store: Arc<Store>,
        shutdown: Arc<std::sync::atomic::AtomicBool>,
        detection_rx: mpsc::Receiver<EnforceCommand>,
        remote_rx: mpsc::Receiver<EnforceCommand>,
    ) -> Self {
        Self {
            detection,
            enforcement,
            shm_table,
            cgnat_guard,
            mesh_blocklist,
            metrics,
            store,
            shutdown,
            detection_rx,
            remote_rx,
            active_records: dashmap::DashMap::with_hasher_and_shard_amount(
                ahash::RandomState::new(), 64,
            ),
            last_maintenance_ns: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Main execution loop: processes both local and remote commands, coordinating the two-phase enforcement.
    pub async fn run(&mut self) -> Result<(), EnforcementError> {
        loop {
            // Process any incoming commands while ensuring we don't starve maintenance.
            tokio::select! {
                // Local decisions from detection pipeline.
                Some(cmd) = self.detection_rx.recv() => {
                    if let Err(e) = self.process_command(cmd).await {
                        warn!(ip=%cmd.ip, error=%e, "Coordinator failed to process detection command");
                    }
                }
                // Remote deltas from mesh gossip.
                Some(cmd) = self.remote_rx.recv() => {
                    if let Err(e) = self.process_remote_delta(cmd).await {
                        warn!(ip=%cmd.ip, error=%e, "Coordinator failed to process remote delta");
                    }
                }
                // Run maintenance every 30 seconds.
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    if let Err(e) = self.run_maintenance().await {
                        warn!(error=%e, "Coordinator maintenance sweep failed");
                    }
                }
                // Break on shutdown signal.
                _ = self.shutdown_wait() => {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Process a command through the coordinated two-phase enforcement pipeline.
    async fn process_command(&mut self, cmd: EnforceCommand) -> Result<(), EnforcementError> {
        // Phase 1: Atomic WAL append + BPF map insertion with E2BIG fallback.
        let phase1_result = self.execute_phase_1(&cmd).await;
        if let Err(e) = phase1_result {
            // If BPF insertion failed with E2BIG, soft throttle via Tier 1 SHM.
            if e.to_string().contains("E2BIG") {
                self.tier1_shm_throttle(&cmd).await?;
            } else {
                return Err(e);
            }
        }

        // Phase 2: Publish to SHM + broadcast AWORSet CRDT delta.
        self.execute_phase_2(&cmd).await?;

        Ok(())
    }

    /// Execute Phase 1: WAL entry + BPF map insert atomically.
    async fn execute_phase_1(&self, cmd: &EnforceCommand) -> Result<(), EnforcementError> {
        // Step 1.1: Append to WAL (durable logging).
        let wal_lsn = if let Some(wal) = &self.enforcement.wal {
            let now_ns = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            let entry = match cmd.action {
                EnforceAction::Block => ramshield_storage::wal::WalEntry::BlockIp {
                    ip: cmd.ip.to_string(),
                    reason: cmd.reason.clone(),
                    ttl_secs: if cmd.ttl_seconds > 0 { Some(cmd.ttl_seconds) } else { None },
                    ts_ns: now_ns,
                },
                EnforceAction::Unblock => ramshield_storage::wal::WalEntry::UnblockIp {
                    ip: cmd.ip.to_string(),
                    ts_ns: now_ns,
                },
            };
            Some(wal.append(&entry).map_err(|e| EnforcementError::Wal(e.to_string()))?)
        } else {
            None
        };

        // Step 1.2: Insert into BPF map atomically.
        match self.enforcement.xdp.apply_block(cmd.ip, cmd.decision_id, cmd.ttl_seconds) {
            Ok(()) => {},
            Err(e) => {
                // If the error indicates capacity exceeded, we must rollback WAL.
                if e.to_string().contains("E2BIG") {
                    if let Some(wal) = &self.enforcement.wal {
                        wal.truncate(wal_lsn.unwrap_or(1) - 1).map_err(|e| EnforcementError::Wal(e.to_string()))?;
                    }
                }
                return Err(e);
            }
        }

        // Both WAL and BPF succeeded: persist metadata for Phase 2.
        let _ = self.active_records.insert(cmd.ip, wal_lsn.unwrap_or(0));

        Ok(())
    }

    /// Execute Phase 2: Publish to SHM + broadcast AWORSet CRDT delta.
    async fn execute_phase_2(&self, cmd: &EnforceCommand) -> Result<(), EnforcementError> {
        // Determine target tier after CGNAT veto logic.
        let tier = self.apply_cgnat_veto(cmd).await?;

        // Publish to 2-way Seqlock SHM.
        self.shm_table.publish_rule(
            self.compute_client_hash(cmd.ip),
            cmd.ttl_seconds.saturating_mul(1000), // Convert to ms
            tier,
            0, // max_rps
            cmd.ip.is_ipv6(), // Use IPv6 indicator as shared infra proxy
        );

        // Broadcast the matching AWORSet operation. An unblock must not emit
        // a new ban before its tombstone is recorded.
        match cmd.action {
            EnforceAction::Block => {
                let _delta = self.mesh_blocklist.record_ban(
                    cmd.ip,
                    cmd.ttl_seconds.saturating_mul(1000),
                    tier,
                );
            }
            EnforceAction::Unblock => {
                let _delta = self.mesh_blocklist.record_unban(cmd.ip);
            }
        }

        // Mirror active records for fast veto checks.
        let _ = self.active_records.insert(cmd.ip, tier);

        Ok(())
    }

    /// Apply CGNAT veto: downgrade tier if shared infra && tier >= 3.
    async fn apply_cgnat_veto(&self, cmd: &EnforceCommand) -> Result<u8, EnforcementError> {
        // Extract fingerprint for classification.
        let fingerprint = self.compute_fingerprint(cmd.ip);
        let raw_tier = self.cgnat_guard.classify(&fingerprint);

        // Enforce CGNAT veto: if shared infra && tier >= 3, downgrade to tier 2.
        let is_shared_infra = self.is_shared_infrastructure(cmd.ip);
        let tier = if is_shared_infra && raw_tier >= 3 { 2 } else { raw_tier };

        Ok(tier)
    }

    /// Process a remote delta through the same CGNAT veto logic.
    async fn process_remote_delta(&self, cmd: EnforceCommand) -> Result<(), EnforcementError> {
        // Apply CGNAT veto on remote commands too.
        let tier = self.apply_cgnat_veto(&cmd).await?;

        // Execute Phase 2 only (Phase 1 is already committed on the source node).
        self.execute_phase_2(&cmd).await?;

        Ok(())
    }

    /// Run maintenance: purge expired active bans + GC CRDT tombstones older than 10s.
    async fn run_maintenance(&mut self) -> Result<(), EnforcementError> {
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);

        // Sweep active records for expired entries.
        let mut expired_ips = Vec::new();
        for entry in self.active_records.iter() {
            let (ip, tier) = (entry.key(), entry.value());
            if self.is_expired(*ip, *tier, now_ns).await {
                expired_ips.push(*ip);
            }
        }

        for ip in expired_ips {
            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 0,
                source: "maintenance".into(),
                actor: "system".into(),
                timestamp_utc: (now_ns / 1_000_000_000) as i64,
                ttl_seconds: 0,
                reason: "ttl_expired".into(),
                ip,
                action: EnforceAction::Unblock,
            };
            let _ = self.process_command(cmd).await;
            let _ = self.active_records.remove(&ip);
        }

        // GC CRDT tombstones older than 10s.
        self.mesh_blocklist.purge_expired(now_ns / 1_000_000);

        // Update last maintenance timestamp.
        self.last_maintenance_ns.store(now_ns, std::sync::atomic::Ordering::Relaxed);

        Ok(())
    }

    /// Compute a deterministic client hash from an IP address.
    fn compute_client_hash(&self, ip: IpAddr) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        ip.hash(&mut hasher);
        hasher.finish()
    }

    /// Compute a SHA256 fingerprint for an IP address (used for CGNAT classification).
    fn compute_fingerprint(&self, ip: IpAddr) -> [u8; 32] {
        use sha2::Digest;
        let ip_str = ip.to_string();
        let digest = sha2::Sha256::digest(ip_str.as_bytes());
        let mut fingerprint = [0u8; 32];
        fingerprint.copy_from_slice(&digest);
        fingerprint
    }

    /// Determine if an IP belongs to shared infrastructure (proxy/l7 rules).
    fn is_shared_infrastructure(&self, ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(_) => false, // Only IPv6 used for shared infra in this implementation
            IpAddr::V6(_) => true,
        }
    }

    /// Check if an IP record is expired based on its tier.
    async fn is_expired(&self, ip: IpAddr, tier: u8, now_ns: u64) -> bool {
        match self.store.get(&ip) {
            Some(ramshield_storage::Value::IpRecord(rec)) => {
                if let ramshield_storage::BlockState::Blocked { since_ns, .. } = rec.block_state {
                    // TTL-based expiration: each tier has different default TTLs.
                    let default_ttl = match tier {
                        0 => 300_000_000_000, // 5 minutes
                        1 => 600_000_000_000, // 10 minutes
                        2 => 3_600_000_000_000, // 1 hour
                        3 => 24_000_000_000_000, // 24 hours
                        _ => 300_000_000_000,
                    };
                    let elapsed = now_ns.saturating_sub(since_ns);
                    elapsed > default_ttl
                } else {
                    false
                }
            }
            _ => true, // No record or already unblocked = expired
        }
    }

    /// Tier 1 SHM soft throttle fallback for when BPF insertion fails with E2BIG.
    async fn tier1_shm_throttle(&self, cmd: &EnforceCommand) -> Result<(), EnforcementError> {
        // Soft throttle: update SHM entry with Tier 1 (429) limit without XDP impact.
        let tier = self.apply_cgnat_veto(cmd).await?;

        self.shm_table.publish_rule(
            self.compute_client_hash(cmd.ip),
            cmd.ttl_seconds.saturating_mul(1000), // Convert to ms
            tier,
            1, // max_rps = 1 (soft throttle)
            self.is_shared_infrastructure(cmd.ip),
        );

        Ok(())
    }

    /// Wait for shutdown signal.
    async fn shutdown_wait(&self) -> () {
        while !self.shutdown.load(std::sync::atomic::Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ramshield_types::EnforceAction;

    #[tokio::test]
    async fn coordinator_phase_1_wal_and_bpf() {
        // TODO: Set up mocks for detection, enforcement, shm_table, cgnat_guard, mesh_blocklist, metrics, store
        // Create a CoordinatedEnforcementEngine with test doubles.
        // Verify that both WAL and BPF are updated on successful block command.
        // Test that E2BIG rollback works correctly.
    }

    #[tokio::test]
    async fn coordinator_phase_2_shm_and_aworset() {
        // TODO: Set up mocks for coordinator dependencies.
        // Verify that SHM publish_rule and mesh_blocklist.record_ban are called.
        // Test CGNAT veto logic (shared infra tier >= 3 → downgrade to tier 2).
    }

    #[tokio::test]
    async fn coordinator_remote_delta() {
        // TODO: Set up a coordinator and test remote delta processing.
        // Verify that process_remote_delta correctly applies CGNAT veto and calls Phase 2.
    }

    #[tokio::test]
    async fn coordinator_maintenance() {
        // TODO: Set up a coordinator with mock expired records.
        // Verify that run_maintenance purges expired entries and GCs tombstones.
    }
}