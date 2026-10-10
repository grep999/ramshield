use arc_swap::ArcSwap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{mpsc, watch};
use tracing::info;

pub mod checkpoint;
pub mod native;
pub mod rss;
pub mod synproxy;
pub mod upstream;
pub mod waf;

mod boot;

use boot::boot_pipeline;

use crate::config::Config;
use crate::detection::DetectionEngine;
use crate::enforcement::{EnforcementService, StubXdpApplier, XdpApplier};
use crate::engine::checkpoint::{
    SnapshotRestore, build_snapshot, load_snapshot, restore_from_snapshot, snapshot_path,
    write_snapshot,
};
use crate::forecasting::Forecaster;
use crate::metrics::{
    BatchRecord, BlockRecord, DashboardSnapshot, Metrics, ModuleStats, SubnetRow,
};
use crate::storage::Store;
use ramshield_enforcement::{replay_wal_cidrs_seeded, replay_wal_into_store_seeded};
#[cfg(feature = "mesh")]
use ramshield_mesh::{MeshHandle, aworset::AworsetBlocklist};
use ramshield_storage::{
    checkpoint_shared::{CheckpointShared, CheckpointState, CidrSnapshot},
    wal::Wal,
};
use ramshield_types::EnforceCommand;

pub struct Engine {
    pub config: Arc<arc_swap::ArcSwap<Config>>,
    pub store: Arc<Store>,
    pub metrics: Arc<Metrics>,
    shutdown: Arc<AtomicBool>,
    enforcement_tx: mpsc::Sender<EnforceCommand>,
    enforcement_rx: std::sync::Mutex<Option<mpsc::Receiver<EnforceCommand>>>,
    /// True only when the kernel XDP dataplane is loaded and attached. False
    /// for StubXdpApplier (degraded mode: in-band enforcement only). Read by
    /// `dashboard_snapshot()` so the UI can surface a "XDP inactive" chip.
    xdp_active: Arc<AtomicBool>,
    /// False until `boot_pipeline` returns Ok. Process-alive ≠ pipeline-ready.
    pipeline_ready: Arc<AtomicBool>,
    /// Set when `boot_pipeline` returns Err. `/healthz` stays 503.
    pipeline_failed: Arc<AtomicBool>,
    /// Shared depth counter for IPC event channel.
    /// Watch channel for async shutdown signaling (replaces AtomicBool polling).
    shutdown_tx: watch::Sender<bool>,
    /// F9: set by boot_pipeline so main can JOIN the batch/subnet threads
    /// (each final-flushes pre_aggs on exit) instead of sleeping blind 5s.
    detection: std::sync::Mutex<Option<Arc<crate::detection::DetectionEngine>>>,
    /// Startup signal: boot_pipeline sends Ok(()) on success, Err(err) on failure.
    /// main blocks on this to learn whether the process should start serving or exit.
    /// oneshot: sync send (usable from rt.block_on), async recv. Exactly-once.
    startup_tx: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<std::io::Result<()>>>>,
    startup_rx: std::sync::Mutex<Option<tokio::sync::oneshot::Receiver<std::io::Result<()>>>>,
}

impl Engine {
    pub fn new(cfg: Config, store: Arc<Store>, metrics: Arc<Metrics>) -> Self {
        let (enforcement_tx, enforcement_rx) = mpsc::channel(8192);
        let (shutdown_tx, _) = watch::channel(false);
        let (startup_tx, startup_rx) = tokio::sync::oneshot::channel::<std::io::Result<()>>();
        Self {
            config: Arc::new(ArcSwap::from_pointee(cfg)),
            store,
            metrics,
            shutdown: Arc::new(AtomicBool::new(false)),
            detection: std::sync::Mutex::new(None),
            enforcement_tx,
            enforcement_rx: std::sync::Mutex::new(Some(enforcement_rx)),
            xdp_active: Arc::new(AtomicBool::new(false)),
            pipeline_ready: Arc::new(AtomicBool::new(false)),
            pipeline_failed: Arc::new(AtomicBool::new(false)),
            shutdown_tx,
            startup_tx: std::sync::Mutex::new(Some(startup_tx)),
            startup_rx: std::sync::Mutex::new(Some(startup_rx)),
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
                    let result = boot_pipeline(self.clone()).await;
                    match result {
                        Ok(()) => {
                            self.pipeline_ready.store(true, Ordering::Release);
                        }
                        Err(ref e) => {
                            tracing::error!("pipeline: {}", e);
                            self.pipeline_failed.store(true, Ordering::Release);
                        }
                    }
                    // Send startup result so main can await readiness/failure.
                    // oneshot::send is sync (no future to await); take() makes it exactly-once.
                    let mut tx_guard = self.startup_tx.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(tx) = tx_guard.take() {
                        // oneshot::send is sync — stores the value immediately.
                        // Receiver::await picks it up in wait_startup.
                        tx.send(result).ok();
                    }
                });
            })
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        let _ = self.shutdown_tx.send(true);
    }

    pub fn shutdown_rx(&self) -> watch::Receiver<bool> {
        self.shutdown_tx.subscribe()
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }

    /// Block caller until boot_pipeline signals Ok or Err.
    /// Call once, after start_async. Returns the pipeline result.
    pub async fn wait_startup(&self) -> std::io::Result<()> {
        let rx = self
            .startup_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .ok_or_else(|| {
                std::io::Error::other("wait_startup called multiple times or never started")
            })?;
        rx.await
            .map_err(|_| std::io::Error::other("startup channel closed before pipeline finished"))?
    }

    /// Tests construct Engine without boot_pipeline. Production never calls this.
    #[cfg(test)]
    pub fn mark_pipeline_ready_for_test(&self) {
        self.pipeline_ready.store(true, Ordering::Release);
    }

    /// F9: join detection batch/subnet threads with a grace cap. Call after
    /// shutdown() — replaces the fixed sleep in main.
    pub fn join_workers(&self, grace: std::time::Duration) {
        // no-unwrap gate (CI lint-no-unwrap scans src/): poisoning must not
        // abort shutdown — a panicked holder still left a valid Option<Arc>
        // behind, and join is exactly what shutdown needs to do then.
        if let Some(det) = self
            .detection
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            det.join_workers(grace);
        }
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
        let channel_depth = self
            .detection
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map_or(0, |d| d.event_queue_depth());
        metrics.set_channel_depth(channel_depth);

        let xdp_configured = self.config.load().xdp.enabled;
        let xdp_active = self.xdp_active.load(Ordering::Acquire);
        let allow_fb = self.config.load().xdp.allow_inband_fallback;
        let pipeline_ok = self.pipeline_ready.load(Ordering::Acquire)
            && !self.pipeline_failed.load(Ordering::Acquire);
        let xdp_projection_stale = metrics.xdp_projection_stale.load(Ordering::Acquire) != 0;
        let protection_state = if self.is_shutting_down() {
            crate::metrics::ProtectionState::Stopping
        } else if self.pipeline_failed.load(Ordering::Acquire) {
            crate::metrics::ProtectionState::Failed
        } else if !self.pipeline_ready.load(Ordering::Acquire) {
            crate::metrics::ProtectionState::Starting
        } else if xdp_configured && !xdp_active && !allow_fb {
            crate::metrics::ProtectionState::Failed
        } else if xdp_configured && !xdp_active && allow_fb {
            crate::metrics::ProtectionState::Degraded
        } else if xdp_projection_stale && !allow_fb {
            crate::metrics::ProtectionState::Failed
        } else if xdp_projection_stale && allow_fb {
            crate::metrics::ProtectionState::Degraded
        } else if xdp_configured && xdp_active && pipeline_ok && ram_pct < 95.0 {
            crate::metrics::ProtectionState::Protected
        } else if ram_pct >= 95.0 {
            crate::metrics::ProtectionState::Degraded
        } else {
            crate::metrics::ProtectionState::Protected
        };
        let is_healthy = !self.is_shutting_down()
            && ram_pct < 95.0
            && pipeline_ok
            && !(xdp_configured && !xdp_active && !allow_fb)
            && !xdp_projection_stale;

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
            frames_rejected_total: metrics.frames_rejected.load(Ordering::Relaxed),
            channel_depth,
            events_shed: metrics.events_shed.load(Ordering::Relaxed),
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
            wal_lsn: self.metrics.wal_lsn.load(Ordering::Relaxed),
            pending_expirations: self.metrics.pending_expirations.load(Ordering::Relaxed),
            xdp_apply_failures: self.metrics.xdp_apply_failures.load(Ordering::Relaxed),
            xdp_projection_stale,
            is_healthy,
            health_reason: if self.is_shutting_down() {
                "shutting down".into()
            } else if self.pipeline_failed.load(Ordering::Acquire) {
                "pipeline failed".into()
            } else if !self.pipeline_ready.load(Ordering::Acquire) {
                "starting".into()
            } else if xdp_configured && !xdp_active && !allow_fb {
                "xdp inactive".into()
            } else if xdp_configured && !xdp_active && allow_fb {
                "xdp degraded".into()
            } else if xdp_projection_stale {
                "xdp reconciliation stale".into()
            } else if ram_pct >= 95.0 {
                "ram pressure".into()
            } else {
                "running".into()
            },
            xdp_active,
            xdp_configured,
            protection_state,
        }
    }

    pub fn get_batch_history(&self) -> Vec<BatchRecord> {
        self.metrics.get_batch_history()
    }

    /// Pre-serialized JSON variants for the dashboard polling endpoints
    /// (RAM-for-CPU item 16): unchanged data costs one Arc clone per poll.
    pub fn get_batch_history_json(&self) -> std::sync::Arc<str> {
        self.metrics.get_batch_history_json()
    }

    pub fn get_block_log_json(&self) -> std::sync::Arc<str> {
        self.metrics.get_block_log_json()
    }

    pub fn get_block_log(&self) -> Vec<BlockRecord> {
        self.metrics.get_block_log()
    }

    pub fn get_active_blocks(&self) -> Vec<BlockRecord> {
        let mut blocks: Vec<BlockRecord> = self
            .store
            .get_all_blocked_ips()
            .into_iter()
            .filter_map(|ip| {
                let value = self.store.get(&ip)?;
                let ramshield_storage::Value::IpRecord(record) = value else {
                    return None;
                };
                let ramshield_storage::BlockState::Blocked {
                    ref reason,
                    since_ns,
                } = record.block_state
                else {
                    return None;
                };
                Some(BlockRecord {
                    ts_ms: since_ns / 1_000_000,
                    ip: ip.to_string(),
                    reason: reason.as_str().to_string(),
                    module: "enforcement".to_string(),
                })
            })
            .collect();
        blocks.sort_unstable_by_key(|block| std::cmp::Reverse(block.ts_ms));
        blocks
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
                // Task 1: IpNetwork Display = family-complete CIDR
                // ("198.51.100.0/24" / "2001:db8::/64"); the old hand-rolled
                // "{}.{}.{}" rendered v6 as garbage and had no /24 suffix.
                SubnetRow {
                    prefix: rec.network.to_string(),
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
        let channel_depth = self
            .detection
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map_or(0, |d| d.event_queue_depth());
        self.metrics.set_channel_depth(channel_depth);
        self.metrics.get_module_stats_data(
            stats.uptime_secs,
            ingested,
            channel_depth,
            stats.ips_tracked,
            stats.ram_bytes,
            stats.ram_limit_mb,
        )
    }

    /// Send a manual mitigation command from the dashboard console.
    /// Uses try_send to avoid blocking on a full enforcement channel.
    pub fn send_mitigation(&self, cmd: EnforceCommand) -> Result<(), String> {
        self.enforcement_tx
            .try_send(cmd)
            .map_err(|e| format!("enforcement channel full or closed: {}", e))
    }
}

#[cfg(test)]
#[path = "tests/startup.rs"]
mod startup_tests;
