use crate::*;
use ramshield_mesh::transport::MeshMessage;

impl EnforcementService {
    pub(crate) async fn apply_mesh_messages(&mut self) {
        let Some(handle) = self.mesh_handle.clone() else {
            return;
        };
        let messages = handle.drain().await;
        if messages.is_empty() {
            return;
        }
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        for message in messages {
            match message {
                MeshMessage::Block(delta) => {
                    let _ = self.apply_mesh_block(delta, now_ms).await;
                }
                MeshMessage::Unblock(delta) => {
                    let _ = self.apply_mesh_unblock(delta, now_ms).await;
                }
                MeshMessage::Sync { blocks, unblocks } => {
                    for delta in blocks {
                        let _ = self.apply_mesh_block(delta, now_ms).await;
                    }
                    for delta in unblocks {
                        let _ = self.apply_mesh_unblock(delta, now_ms).await;
                    }
                }
            }
        }
    }

    async fn apply_mesh_block(
        &mut self,
        delta: ramshield_mesh::aworset::ClusterBlockDelta,
        now_ms: u64,
    ) -> Result<(), EnforcementError> {
        if delta.expires_at_ms <= now_ms {
            return Ok(());
        }
        let Some(mesh) = &self.mesh_blocklist else {
            return Ok(());
        };
        if self.mesh_operator_suppressions.contains(&delta.ip) {
            return Ok(());
        }
        // CRDT state can be merged before enforcement fails (e.g. WAL
        // append or storage error). Retry an unchanged delta until its
        // projection has succeeded; only skip duplicates already applied.
        let changed = mesh.merge_delta(&delta);
        if should_skip_mesh_block(changed, self.mesh_applied_ips.contains(&delta.ip)) {
            return Ok(());
        }
        let remaining_ms = delta.expires_at_ms.saturating_sub(now_ms);
        let ttl = if delta.expires_at_ms == u64::MAX {
            0
        } else {
            remaining_ms.saturating_add(999) / 1000
        };
        if delta.expires_at_ms != u64::MAX && ttl == 0 {
            return Ok(());
        }
        let cmd = EnforceCommand {
            decision_id: Uuid::new_v4(),
            policy_version: 1,
            source: "mesh".into(),
            actor: format!("mesh:{}", delta.dot.node_id),
            timestamp_utc: (now_ms / 1000) as i64,
            ttl_seconds: ttl,
            reason: "mesh_final".into(),
            ip: delta.ip,
            cidr: None,
            action: EnforceAction::Block,
            evidence_source: ramshield_types::EvidenceSource::FleetSignals,
        };
        let enforce_result = self.enforce(cmd).await;
        if enforce_result.is_ok() {
            self.mesh_applied_ips.insert(delta.ip);
        }
        enforce_result.map(|_| ())
    }

    async fn apply_mesh_unblock(
        &mut self,
        delta: ramshield_mesh::aworset::ClusterUnblockDelta,
        now_ms: u64,
    ) -> Result<(), EnforcementError> {
        let Some(mesh) = &self.mesh_blocklist else {
            return Ok(());
        };
        // As with blocks, retain retryability if the tombstone was
        // merged but the local enforcement unblock failed.
        mesh.merge_unblock_delta(&delta);
        let still_blocked = mesh.is_blocked(&delta.ip, now_ms);
        let locally_applied = self.mesh_applied_ips.contains(&delta.ip);
        if should_skip_mesh_unblock(still_blocked, locally_applied) {
            return Ok(());
        }
        let cmd = EnforceCommand {
            decision_id: Uuid::new_v4(),
            policy_version: 1,
            source: "mesh".into(),
            actor: format!("mesh:{}", delta.dot.node_id),
            timestamp_utc: (now_ms / 1000) as i64,
            ttl_seconds: 0,
            reason: "mesh_purge".into(),
            ip: delta.ip,
            cidr: None,
            action: EnforceAction::Unblock,
            evidence_source: ramshield_types::EvidenceSource::FleetSignals,
        };
        let enforce_result = self.enforce(cmd).await;
        if enforce_result.is_ok() {
            self.mesh_applied_ips.remove(&delta.ip);
        }
        enforce_result.map(|_| ())
    }
}

fn should_skip_mesh_block(crdt_changed: bool, locally_applied: bool) -> bool {
    !crdt_changed && locally_applied
}

fn should_skip_mesh_unblock(still_blocked: bool, locally_applied: bool) -> bool {
    still_blocked || !locally_applied
}

#[cfg(test)]
mod retry_tests {
    use super::{should_skip_mesh_block, should_skip_mesh_unblock};

    #[test]
    fn unchanged_block_is_retried_until_local_projection_succeeds() {
        assert!(
            !should_skip_mesh_block(false, false),
            "failed prior apply must retry"
        );
        assert!(
            should_skip_mesh_block(false, true),
            "successful duplicate can be skipped"
        );
        assert!(
            !should_skip_mesh_block(true, true),
            "new CRDT state must be applied"
        );
    }

    #[test]
    fn merged_unblock_is_retried_until_local_projection_succeeds() {
        assert!(
            !should_skip_mesh_unblock(false, true),
            "failed prior unblock must retry"
        );
        assert!(
            should_skip_mesh_unblock(true, true),
            "another live ban still blocks the IP"
        );
        assert!(
            should_skip_mesh_unblock(false, false),
            "already-unapplied IP needs no duplicate unblock"
        );
    }
}
