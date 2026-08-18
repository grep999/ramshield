    pub fn log_block_decision(&mut self, decision: &BlockReason, actor: String) {
        let prev_hash = self.audit_log.last()
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
