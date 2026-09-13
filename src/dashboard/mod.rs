pub mod auth;

use crate::config::Config;
use crate::engine::Engine;
use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    middleware as axum_mw,
    response::{Html, Json},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::metrics::{DashboardSnapshot, ModuleStats, SubnetRow};

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
        .layer(axum_mw::from_fn_with_state(app_state, auth::require_auth));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| e.to_string())?;
    info!("Dashboard http://{}", addr);
    // into_make_service_with_connect_info surfaces the peer SocketAddr to
    // handlers via ConnectInfo — login_submit needs it for per-IP lockout.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|e| e.to_string())
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
        state.engine.metrics.render_prometheus_cached().to_string(),
    )
}

async fn api_snapshot(State(state): State<AppState>) -> Json<DashboardSnapshot> {
    Json(state.engine.dashboard_snapshot())
}

async fn api_history_batches(
    State(state): State<AppState>,
) -> ([(axum::http::header::HeaderName, &'static str); 1], String) {
    use axum::http::header;
    (
        [(header::CONTENT_TYPE, "application/json")],
        state.engine.get_batch_history_json().to_string(),
    )
}

async fn api_history_blocks(
    State(state): State<AppState>,
) -> ([(axum::http::header::HeaderName, &'static str); 1], String) {
    use axum::http::header;
    (
        [(header::CONTENT_TYPE, "application/json")],
        state.engine.get_block_log_json().to_string(),
    )
}

async fn api_traffic_subnets(State(state): State<AppState>) -> Json<Vec<SubnetRow>> {
    Json(state.engine.get_hot_subnets())
}

async fn api_status_modules(State(state): State<AppState>) -> Json<Vec<ModuleStats>> {
    Json(state.engine.get_module_stats())
}

async fn api_get_config(State(state): State<AppState>) -> Json<ConfigView> {
    let cfg = state.engine.config.load().as_ref().clone();
    Json(ConfigView::from_config(&cfg))
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ConfigView {
    pub engine: crate::config::EngineConfig,
    pub detection: crate::config::DetectionConfig,
    pub ipc: crate::config::IpcConfig,
    pub forecasting: crate::config::ForecastingConfig,
    pub dashboard: crate::config::DashboardConfig,
    /// True when IPC HMAC auth is configured (regardless of how many keys).
    pub auth_enabled: bool,
}

impl ConfigView {
    pub fn from_config(c: &crate::config::Config) -> Self {
        // P0 fix: redact raw HMAC key material. Each `key_id:hex` is replaced
        // with `key_id:<redacted>` so /api/config is safe to expose to any
        // authenticated dashboard user, while still letting operators see
        // which key_ids are configured.
        let redacted: Vec<String> = c
            .ipc
            .auth_keys
            .iter()
            .map(|entry| match entry.split_once(':') {
                Some((id, _)) => format!("{id}:{}", ramshield_config::REDACTED_PLACEHOLDER),
                None => ramshield_config::REDACTED_PLACEHOLDER.into(),
            })
            .collect();
        let mut ipc = c.ipc.clone();
        ipc.auth_keys = redacted;
        Self {
            engine: c.engine.clone(),
            detection: c.detection.clone(),
            ipc,
            forecasting: c.forecasting.clone(),
            dashboard: {
                let mut d = c.dashboard.clone();
                // ponytail: admin_password_hash is Argon2 PHC — never expose.
                if d.admin_password_hash.is_some() {
                    d.admin_password_hash = Some(ramshield_config::REDACTED_PLACEHOLDER.into());
                }
                d
            },
            auth_enabled: !c.ipc.auth_keys.is_empty(),
        }
    }
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
    config: ConfigView,
}

/// F1-fix (CWE-352 / OWASP A01): CSRF protection on POST /api/config.
///
/// Why: this endpoint accepts full config patches (IPC auth keys, dashboard
/// password hash). Without an origin check, an authenticated admin who
/// visits a hostile page could have that page fire a cross-origin POST
/// here (CORS blocks reading the response, NOT the request itself), e.g.
/// rotating IPC keys to lock out other admins or swapping config.
///
/// Defense-in-depth: the session cookie already carries `SameSite=Lax`,
/// which modern browsers withhold on cross-site POSTs — this check is the
/// second layer (and covers non-browser clients that DO attach cookies).
///
/// Logic: a browser always sends `Origin` on cross-origin requests; for
/// same-origin fetches `Origin` is also present, so we compare its
/// authority (host[:port]) against the request's `Host` header. If
/// `Origin` is absent we fall back to `Referer` (older clients, form
/// posts). If BOTH are absent the client is not a browser (curl,
/// scripts, IPC tooling) — fail-open, since no CSRF is possible without
/// a browser to smuggle cookies into. A present-but-mismatched header
/// fails closed with 403 before any config parsing happens.
///
/// ponytail: string-slice authority parsing instead of the `url` crate —
/// origin values are always `scheme://authority[/path]`, a find+split
/// covers that; add the crate if IPv6 literals in Host ever matter.
/// Upgrade path: SameSite=Strict on the cookie, or a double-submit CSRF
/// token, if the dashboard ever grows GET-side state changes too.
async fn api_set_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(patch): Json<ConfigPatch>,
) -> (StatusCode, Json<ConfigResponse>) {
    let mut cfg = state.engine.config.load().as_ref().clone();
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_lowercase);
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_lowercase);
    let referer = headers
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_lowercase);

    fn is_cross_origin(origin: Option<&str>, referer: Option<&str>, host: Option<&str>) -> bool {
        // If Origin present, use it; otherwise check Referer
        let header = origin.or(referer);
        // Both missing (curl/operator) => fail-open, allow
        let Some(header) = header else { return false };
        // Parse as URL: take everything after :// up to next / (path/query)
        let header = header.trim();
        let scheme_end = header.find("://").map(|i| i + 3).unwrap_or(0);
        let authority = &header[scheme_end..];
        let authority = authority.split('/').next().unwrap_or("");
        let authority = authority.split('?').next().unwrap_or("");
        // Compare authority against Host header; empty => allow
        !host.is_some_and(|h| h == authority)
    }

    if is_cross_origin(origin.as_deref(), referer.as_deref(), host.as_deref()) {
        return (
            StatusCode::FORBIDDEN,
            Json(ConfigResponse {
                ok: false,
                config: ConfigView::from_config(&cfg),
            }),
        );
    }
    // P2 fix: GET /api/config returns auth_keys as "id:<redacted>" and
    // admin_password_hash as "<redacted>". An operator (or UI) POSTing the
    // viewed config back used to PASS validate() — the placeholder strings
    // are non-empty, so the public-bind guard waved them through — then on
    // restart hex-decode failed, keys were SKIPPED, and the server came up
    // with auth silently disabled (or the dashboard permanently un-loginable
    // via PasswordHash::new failing). Placeholders are never valid input.
    fn contains_placeholder(cfg: &ramshield_config::Config) -> bool {
        cfg.ipc
            .auth_keys
            .iter()
            .any(|k| k.contains(ramshield_config::REDACTED_PLACEHOLDER))
            || cfg
                .dashboard
                .admin_password_hash
                .as_deref()
                .is_some_and(|h| h.contains(ramshield_config::REDACTED_PLACEHOLDER))
    }
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
    let invalid = cfg.validate().is_err() || contains_placeholder(&cfg);
    if invalid {
        return (
            StatusCode::BAD_REQUEST,
            Json(ConfigResponse {
                ok: false,
                config: ConfigView::from_config(&state.engine.config.load()),
            }),
        );
    }
    state.engine.config.store(Arc::new(cfg.clone()));
    (
        StatusCode::OK,
        Json(ConfigResponse {
            ok: true,
            config: ConfigView::from_config(&cfg),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;
    use crate::metrics::{BatchRecord, BlockRecord};
    use axum::{
        Router,
        body::Body,
        http::Request,
        routing::{get, post},
    };
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
        let json: ConfigView = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.engine.ram_limit_mb, 512);
        assert_eq!(json.engine.shard_count, 256);
    }

    /// P0 regression: /api/config must NOT return raw HMAC keys. Without
    /// the redaction, anyone with a session token could read every
    /// ipc.auth_keys entry and forge signed IPC frames.
    #[tokio::test]
    async fn config_redacts_hmac_keys() {
        let state = test_app_state();
        // Plant a fake key into the live config; should appear as <redacted>.
        let mut cfg = state.engine.config.load().as_ref().clone();
        cfg.ipc.auth_keys =
            vec!["k1:deadbeefcafebabe0123456789abcdef0123456789abcdef0123456789abcdef".into()];
        state.engine.config.store(Arc::new(cfg));
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
        let raw = std::str::from_utf8(&body).unwrap();
        assert!(
            !raw.contains("deadbeefcafebabe"),
            "raw hex secret leaked in /api/config response"
        );
        assert!(
            raw.contains("k1:<redacted>"),
            "expected redacted key id marker"
        );
    }

    /// P0 regression: POST /api/config must also redact HMAC keys.
    #[tokio::test]
    async fn post_config_redacts_hmac_keys() {
        let state = test_app_state();
        let mut cfg = state.engine.config.load().as_ref().clone();
        cfg.ipc.auth_keys =
            vec!["k1:deadbeefcafebabe0123456789abcdef0123456789abcdef0123456789abcdef".into()];
        state.engine.config.store(Arc::new(cfg));
        let app = Router::new()
            .route("/api/config", post(api_set_config))
            .with_state(state);
        let body = serde_json::json!({"engine": {"max_peers": 42}});
        let response = app
            .oneshot(
                Request::post("/api/config")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), 100_000)
            .await
            .unwrap();
        let raw = std::str::from_utf8(&body).unwrap();
        assert!(
            !raw.contains("deadbeefcafebabe"),
            "POST /api/config leaked raw HMAC key"
        );
    }

    /// F1 regression: cross-origin POST /api/config (Origin header pointing
    /// at a hostile site) must be rejected — this is the CSRF guard.
    #[tokio::test]
    async fn post_config_cross_origin_forbidden() {
        let state = test_app_state();
        let app = Router::new()
            .route("/api/config", post(api_set_config))
            .with_state(state.clone());
        let body = serde_json::json!({"engine": {"worker_threads": 2, "ram_limit_mb": 128, "shard_count": 8}});
        let response = app
            .oneshot(
                Request::post("/api/config")
                    .header("content-type", "application/json")
                    .header("origin", "https://evil.example")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    /// F1 regression: same-origin POST (Origin matches Host) is allowed —
    /// the guard must not break legitimate dashboard use.
    #[tokio::test]
    async fn post_config_same_origin_allowed() {
        let state = test_app_state();
        let app = Router::new()
            .route("/api/config", post(api_set_config))
            .with_state(state.clone());
        let body = serde_json::json!({"engine": {"worker_threads": 4, "ram_limit_mb": 256, "shard_count": 64}});
        let response = app
            .oneshot(
                Request::post("/api/config")
                    .header("content-type", "application/json")
                    .header("host", "127.0.0.1:9999")
                    .header("origin", "http://127.0.0.1:9999")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cfg = state.engine.config.load();
        assert_eq!(cfg.engine.ram_limit_mb, 256);
        assert_eq!(cfg.engine.shard_count, 64);
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
        assert_eq!(modules.len(), 7);
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
            .layer(axum_mw::from_fn_with_state(app_state, auth::require_auth));

        // Auth disabled in test_app_state (None password hash) — request
        // passes the middleware, lands in api_snapshot, returns 200.
        let response = app
            .oneshot(Request::get("/api/snapshot").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
