use ramshield_types::error::BlockReason;

pub struct ComplianceManager {
    pub audit_log: Vec<AuditEntry>,
    pub next_event_id: u64,
}

impl ComplianceManager {
    pub fn new() -> Self {
        Self {
            audit_log: Vec::new(),
            next_event_id: 0,
        }
    }

    pub fn log_block_decision(&mut self, decision: &BlockReason, actor: String) {
        let prev_hash = self
            .audit_log
            .last()
            .map(|e| e.entry_hash.clone())
            .unwrap_or_else(|| "0".repeat(64));

        let entry = AuditEntry::new(
            self.next_event_id,
            AuditEventType::BlockDecision,
            actor,
            None,
            serde_json::json!({
                "reason": format!("{:?}", decision),
            }),
            prev_hash,
        );
        self.audit_log.push(entry);
        self.next_event_id += 1;
    }
}

pub struct AuditEntry {
    pub id: u64,
    pub event_type: AuditEventType,
    pub actor: String,
    pub target: Option<String>,
    pub metadata: serde_json::Value,
    pub entry_hash: String,
}

impl AuditEntry {
    pub fn new(
        id: u64,
        event_type: AuditEventType,
        actor: String,
        target: Option<String>,
        metadata: serde_json::Value,
        prev_hash: String,
    ) -> Self {
        let entry_hash = format!(
            "{:x}",
            sha3::Sha3_256::digest(
                format!(
                    "{}{}{:?}{}{:?}{}",
                    id,
                    event_type as u8,
                    actor,
                    target.clone().unwrap_or_default(),
                    metadata,
                    prev_hash
                )
                .as_bytes()
            )
        );
        Self {
            id,
            event_type,
            actor,
            target,
            metadata,
            entry_hash,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AuditEventType {
    BlockDecision,
}

use sha3::Digest;

impl Default for ComplianceManager {
    fn default() -> Self {
        Self::new()
    }
}
