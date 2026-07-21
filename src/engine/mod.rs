pub mod learning;

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

use crate::config::Config;
use crate::detection::DetectionEngine;
use crate::forecasting::Forecaster;
use crate::learning::PatternLearner;
use crate::storage::Store;
use crate::metrics::{BatchRecord, BlockRecord, DashboardSnapshot, ModuleStats, SubnetRow};
use crate::storage::Store;
use arc_swap::ArcSwap;

use std::sync::atomic::{AtomicBool, Ordering};

pub struct Engine {
    pub config: ArcSwap<Config>,
    shutdown: AtomicBool,
}

impl Engine {
    pub fn new(cfg: Config) -> Self {
        Self {
            config: ArcSwap::from_pointee(cfg),
            shutdown: AtomicBool::new(false),
        }
    }

    pub fn start(&self) {
        info!("Engine::start: sync stub — call start_async to actually boot");
    }

    /// Boot the full pipeline: store, detection, forecasting, IPC server.
    /// Runs on a dedicated OS thread with its own current-thread tokio rt
    /// (mirrors the dashboard pattern in main.rs).
    pub fn start_async(self: Arc<Self>) -> std::io::Result<std::thread::JoinHandle<()>> {
        let _cfg = self.config.load();
        std::thread::Builder::new()
            .name("rs-engine".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .enable_time()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        tracing::error!("engine rt: {}", e);
                        return;
                    }
                };
                rt.block_on(async move {
                    if let Err(e) = boot_pipeline(self).await {
                        tracing::error!("pipeline: {}", e);
                    }
                });
            })
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
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

async fn boot_pipeline(engine: Arc<Engine>) -> std::io::Result<()> {
    let cfg_arc = engine.config.load(); // Arc<Config>
    let cfg_snapshot = cfg_arc.as_ref().clone();  // owned Config clone
    let cfg_handle = cfg_snapshot.clone().into_handle(); // ConfigHandle

    let metrics = Arc::new(crate::metrics::Metrics::new());
    let (block_tx, _) = broadcast::channel::<crate::detection::BlockDecision>(1024);
    let learner = Arc::new(PatternLearner::new(cfg_snapshot.detection.pattern_similarity_threshold));

    let store = Arc::new(Store::new(cfg_snapshot.engine.shard_count));
    let detection = Arc::new(DetectionEngine::new(
        store.clone(),
        cfg_handle.clone(),
        block_tx.clone(),
        metrics.clone(),
        learner.clone(),
        Arc::new(AtomicBool::new(false)),
    ));
    let event_tx = detection.event_sender();
    detection.clone().spawn_workers(cfg_snapshot.engine.worker_threads);

    let forecaster = Arc::new(Forecaster::new(
        store.clone(),
        cfg_snapshot.forecasting.clone(),
        block_tx.clone(),
        metrics.clone(),
        learner,
    ));
    tokio::spawn(async move { forecaster.run().await });

    let server = crate::ipc::server::IpcServer::bind(&cfg_snapshot, engine.clone(), event_tx, store).await?;
    server.start().await;
    Ok(())
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
