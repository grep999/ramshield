use crate::config::Config;
use crate::engine::Engine;
use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, Json,
    },
    routing::get,
    Router,
};
use futures_util::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;
use tracing::info;

pub async fn serve(engine: Arc<Engine>, addr: &str) -> Result<(), String> {
    let app = Router::new()
        .route("/", get(index))
        .route("/healthz", get(api_healthz))
        .route("/api/snapshot", get(api_snapshot))
        .route("/api/config", get(api_get_config).post(api_set_config))
        .route("/api/stream", get(api_stream))
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
    let healthy = snapshot.channel_depth < 1_000_000;
    let status = if healthy { "ok" } else { "degraded" };
    (
        if healthy { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE },
        Json(serde_json::json!({
            "status": status,
            "uptime_secs": snapshot.uptime_secs,
            "ips_tracked": snapshot.ips_tracked,
            "blocked_total": snapshot.blocked_total,
            "channel_depth": snapshot.channel_depth,
            "ram_pct": snapshot.ram_pct,
        })),
    )
}

async fn api_snapshot(State(eng): State<Arc<Engine>>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(eng.dashboard_snapshot()).unwrap_or_default())
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

async fn api_stream(
    State(eng): State<Arc<Engine>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream::unfold(eng, |eng| async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let snap = eng.dashboard_snapshot();
        let json = serde_json::to_string(&snap).unwrap_or_else(|_| "{}".into());
        let ev = Event::default().data(json);
        Some((Ok(ev), eng))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
