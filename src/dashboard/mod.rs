pub mod auth;

use crate::config::Config;
use crate::engine::Engine;
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    middleware as axum_mw,
    response::{Html, Json},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::metrics::{BatchRecord, BlockRecord, DashboardSnapshot, ModuleStats, SubnetRow};

/// Single state type so one Router::with_state call satisfies all handlers.
#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Engine>,
    pub auth: Arc<auth::AuthState>,
}

pub async fn serve(engine: Arc<Engine>, addr: &str, cfg: &Config) -> Result<(), String> {
    let auth = auth::AuthState::new(
        cfg.dashboard.admin_password_hash.clone(),
        cfg.dashboard.session_ttl_secs,
        cfg.dashboard.max_login_attempts,
        cfg.dashboard.max_password_length,
    );
    let app_state = AppState {
        engine: engine.clone(),
        auth: Arc::new(auth.clone()),
    };
    let login = auth::router().with_state(auth);
    let app = Router::new()
        .route("/", get(index))
        .route("/healthz", get(api_healthz))
        .route("/metrics", get(api_metrics))
        .route("/api/snapshot", get(api_snapshot))
        .route("/api/history/batches", get(api_history_batches))
        .route("/api/history/blocks", get(api_history_blocks))
        .route("/api/traffic/subnets", get(api_traffic_subnets))
        .route("/api/status/modules", get(api_status_modules))
        .route("/api/config", get(api_get_config).post(api_set_config))
        .merge(login)
        .with_state(app_state.clone())
        // Auth on → same-origin only. Open dashboard (loopback, no password)
        // keeps permissive CORS for local tooling.
        .layer(if app_state.auth.enabled() {
            CorsLayer::new()
        } else {
            CorsLayer::permissive()
        })
        .layer(axum_mw::from_fn_with_state(
            app_state,
            auth::require_auth,
        ));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| e.to_string())?;
    info!("Dashboard http://{}", addr);
    axum::serve(listener, app).await.map_err(|e| e.to_string())
}

async fn index() -> Html<&'static str> {
    Html(include_str!("static/index.html"))
}

async fn api_healthz(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let snapshot = state.engine.dashboard_snapshot();
    let status = if snapshot.is_healthy {
        "ok"
    } else {
        "degraded"
    };
    (
        if snapshot.is_healthy {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(serde_json::json!({
            "status": status,
            "reason": snapshot.health_reason,
            "uptime_secs": snapshot.uptime_secs,
        })),
    )
}

/// Prometheus exposition format. Public like /healthz — scrape targets are
/// meant to be pollable; sensitive data stays behind authed routes.
async fn api_metrics(
    State(state): State<AppState>,
) -> ([(axum::http::header::HeaderName, &'static str); 1], String) {
    use axum::http::header;
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        state.engine.metrics.render_prometheus(),
    )
}

async fn api_snapshot(State(state): State<AppState>) -> Json<DashboardSnapshot> {
    Json(state.engine.dashboard_snapshot())
}

async fn api_history_batches(State(state): State<AppState>) -> Json<Vec<BatchRecord>> {
    Json(state.engine.get_batch_history())
}

async fn api_history_blocks(State(state): State<AppState>) -> Json<Vec<BlockRecord>> {
    Json(state.engine.get_block_log())
}

async fn api_traffic_subnets(State(state): State<AppState>) -> Json<Vec<SubnetRow>> {
    Json(state.engine.get_hot_subnets())
}

async fn api_status_modules(State(state): State<AppState>) -> Json<Vec<ModuleStats>> {
    Json(state.engine.get_module_stats())
}

async fn api_get_config(State(state): State<AppState>) -> Json<Config> {
    Json(state.engine.config.load().as_ref().clone())
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
    ok: bool,
    config: Config,
}

async fn api_set_config(
    State(state): State<AppState>,
    Json(patch): Json<ConfigPatch>,
) -> (StatusCode, Json<ConfigResponse>) {
    let mut cfg = state.engine.config.load().as_ref().clone();
    if let Some(v) = patch.engine {
        cfg.engine = v;
    }
    if let Some(v) = patch.detection {
        cfg.detection = v;
    }
    if let Some(v) = patch.ipc {
        cfg.ipc = v;
    }
    if let Some(v) = patch.forecasting {
        cfg.forecasting = v;
    }
    if let Some(v) = patch.dashboard {
        cfg.dashboard = v;
    }
    if cfg.validate().is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ConfigResponse {
                ok: false,
                config: state.engine.config.load().as_ref().clone(),
            }),
        );
    }
    state.engine.config.store(Arc::new(cfg.clone()));
    (
        StatusCode::OK,
        Json(ConfigResponse {
            ok: true,
            config: cfg,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;
    use axum::{Router, body::Body, http::Request, routing::get};
    use std::sync::Arc;
    use tower::ServiceExt;

    fn test_app_state() -> AppState {
        use crate::metrics::Metrics;
        use crate::storage::Store;
        let engine = Arc::new(Engine::new(
            Config::default(),
            Arc::new(Store::new(16)),
            Arc::new(Metrics::new()),
        ));
        let auth = Arc::new(auth::AuthState::new(None, 3600, 50, 1024));
        AppState { engine, auth }
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        let state = test_app_state();
        let app = Router::new()
            .route("/healthz", get(api_healthz))
            .with_state(state);

        let response = app
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 10_000)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn snapshot_returns_valid_json() {
        let state = test_app_state();
        let app = Router::new()
            .route("/api/snapshot", get(api_snapshot))
            .with_state(state);

        let response = app
            .oneshot(Request::get("/api/snapshot").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 100_000)
            .await
            .unwrap();
        let json: DashboardSnapshot = serde_json::from_slice(&body).unwrap();
        // Uptime can be 0 for a freshly created engine with cached snapshot
        assert!(json.events_ingested == 0);
    }

    #[tokio::test]
    async fn config_get_returns_default() {
        let state = test_app_state();
        let app = Router::new()
            .route("/api/config", get(api_get_config))
            .with_state(state);

        let response = app
            .oneshot(Request::get("/api/config").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 100_000)
            .await
            .unwrap();
        let json: Config = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.engine.ram_limit_mb, 512);
        assert_eq!(json.engine.shard_count, 256);
    }

    #[tokio::test]
    async fn history_batches_returns_ok() {
        let state = test_app_state();
        let app = Router::new()
            .route("/api/history/batches", get(api_history_batches))
            .with_state(state);
        let response = app
            .oneshot(
                Request::get("/api/history/batches")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10_000)
            .await
            .unwrap();
        let batches: Vec<BatchRecord> = serde_json::from_slice(&body).unwrap();
        assert!(batches.is_empty());
    }

    #[tokio::test]
    async fn history_blocks_returns_ok() {
        let state = test_app_state();
        let app = Router::new()
            .route("/api/history/blocks", get(api_history_blocks))
            .with_state(state);
        let response = app
            .oneshot(
                Request::get("/api/history/blocks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10_000)
            .await
            .unwrap();
        let blocks: Vec<BlockRecord> = serde_json::from_slice(&body).unwrap();
        assert!(blocks.is_empty());
    }

    #[tokio::test]
    async fn traffic_subnets_returns_ok() {
        let state = test_app_state();
        let app = Router::new()
            .route("/api/traffic/subnets", get(api_traffic_subnets))
            .with_state(state);
        let response = app
            .oneshot(
                Request::get("/api/traffic/subnets")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10_000)
            .await
            .unwrap();
        let subnets: Vec<SubnetRow> = serde_json::from_slice(&body).unwrap();
        assert!(subnets.is_empty());
    }

    #[tokio::test]
    async fn status_modules_returns_ok() {
        let state = test_app_state();
        let app = Router::new()
            .route("/api/status/modules", get(api_status_modules))
            .with_state(state);
        let response = app
            .oneshot(
                Request::get("/api/status/modules")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10_000)
            .await
            .unwrap();
        let modules: Vec<ModuleStats> = serde_json::from_slice(&body).unwrap();
        assert!(!modules.is_empty()); // Should have at least default modules
        assert_eq!(modules.len(), 4);
    }

    /// REGRESSION: AppState was introduced because the original code called
    /// `.with_state(engine)` then `.with_state(auth)`, and the second call
    /// silently replaced the first. Handlers extracting `State<Arc<Engine>>`
    /// would 500 in production. This test wires the full router (login merge
    /// + auth middleware + state) and confirms an authenticated request to
    /// `/api/snapshot` returns 200 with a real snapshot — proving both that
    /// AppState is the correct state type AND that the engine handle survives
    /// after the auth middleware has run.
    #[tokio::test]
    async fn full_router_serves_snapshot_via_app_state() {
        use axum::middleware as axum_mw;
        use tower_http::cors::CorsLayer;

        let state = test_app_state();
        let app_state = state.clone();
        let login = auth::router().with_state((*state.auth).clone());
        let app = Router::new()
            .route("/api/snapshot", get(api_snapshot))
            .route("/api/config", get(api_get_config))
            .merge(login)
            .with_state(state)
            .layer(CorsLayer::new())
            .layer(axum_mw::from_fn_with_state(
                app_state,
                auth::require_auth,
            ));

        // Auth disabled in test_app_state (None password hash) — request
        // passes the middleware, lands in api_snapshot, returns 200.
        let response = app
            .oneshot(
                Request::get("/api/snapshot")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
