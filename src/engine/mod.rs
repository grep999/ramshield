pub mod learning;

use arc_swap::ArcSwap;
use crate::config::Config;
use crate::metrics::{BatchRecord, BlockRecord, DashboardSnapshot, ModuleStats, SubnetRow};

pub struct Engine {
    pub config: ArcSwap<Config>,
}

impl Engine {
    pub fn new(cfg: Config) -> Self {
        Self {
            config: ArcSwap::from_pointee(cfg),
        }
    }

    pub fn start(&self) {
        // stub: engine lifecycle
    }

    pub fn dashboard_snapshot(&self) -> DashboardSnapshot {
        DashboardSnapshot::default()
    }

    pub fn get_batch_history(&self) -> Vec<BatchRecord> {
        Vec::new()
    }

    pub fn get_block_log(&self) -> Vec<BlockRecord> {
        Vec::new()
    }

    pub fn get_hot_subnets(&self) -> Vec<SubnetRow> {
        Vec::new()
    }

    pub fn get_module_stats(&self) -> Vec<ModuleStats> {
        vec![
            ModuleStats { label: "IPC".into(), events: 0, errors: 0, rate_per_sec: 0.0, detail: serde_json::json!({}) },
            ModuleStats { label: "Detection".into(), events: 0, errors: 0, rate_per_sec: 0.0, detail: serde_json::json!({}) },
            ModuleStats { label: "Forecasting".into(), events: 0, errors: 0, rate_per_sec: 0.0, detail: serde_json::json!({}) },
            ModuleStats { label: "Storage".into(), events: 0, errors: 0, rate_per_sec: 0.0, detail: serde_json::json!({}) },
        ]
    }
}

#[cfg(test)]
mod startup_tests {
    //! BACKLOG #14 — engine startup integration tests.
    //! Lives in-tree rather than in `tests/` because the bin currently
    //! fails to compile (pre-existing rot, out of scope for this atomic
    //! task); in-tree tests ride `cargo test --lib`.
    use super::*;
    use crate::Config;

    #[test]
    fn engine_constructs_with_default_config() {
        let _engine = Engine::new(Config::default());
    }

    #[test]
    fn engine_start_then_snapshot_default_state() {
        let engine = Engine::new(Config::default());
        engine.start();
        let snap = engine.dashboard_snapshot();
        assert!(snap.is_healthy);
        assert_eq!(snap.ips_tracked, 0);
        assert_eq!(snap.blocked_total, 0);
        assert_eq!(snap.events_ingested, 0);
    }

    #[test]
    fn engine_module_stats_have_four_canonical_rows() {
        let engine = Engine::new(Config::default());
        engine.start();
        let stats = engine.get_module_stats();
        assert_eq!(stats.len(), 4);
        let labels: Vec<&str> = stats.iter().map(|m| m.label.as_str()).collect();
        assert!(labels.contains(&"IPC"));
        assert!(labels.contains(&"Detection"));
        assert!(labels.contains(&"Forecasting"));
        assert!(labels.contains(&"Storage"));
    }
}
