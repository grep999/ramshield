use std::sync::Arc;
use tokio::sync::broadcast;
use anyhow::Result;
use tracing::info;
use axum::{
    Router,
    routing::get,
    extract::{State, Path},
    response::{Html, Json, Sse},
    http::StatusCode,
};
use futures::stream::{self, Stream};
use std::convert::Infallible;
use std::time::Duration;
use tokio::time::interval;
use tower_http::services::ServeDir;

use crate::config::{Config, ConfigHandle, EngineConfig, DetectionConfig, StorageConfig, IpcConfig, ForecastingConfig, DashboardConfig as DashCfg, AlertingConfig};
use crate::metrics::{Metrics, DashboardSnapshot, ModuleStats, BatchRecord, BlockRecord};
use crate::error::RamShieldError;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ConfigPatch {
    #[serde(default)]
    pub engine: Option<EngineConfig>,
    #[serde(default)]
    pub detection: Option<DetectionConfig>,
    #[serde(default)]
    pub ipc: Option<IpcConfig>,
    #[serde(default)]
    pub forecasting: Option<ForecastingConfig>,
    #[serde(default)]
    pub dashboard: Option<DashCfg>,
    #[serde(default)]
    pub alerting: Option<AlertingConfig>,
}

#[derive(Clone)]
pub struct DashState {
    pub metrics: Arc<Metrics>,
    pub config: ConfigHandle,
    pub store: Arc<crate::storage::Store>,
}

pub struct Dashboard {
    config: DashCfg,
    state: DashState,
}

impl Dashboard {
    pub fn new(config: DashCfg, metrics: Arc<Metrics>, app_config: ConfigHandle, store: Arc<crate::storage::Store>) -> Self {
        Self {
            config,
            state: DashState {
                metrics,
                config: app_config,
                store,
            },
        }
    }

    pub async fn start_server(&self, mut shutdown_rx: broadcast::Receiver<()>) -> Result<(), RamShieldError> {
        let app = self.create_router();
        let addr = self.config.http_addr.clone();
        info!("Dashboard: Starting server on {}", addr);

        let listener = tokio::net::TcpListener::bind(&addr).await
            .map_err(|e| RamShieldError::GenericError(format!("Failed to bind to {}: {}", addr, e)))?;

        let server = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.recv().await;
                info!("Dashboard: Shutdown signal received");
            });

        server.await
            .map_err(|e| RamShieldError::GenericError(format!("Dashboard server error: {}", e)))?;

        Ok(())
    }

    fn create_router(&self) -> Router {
        let state = self.state.clone();
        let static_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/dashboard/static");

        Router::new()
            .route("/", get(serve_index))
            .route("/healthz", get(health_check))
            .route("/api/stats", get(get_stats))
            .route("/api/metrics", get(get_metrics))
            .route("/api/events/batches", get(get_batch_history))
            .route("/api/events/blocks", get(get_block_log))
            .route("/api/modules", get(get_module_stats))
            .route("/api/sse", get(sse_handler))
            .route("/sse", get(sse_handler))
            .route("/api/config", get(get_config).post(update_config))
            .route("/api/config/:section", get(get_config_section).post(update_config_section))
            .route("/api/export/stats", get(export_stats))
            .route("/api/export/blocks", get(export_blocks))
            .route("/stats", get(get_stats))
            .route("/metrics", get(get_metrics))
            .route("/events/batches", get(get_batch_history))
            .route("/events/blocks", get(get_block_log))
            .route("/modules", get(get_module_stats))
            .nest_service("/static", ServeDir::new(static_dir))
            .with_state(state)
    }
}

async fn serve_index() -> impl axum::response::IntoResponse {
    (
        [
            ("cache-control", "no-cache, no-store, must-revalidate"),
            ("pragma", "no-cache"),
            ("expires", "0"),
        ],
        Html(include_str!("static/index.html")),
    )
}

async fn health_check(State(st): State<DashState>) -> Json<serde_json::Value> {
    let snapshot = crate::metrics::build_snapshot(
        &st.metrics,
        st.store.ips_tracked(),
        st.store.ram_bytes(),
        st.store.ram_limit_mb(),
        st.store.channel_depth(),
        st.store.hot_subnets(),
    );
    Json(serde_json::json!({
        "status": if snapshot.is_healthy { "healthy" } else { "degraded" },
        "service": "ramshield-dashboard",
        "uptime_secs": snapshot.uptime_secs,
        "ips_tracked": snapshot.ips_tracked,
        "events_ingested": snapshot.events_ingested,
        "blocks_active": st.store.blocked_count(),
    }))
}

async fn get_stats(State(st): State<DashState>) -> Json<DashboardSnapshot> {
    let snapshot = crate::metrics::build_snapshot(
        &st.metrics,
        st.store.ips_tracked(),
        st.store.ram_bytes(),
        st.store.ram_limit_mb(),
        st.store.channel_depth(),
        st.store.hot_subnets(),
    );
    Json(snapshot)
}

async fn get_metrics(State(st): State<DashState>) -> Json<serde_json::Value> {
    let snapshot = crate::metrics::build_snapshot(
        &st.metrics,
        st.store.ips_tracked(),
        st.store.ram_bytes(),
        st.store.ram_limit_mb(),
        st.store.channel_depth(),
        st.store.hot_subnets(),
    );
    Json(serde_json::to_value(snapshot).unwrap_or_default())
}

async fn get_batch_history(State(st): State<DashState>) -> Json<Vec<BatchRecord>> {
    Json(st.metrics.get_batch_history())
}

async fn get_block_log(State(st): State<DashState>) -> Json<Vec<BlockRecord>> {
    Json(st.metrics.get_block_log())
}

async fn get_module_stats(State(st): State<DashState>) -> Json<Vec<ModuleStats>> {
    let snapshot_data = crate::metrics::build_snapshot(
        &st.metrics,
        st.store.ips_tracked(),
        st.store.ram_bytes(),
        st.store.ram_limit_mb(),
        st.store.channel_depth(),
        st.store.hot_subnets(),
    );
    let module_stats_data = snapshot_data.modules.clone();
    Json(module_stats_data)
}

async fn sse_handler(
    State(st): State<DashState>,
) -> Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>> {
    let st_clone = st.clone();
    let stream = stream::unfold(
        (st_clone, interval(Duration::from_secs(5))),
        |(st, mut interval)| async move {
            interval.tick().await;
            let snapshot = crate::metrics::build_snapshot(
                &st.metrics,
                st.store.ips_tracked(),
                st.store.ram_bytes(),
                st.store.ram_limit_mb(),
                st.store.channel_depth(),
                st.store.hot_subnets(),
            );
            let data = serde_json::to_string(&snapshot).unwrap_or_default();
            let event = axum::response::sse::Event::default().data(data);
            Some((Ok(event), (st, interval)))
        },
    );
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(15)))
}

async fn get_config(State(st): State<DashState>) -> Json<Config> {
    let cfg = st.config.read().clone();
    Json(cfg)
}

async fn export_stats(State(st): State<DashState>) -> Json<DashboardSnapshot> {
    let snapshot = crate::metrics::build_snapshot(
        &st.metrics,
        st.store.ips_tracked(),
        st.store.ram_bytes(),
        st.store.ram_limit_mb(),
        st.store.channel_depth(),
        st.store.hot_subnets(),
    );
    Json(snapshot)
}

async fn export_blocks(State(st): State<DashState>) -> Json<Vec<BlockRecord>> {
    Json(st.metrics.get_block_log())
}

async fn update_config(
    State(st): State<DashState>,
    Json(patch): Json<ConfigPatch>,
) -> Result<Json<Config>, (StatusCode, String)> {
    let mut cfg = st.config.write();
    if let Some(v) = patch.engine { cfg.engine = v; }
    if let Some(v) = patch.detection { cfg.detection = v; }
    if let Some(v) = patch.ipc { cfg.ipc = v; }
    if let Some(v) = patch.forecasting { cfg.forecasting = v; }
    if let Some(v) = patch.dashboard { cfg.dashboard = v; }
    if let Some(v) = patch.alerting { cfg.alerting = v; }
    Ok(Json(cfg.clone()))
}

async fn get_config_section(
    State(st): State<DashState>,
    Path(section): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let cfg = st.config.read();
    let value = serde_json::to_value(&*cfg).unwrap_or_default();
    match value.get(&section) {
        Some(v) => Ok(Json(v.clone())),
        None => Err((StatusCode::NOT_FOUND, format!("Section '{}' not found", section))),
    }
}

async fn update_config_section(
    State(st): State<DashState>,
    Path(section): Path<String>,
    Json(patch): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut cfg = st.config.write();
    match section.as_str() {
        "engine" => {
            if let Ok(parsed) = serde_json::from_value::<EngineConfig>(patch.clone()) {
                cfg.engine = parsed;
            }
        }
        "detection" => {
            if let Ok(parsed) = serde_json::from_value::<DetectionConfig>(patch.clone()) {
                cfg.detection = parsed;
            }
        }
        "storage" => {
            if let Ok(parsed) = serde_json::from_value::<StorageConfig>(patch.clone()) {
                cfg.storage = parsed;
            }
        }
        "ipc" => {
            if let Ok(parsed) = serde_json::from_value::<IpcConfig>(patch.clone()) {
                cfg.ipc = parsed;
            }
        }
        "dashboard" => {
            if let Ok(parsed) = serde_json::from_value::<DashCfg>(patch.clone()) {
                cfg.dashboard = parsed;
            }
        }
        "forecasting" => {
            if let Ok(parsed) = serde_json::from_value::<ForecastingConfig>(patch.clone()) {
                cfg.forecasting = parsed;
            }
        }
        "alerting" => {
            if let Ok(parsed) = serde_json::from_value::<AlertingConfig>(patch.clone()) {
                cfg.alerting = parsed;
            }
        }
        _ => return Err((StatusCode::NOT_FOUND, format!("Section '{}' not found", section))),
    }
    let value = serde_json::to_value(&*cfg).unwrap_or_default();
    Ok(Json(value.get(&section).cloned().unwrap_or_default()))
}