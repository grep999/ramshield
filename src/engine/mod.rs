pub mod learning;

use arc_swap::ArcSwap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;
use tracing::info;

use crate::config::Config;
use crate::detection::DetectionEngine;
use crate::enforcement::{EnforcementService, StubXdpApplier, XdpApplier};
use crate::forecasting::Forecaster;
use crate::metrics::{
    BatchRecord, BlockRecord, DashboardSnapshot, Metrics, ModuleStats, SubnetRow,
};
use crate::storage::Store;
use ramshield_storage::wal::Wal;
use ramshield_types::EnforceCommand;

pub struct Engine {
    pub config: Arc<arc_swap::ArcSwap<Config>>,
    pub store: Arc<Store>,
    pub metrics: Arc<Metrics>,
    shutdown: AtomicBool,
    enforcement_tx: mpsc::Sender<EnforceCommand>,
    enforcement_rx: std::sync::Mutex<Option<mpsc::Receiver<EnforceCommand>>>,
}

impl Engine {
    pub fn new(cfg: Config, store: Arc<Store>, metrics: Arc<Metrics>) -> Self {
        let (enforcement_tx, enforcement_rx) = mpsc::channel(4096);
        Self {
            config: Arc::new(ArcSwap::from_pointee(cfg)),
            store,
            metrics,
            shutdown: AtomicBool::new(false),
            enforcement_tx,
            enforcement_rx: std::sync::Mutex::new(Some(enforcement_rx)),
        }
    }

    #[deprecated(
        since = "0.2.0",
        note = "no-op stub; use start_async() instead to actually boot the pipeline"
    )]
    pub fn start(&self) {
        info!("Engine::start: sync stub — call start_async to actually boot");
    }

    /// Boot the full pipeline: store, detection, forecasting, IPC server.
    pub fn start_async(self: Arc<Self>) -> std::io::Result<std::thread::JoinHandle<()>> {
        let _cfg = self.config.load();
        std::thread::Builder::new()
            .name("rs-engine".into())
            .spawn(move || {
                // Multi-thread: IPC accept loop serves every connection's
                // read/write on this runtime; current_thread starved under
                // attack load (5s read timeouts during subnet floods).
                let rt = match tokio::runtime::Builder::new_multi_thread()
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
        let (cpu_usage, total_ram_mb, memory_usage_mb) = crate::metrics::get_system_usage();

        let ram_pct = if stats.ram_limit_mb > 0 {
            (stats.ram_bytes as f64 / (stats.ram_limit_mb as f64 * 1048576.0) * 100.0).min(100.0)
        } else {
            0.0
        };
        let ingested = metrics.events_ingested.load(Ordering::Relaxed);
        let batches = metrics.batches_total.load(Ordering::Relaxed);
        let promotions = metrics.promotions_total.load(Ordering::Relaxed);
        let blocks_applied = metrics.blocks_detection.load(Ordering::Relaxed)
            + metrics.blocks_subnet.load(Ordering::Relaxed)
            + metrics.blocks_forecast.load(Ordering::Relaxed);
        let channel_depth = 0usize;
        // ponytail: tokio::sync::mpsc::Sender has no len(). Real depth
        // requires an AtomicU64 gauge in IPC send + enforcement receive
        // paths. Add when dashboard_channel_depth becomes a real SLO target.

        DashboardSnapshot {
            ts_ms: crate::metrics::now_ms(),
            uptime_secs: stats.uptime_secs,
            ips_tracked: stats.ips_tracked,
            blocked_total: stats.blocked,
            ram_bytes: stats.ram_bytes,
            ram_limit_mb: stats.ram_limit_mb,
            ram_pct,
            cpu_usage,
            memory_usage_mb,
            total_ram_mb,
            ipc_requests: metrics.requests_total.load(Ordering::Relaxed),
            events_ingested: ingested,
            events_rejected: metrics.events_rejected.load(Ordering::Relaxed),
            channel_depth,
            batches_total: batches,
            promotions,
            cold_skipped: metrics.cold_skipped_total.load(Ordering::Relaxed),
            blocks_applied,
            pipeline: crate::metrics::PipelineFlow {
                ingest: ingested,
                queued: channel_depth as u64,
                batched: batches,
                promoted: promotions,
                merged: stats.ips_tracked as u64,
                blocked: blocks_applied,
            },
            is_healthy: !self.is_shutting_down(),
            health_reason: if self.is_shutting_down() { "shutting down".into() } else { "running".into() },
        }
    }

    pub fn get_batch_history(&self) -> Vec<BatchRecord> {
        self.metrics.get_batch_history()
    }

    pub fn get_block_log(&self) -> Vec<BlockRecord> {
        self.metrics.get_block_log()
    }

    pub fn get_hot_subnets(&self) -> Vec<SubnetRow> {
        // ponytail: select_nth_unstable finds the 100th in O(n) — old sort was
        // O(n log n) when only top-100 is kept. Strings still allocate; the
        // real win is avoiding the sort.
        if self.store.subnet_table().is_empty() {
            return Vec::new();
        }
        let mut rows: Vec<SubnetRow> = self
            .store
            .subnet_table()
            .iter()
            .map(|e| {
                let rec = e.value();
                // ponytail: pre-sized — IPv4 /24 never exceeds 15 chars.
                let mut prefix = String::with_capacity(15);
                let _ = std::fmt::Write::write_fmt(
                    &mut prefix,
                    format_args!(
                        "{}.{}.{}",
                        rec.prefix[0], rec.prefix[1], rec.prefix[2]
                    ),
                );
                SubnetRow {
                    prefix,
                    events: rec.total_rps,
                }
            })
            .collect();
        if rows.len() > 100 {
            rows.select_nth_unstable_by_key(100, |r| std::cmp::Reverse(r.events));
            rows.truncate(100);
        } else {
            rows.sort_by_key(|r| std::cmp::Reverse(r.events));
        }
        rows
    }

    pub fn get_module_stats(&self) -> Vec<ModuleStats> {
        let stats = self.store.get_stats();
        let ingested = self.metrics.events_ingested.load(Ordering::Relaxed);
        let channel_depth = 0usize;
        // ponytail: tokio::sync::mpsc::Sender has no len(). Real depth
        // requires an AtomicU64 gauge in IPC send + enforcement receive
        // paths. Add when dashboard_channel_depth becomes a real SLO target.
        self.metrics.get_module_stats_data(
            stats.uptime_secs,
            ingested,
            channel_depth,
            stats.ips_tracked,
            stats.ram_bytes,
            stats.ram_limit_mb,
        )
    }
}

async fn boot_pipeline(engine: Arc<Engine>) -> std::io::Result<()> {
    let cfg_arc = engine.config.load(); // Arc<Config>
    let cfg_snapshot = cfg_arc.as_ref().clone(); // owned Config clone
    let cfg_handle = cfg_snapshot.clone().into_handle(); // ConfigHandle

    // Use engine's shared store and metrics (shared with dashboard)
    let store = engine.store.clone();
    let metrics = engine.metrics.clone();

    // Take the enforcement receiver ONCE
    let enforcement_rx = engine
        .enforcement_rx
        .lock()
        .map_err(|_| std::io::Error::other("enforcement receiver lock poisoned"))?
        .take()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "enforcement service already started",
            )
        })?;
    // The service follows the engine shutdown flag through a dedicated watcher.
    let enforcement_shutdown = Arc::new(AtomicBool::new(false));
    // Dataplane: real aya XDP when [xdp].enabled, else in-band-only stub.
    // Load failure is not fatal — daemon runs degraded (in-band enforcement
    // still works) and logs loudly. See plans/2026-08-22_enforcement-production.md.
    let xdp_box: Box<dyn XdpApplier> = if cfg_snapshot.xdp.enabled {
        #[cfg(feature = "xdp")]
        {
            let mut applier = crate::enforcement::xdp::AyaXdpApplier::new(
                &cfg_snapshot.xdp.interface,
                &cfg_snapshot.xdp.mode,
            );
            match applier.load_and_attach() {
                Ok(()) => {
                    tracing::info!(iface = %cfg_snapshot.xdp.interface, mode = %cfg_snapshot.xdp.mode, "XDP dataplane active");
                    Box::new(applier)
                }
                Err(e) => {
                    tracing::error!(
                        "XDP load/attach failed ({}): {} — falling back to in-band enforcement",
                        cfg_snapshot.xdp.interface,
                        e
                    );
                    Box::new(StubXdpApplier)
                }
            }
        }
        #[cfg(not(feature = "xdp"))]
        {
            tracing::warn!(
                "[xdp].enabled=true but binary built without 'xdp' feature — in-band enforcement only"
            );
            Box::new(StubXdpApplier)
        }
    } else {
        Box::new(StubXdpApplier)
    };
    let mut enforcement = EnforcementService::new(
        store.clone(),
        metrics.clone(),
        xdp_box,
        enforcement_shutdown.clone(),
    );
    // Crash-durable block state: open WAL, replay live blocks into the store
    // BEFORE run() reconciles store → XDP.
    if cfg_snapshot.wal.enabled {
        match Wal::open(
            &cfg_snapshot.wal.dir,
            cfg_snapshot.wal.compress,
            cfg_snapshot.wal.durability,
            cfg_snapshot.wal.seg_max_bytes,
            cfg_snapshot.wal.retention_max_bytes,
        ) {
            Ok(wal) => {
                let wal = Arc::new(wal);
                match ramshield_enforcement::replay_wal_into_store(&store, &wal) {
                    Ok(n) => tracing::info!(
                        "WAL enabled at {} — {} blocks restored",
                        cfg_snapshot.wal.dir,
                        n
                    ),
                    Err(e) => {
                        tracing::error!("WAL replay failed: {} — starting with empty block set", e)
                    }
                }
                enforcement = enforcement.with_wal(wal);
            }
            Err(e) => {
                tracing::error!(
                    "WAL open failed ({}): {} — running without durability",
                    cfg_snapshot.wal.dir,
                    e
                );
            }
        }
    }
    let engine_for_shutdown = engine.clone();
    tokio::spawn(async move {
        loop {
            if engine_for_shutdown.is_shutting_down() {
                enforcement_shutdown.store(true, Ordering::Release);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });
    tokio::spawn(async move {
        if let Err(e) = enforcement.run(enforcement_rx).await {
            tracing::error!("enforcement service: {}", e);
        }
    });

    let detection = Arc::new(DetectionEngine::new(
        store.clone(),
        cfg_handle.clone(),
        engine.enforcement_tx.clone(),
        metrics.clone(),
        Arc::new(AtomicBool::new(false)),
    ));
    let event_tx = detection.event_sender();
    detection
        .clone()
        .spawn_workers(cfg_snapshot.engine.worker_threads);

    let forecaster = Arc::new(Forecaster::new(
        store.clone(),
        cfg_snapshot.forecasting.clone(),
        engine.enforcement_tx.clone(),
        metrics.clone(),
    ));
    tokio::spawn(async move { forecaster.run().await });

    let server = crate::ipc::server::IpcServer::bind(
        &cfg_snapshot,
        engine.clone(),
        event_tx,
        store,
        engine.enforcement_tx.clone(),
    )
    .await?;
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
        let _engine = Engine::new(
            Config::default(),
            Arc::new(Store::new(16)),
            Arc::new(Metrics::new()),
        );
    }

    #[test]
    fn engine_start_then_snapshot_default_state() {
        let engine = Engine::new(
            Config::default(),
            Arc::new(Store::new(16)),
            Arc::new(Metrics::new()),
        );
        #[allow(deprecated)]
        engine.start();
        let snap = engine.dashboard_snapshot();
        assert!(snap.is_healthy);
        assert_eq!(snap.ips_tracked, 0);
        assert_eq!(snap.blocked_total, 0);
        assert_eq!(snap.events_ingested, 0);
    }

    #[test]
    fn engine_module_stats_have_four_canonical_rows() {
        let engine = Engine::new(
            Config::default(),
            Arc::new(Store::new(16)),
            Arc::new(Metrics::new()),
        );
        #[allow(deprecated)]
        engine.start();
        let stats = engine.get_module_stats();
        assert_eq!(stats.len(), 4);
        let labels: Vec<&str> = stats.iter().map(|m| m.label.as_str()).collect();
        assert!(labels.contains(&"IPC"));
        assert!(labels.contains(&"Detection"));
        assert!(labels.contains(&"Forecasting"));
        assert!(labels.contains(&"Storage"));
    }

    #[test]
    fn engine_snapshot_unhealthy_when_shutting_down() {
        let engine = Engine::new(
            Config::default(),
            Arc::new(Store::new(16)),
            Arc::new(Metrics::new()),
        );
        engine.shutdown();
        let snap = engine.dashboard_snapshot();
        assert!(!snap.is_healthy);
        assert_eq!(snap.health_reason, "shutting down");
    }
}
