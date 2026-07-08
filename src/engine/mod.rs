use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;
use crate::config::{Config, ConfigHandle};
use crate::detection::DetectionEngine;
use crate::error::RamShieldError;
use crate::ipc::IpcServer;
use crate::metrics::Metrics;
use crate::storage::{StorageEngine, Store};
use crate::forecasting::ForecastingEngine;
use crate::alerting::AlertingEngine;
use crate::dashboard::Dashboard;
use crate::metrics::DashboardSnapshot;

pub struct Engine {
    config:              ConfigHandle,
    metrics:             Arc<Metrics>,
    detection_engine:    Arc<DetectionEngine>,
    storage_engine:      Arc<StorageEngine>,
    forecasting_engine:  Arc<ForecastingEngine>,
    alerting_engine:     Arc<AlertingEngine>,
    shutdown_tx:         broadcast::Sender<()>,
    is_shutting_down:    Arc<tokio::sync::Mutex<bool>>,
    worker_handles:      Arc<tokio::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

impl Engine {
    pub fn new(config: Config) -> Self {
        let metrics = Arc::new(Metrics::new());
        let (shutdown_tx, _) = broadcast::channel(1);
        let config_handle = Arc::new(parking_lot::RwLock::new(config.clone()));

        let main_store = Arc::new(Store::new(config.engine.ram_limit_mb * 1024 * 1024));

        let storage_engine = Arc::new(StorageEngine::new(config.storage.clone(), metrics.clone(), main_store.clone()));
        let detection_engine = Arc::new(DetectionEngine::new(config.detection.clone(), metrics.clone(), main_store.clone()));
        let forecasting_engine = Arc::new(ForecastingEngine::new(config.forecasting.clone(), metrics.clone(), main_store.clone()));
        let alerting_engine = Arc::new(AlertingEngine::new(config.alerting.clone(), metrics.clone()));

        Self {
            config: config_handle,
            metrics,
            detection_engine,
            storage_engine,
            forecasting_engine,
            alerting_engine,
            shutdown_tx,
            is_shutting_down: Arc::new(tokio::sync::Mutex::new(false)),
            worker_handles: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }

    pub fn config_handle(&self) -> ConfigHandle {
        self.config.clone()
    }

    pub fn dashboard_snapshot(&self) -> DashboardSnapshot {
        let cfg = self.config.read();
        crate::metrics::build_snapshot(
            &self.metrics,
            self.storage_engine.store.ips_tracked(),
            self.storage_engine.store.ram_bytes(),
            cfg.engine.ram_limit_mb,
            self.storage_engine.store.channel_depth(),
            self.storage_engine.store.hot_subnets(),
        )
    }

    pub async fn start(&self) -> Result<(), RamShieldError> {
        info!("Starting RamShield engine...");

        let mut handles = self.worker_handles.lock().await;

        // Storage engine
        let storage_engine = self.storage_engine.clone();
        let shutdown_rx = self.shutdown_tx.subscribe();
        handles.push(tokio::spawn(async move {
            if let Err(e) = storage_engine.start(shutdown_rx).await {
                tracing::error!("Storage engine error: {}", e);
            }
        }));

        // Detection engine
        let detection_engine = self.detection_engine.clone();
        let shutdown_rx = self.shutdown_tx.subscribe();
        handles.push(tokio::spawn(async move {
            if let Err(e) = detection_engine.start(shutdown_rx).await {
                tracing::error!("Detection engine error: {}", e);
            }
        }));

        // Forecasting engine
        let forecasting_engine = self.forecasting_engine.clone();
        let shutdown_rx = self.shutdown_tx.subscribe();
        handles.push(tokio::spawn(async move {
            if let Err(e) = forecasting_engine.start(shutdown_rx).await {
                tracing::error!("Forecasting engine error: {}", e);
            }
        }));

        // Alerting engine
        let alerting_engine = self.alerting_engine.clone();
        let alert_shutdown_rx = self.shutdown_tx.subscribe();
        handles.push(tokio::spawn(async move {
            if let Err(e) = alerting_engine.start(alert_shutdown_rx).await {
                tracing::error!("Alerting engine error: {}", e);
            }
        }));

        // IPC server
        {
            let cfg = self.config.read();
            if cfg.ipc.enabled {
                let ipc_server = IpcServer::new(cfg.ipc.clone(), self.detection_engine.clone(), self.metrics.clone(), self.storage_engine.store.clone());
                let ipc_shutdown_rx = self.shutdown_tx.subscribe();
                handles.push(tokio::spawn(async move {
                    if let Err(e) = ipc_server.start(ipc_shutdown_rx).await {
                        tracing::error!("IPC server error: {}", e);
                    }
                }));
            }
        }

        // Dashboard
        {
            let cfg = self.config.read();
            if cfg.dashboard.enabled {
                let dashboard = Dashboard::new(cfg.dashboard.clone(), self.metrics.clone(), self.config.clone(), self.storage_engine.store.clone());
                let dashboard_shutdown_rx = self.shutdown_tx.subscribe();
                handles.push(tokio::spawn(async move {
                    if let Err(e) = dashboard.start_server(dashboard_shutdown_rx).await {
                        tracing::error!("Dashboard server error: {}", e);
                    }
                }));
            }
        }

        let worker_threads = self.config.read().engine.worker_threads;
        info!(
            "RamShield engine started successfully with {} worker threads",
            worker_threads
        );
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), RamShieldError> {
        info!("Shutting down RamShield engine...");
        *self.is_shutting_down.lock().await = true;
        self.detection_engine.shutdown_flag().store(true, std::sync::atomic::Ordering::Release);

        self.shutdown_tx.send(())
            .map_err(|e| RamShieldError::GenericError(format!("Failed to send shutdown signal: {}", e)))?;

        let mut handles = self.worker_handles.lock().await;
        for handle in handles.drain(..) {
            let _ = handle.await;
        }

        info!("RamShield engine shutdown complete.");
        Ok(())
    }

    pub async fn is_shutting_down(&self) -> bool {
        *self.is_shutting_down.lock().await
    }
}