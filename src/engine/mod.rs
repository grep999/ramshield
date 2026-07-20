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
