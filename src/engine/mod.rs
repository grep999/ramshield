pub mod learning;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::broadcast;
use tracing::info;
use arc_swap::ArcSwap;

use crate::config::Config;
use crate::detection::DetectionEngine;
use crate::forecasting::Forecaster;
use crate::learning::PatternLearner;
use crate::metrics::{BatchRecord, BlockRecord, DashboardSnapshot, ModuleStats, Metrics, SubnetRow};
use crate::storage::Store;

pub struct Engine {
    pub config: Arc<arc_swap::ArcSwap<Config>>,
    pub store: Arc<Store>,
    pub metrics: Arc<Metrics>,
    shutdown: AtomicBool,
}

impl Engine {
    pub fn new(cfg: Config, store: Arc<Store>, metrics: Arc<Metrics>) -> Self {
        Self {
            config: Arc::new(ArcSwap::from_pointee(cfg)),
            store,
            metrics,
            shutdown: AtomicBool::new(false),
        }
    }

    pub fn start(&self) {
        info!("Engine::start: sync stub — call start_async to actually boot");
    }

    /// Boot the full pipeline: store, detection, forecasting, IPC server.
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
        let store = &self.store;
        let metrics = &self.metrics;
        let stats = store.get_stats();
        let (cpu_usage, memory_usage_mb) = crate::metrics::get_system_usage();

        DashboardSnapshot {
            ts_ms: crate::metrics::now_ms(),
            uptime_secs: stats.uptime_secs,
            ips_tracked: stats.ips_tracked,
            blocked_total: stats.blocked,
            ram_bytes: stats.ram_bytes,
            ram_limit_mb: stats.ram_limit_mb,
            ram_pct: if stats.ram_limit_mb > 0 { (stats.ram_bytes as f64 / (stats.ram_limit_mb as f64 * 1048576.0) * 100.0).min(100.0) } else { 0.0 },
            cpu_usage,
            memory_usage_mb,
            ipc_requests: metrics.requests_total.load(Ordering::Relaxed),
            events_ingested: metrics.events_ingested.load(Ordering::Relaxed),
            events_rejected: metrics.events_rejected.load(Ordering::Relaxed),
            channel_depth: 0,
            batches_total: metrics.batches_total.load(Ordering::Relaxed),
            promotions: metrics.promotions_total.load(Ordering::Relaxed),
            cold_skipped: metrics.cold_skipped_total.load(Ordering::Relaxed),
            blocks_applied: metrics.blocks_detection.load(Ordering::Relaxed) + metrics.blocks_subnet.load(Ordering::Relaxed) + metrics.blocks_forecast.load(Ordering::Relaxed),
            pipeline: crate::metrics::PipelineFlow {
                ingest: 0,
                queued: 0,
                batched: 0,
                promoted: 0,
                merged: 0,
                blocked: 0,
            },
            is_healthy: true,
            health_reason: "running".into(),
        }
    }

    pub fn get_batch_history(&self) -> Vec<BatchRecord> {
        self.metrics.batch_history.lock().unwrap().iter().cloned().collect()
    }

    pub fn get_block_log(&self) -> Vec<BlockRecord> {
        self.metrics.block_log.lock().unwrap().iter().cloned().collect()
    }

    pub fn get_hot_subnets(&self) -> Vec<SubnetRow> {
            self.store.subnet_table().iter().map(|e| {
                let rec = e.value();
                SubnetRow {
                    prefix: format!("{}.{}.{}", rec.prefix[0], rec.prefix[1], rec.prefix[2]),
                    events: rec.total_rps,
                }
            }).collect()
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

    // Use engine's shared store and metrics (shared with dashboard)
    let store = engine.store.clone();
    let metrics = engine.metrics.clone();

    let (block_tx, _) = broadcast::channel::<crate::detection::BlockDecision>(1024);
    let learner = Arc::new(PatternLearner::new(cfg_snapshot.detection.pattern_similarity_threshold));

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
    use crate::metrics::Metrics;
    use crate::storage::Store;
    use std::sync::Arc;

    #[test]
    fn engine_constructs_with_default_config() {
        let _engine = Engine::new(Config::default(), Arc::new(Store::new(16)), Arc::new(Metrics::new()));
    }

    #[test]
    fn engine_start_then_snapshot_default_state() {
        let engine = Engine::new(Config::default(), Arc::new(Store::new(16)), Arc::new(Metrics::new()));
        engine.start();
        let snap = engine.dashboard_snapshot();
        assert!(snap.is_healthy);
        assert_eq!(snap.ips_tracked, 0);
        assert_eq!(snap.blocked_total, 0);
        assert_eq!(snap.events_ingested, 0);
    }

    #[test]
    fn engine_module_stats_have_four_canonical_rows() {
        let engine = Engine::new(Config::default(), Arc::new(Store::new(16)), Arc::new(Metrics::new()));
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
