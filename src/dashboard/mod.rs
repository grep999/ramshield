use crate::config::Config;
use crate::engine::Engine;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, Json},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::metrics::{BatchRecord, BlockRecord, DashboardSnapshot, ModuleStats, SubnetRow};

pub async fn serve(engine: Arc<Engine>, addr: &str) -> Result<(), String> {
    let app = Router::new()
        .route("/", get(index))
        .route("/healthz", get(api_healthz))
        .route("/api/snapshot", get(api_snapshot))
        .route("/api/history/batches", get(api_history_batches))
        .route("/api/history/blocks", get(api_history_blocks))
        .route("/api/traffic/subnets", get(api_traffic_subnets))
        .route("/api/status/modules", get(api_status_modules))
        .route("/api/config", get(api_get_config).post(api_set_config))
        .with_state(engine)
        .layer(CorsLayer::permissive());


    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| e.to_string())?;
    info!("Dashboard http://{}", addr);
    axum::serve(listener, app).await.map_err(|e| e.to_string())
}

async fn index() -> Html<&'static str> {
    Html(include_str!("static/index.html"))
}

async fn api_healthz(State(eng): State<Arc<Engine>>) -> (StatusCode, Json<serde_json::Value>) {
    let snapshot = eng.dashboard_snapshot();
    let status = if snapshot.is_healthy { "ok" } else { "degraded" };
    (
        if snapshot.is_healthy { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE },
        Json(serde_json::json!({
            "status": status,
            "reason": snapshot.health_reason,
            "uptime_secs": snapshot.uptime_secs,
        })),
    )
}

async fn api_snapshot(State(eng): State<Arc<Engine>>) -> Json<DashboardSnapshot> {
    Json(eng.dashboard_snapshot())
}

async fn api_history_batches(State(eng): State<Arc<Engine>>) -> Json<Vec<BatchRecord>> {
    Json(eng.get_batch_history())
}

async fn api_history_blocks(State(eng): State<Arc<Engine>>) -> Json<Vec<BlockRecord>> {
    Json(eng.get_block_log())
}

async fn api_traffic_subnets(State(eng): State<Arc<Engine>>) -> Json<Vec<SubnetRow>> {
    Json(eng.get_hot_subnets())
}

async fn api_status_modules(State(eng): State<Arc<Engine>>) -> Json<Vec<ModuleStats>> {
    Json(eng.get_module_stats())
}

async fn api_get_config(State(eng): State<Arc<Engine>>) -> Json<Config> {
    Json(eng.config.load().as_ref().clone())
}

#[derive(Debug, Deserialize)]
pub struct ConfigPatch {
    #[serde(default)]
    pub engine: Option<crate::config::EngineConfig>,
    #[serde(default)]
    pub detection: Option<crate::config::DetectionConfig>,
    #[serde(default)]
    pub ipc: Option<crate::config::IpcConfig>,
    #[serde(default)]
    pub forecasting: Option<crate::config::ForecastingConfig>,
    #[serde(default)]
    pub dashboard: Option<crate::config::DashboardConfig>,
}

#[derive(Serialize)]
struct ConfigResponse {
    ok:     bool,
    config: Config,
}

async fn api_set_config(
    State(eng): State<Arc<Engine>>,
    Json(patch): Json<ConfigPatch>,
) -> (StatusCode, Json<ConfigResponse>) {
    let mut cfg = eng.config.load().as_ref().clone();
    if let Some(v) = patch.engine { cfg.engine = v; }
    if let Some(v) = patch.detection { cfg.detection = v; }
    if let Some(v) = patch.ipc { cfg.ipc = v; }
    if let Some(v) = patch.forecasting { cfg.forecasting = v; }
    if let Some(v) = patch.dashboard { cfg.dashboard = v; }
    eng.config.store(Arc::new(cfg.clone()));
    (
        StatusCode::OK,
        Json(ConfigResponse { ok: true, config: cfg }),
    )
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;
    use axum::{
        body::Body,
        http::Request,
        routing::get,
        Router,
    };
    use std::sync::Arc;
    use tower::ServiceExt;

    fn test_engine() -> Arc<Engine> {
        Arc::new(Engine::new(Config::default()))
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        let eng = test_engine();
        let app = Router::new()
            .route("/healthz", get(api_healthz))
            .with_state(eng);

        let response = app
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 10_000).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn snapshot_returns_valid_json() {
        let eng = test_engine();
        let app = Router::new()
            .route("/api/snapshot", get(api_snapshot))
            .with_state(eng);

        let response = app
            .oneshot(Request::get("/api/snapshot").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 100_000).await.unwrap();
        let json: DashboardSnapshot = serde_json::from_slice(&body).unwrap();
        // Uptime can be 0 for a freshly created engine with cached snapshot
        assert!(json.events_ingested == 0);
    }

    #[tokio::test]
    async fn config_get_returns_default() {
        let eng = test_engine();
        let app = Router::new()
            .route("/api/config", get(api_get_config))
            .with_state(eng);

        let response = app
            .oneshot(Request::get("/api/config").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 100_000).await.unwrap();
        let json: Config = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.engine.ram_limit_mb, 512);
        assert_eq!(json.engine.shard_count, 256);
    }

    #[tokio::test]
    async fn history_batches_returns_ok() {
        let eng = test_engine();
        let app = Router::new()
            .route("/api/history/batches", get(api_history_batches))
            .with_state(eng);
        let response = app.oneshot(Request::get("/api/history/batches").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10_000).await.unwrap();
        let batches: Vec<BatchRecord> = serde_json::from_slice(&body).unwrap();
        assert!(batches.is_empty());
    }

    #[tokio::test]
    async fn history_blocks_returns_ok() {
        let eng = test_engine();
        let app = Router::new()
            .route("/api/history/blocks", get(api_history_blocks))
            .with_state(eng);
        let response = app.oneshot(Request::get("/api/history/blocks").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10_000).await.unwrap();
        let blocks: Vec<BlockRecord> = serde_json::from_slice(&body).unwrap();
        assert!(blocks.is_empty());
    }

    #[tokio::test]
    async fn traffic_subnets_returns_ok() {
        let eng = test_engine();
        let app = Router::new()
            .route("/api/traffic/subnets", get(api_traffic_subnets))
            .with_state(eng);
        let response = app.oneshot(Request::get("/api/traffic/subnets").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10_000).await.unwrap();
        let subnets: Vec<SubnetRow> = serde_json::from_slice(&body).unwrap();
        assert!(subnets.is_empty());
    }

    #[tokio::test]
    async fn status_modules_returns_ok() {
        let eng = test_engine();
        let app = Router::new()
            .route("/api/status/modules", get(api_status_modules))
            .with_state(eng);
        let response = app.oneshot(Request::get("/api/status/modules").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10_000).await.unwrap();
        let modules: Vec<ModuleStats> = serde_json::from_slice(&body).unwrap();
        assert!(!modules.is_empty()); // Should have at least default modules
        assert_eq!(modules.len(), 4);
    }
}
