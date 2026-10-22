pub mod auth;

use crate::config::Config;
use crate::engine::Engine;
use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    middleware as axum_mw,
    response::{
        Html, Json, Sse,
        sse::{Event, KeepAlive},
    },
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

use crate::metrics::{BlockRecord, DashboardSnapshot, ModuleStats, SubnetRow};

/// Single state type so one Router::with_state call satisfies all handlers.
#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Engine>,
    pub auth: Arc<auth::AuthState>,
}

// ponytail: this is redundant due to Rust edition; async fn is part of
// the standard (edition 2021). Keep for clarity, but no syntax change needed.
/// Build the dashboard's auth state and app state around the engine.
///
/// The auth state shares the **engine's** metrics instance. It used to
/// get a fresh one, so the CSRF and lockout counters it wrote never
/// reached the instance `/metrics` exports - the control-plane abuse
/// signals were structurally always zero.
fn app_state(engine: &Arc<Engine>, cfg: &Config) -> (auth::AuthState, AppState) {
    let auth = auth::AuthState::new(
        cfg.dashboard.admin_password_hash.clone(),
        cfg.dashboard.session_ttl_secs,
        cfg.dashboard.max_login_attempts,
        cfg.dashboard.max_password_length,
        cfg.dashboard.trusted_proxies.clone(),
        cfg.dashboard
            .cookie_secure
            .unwrap_or(cfg.dashboard.tls_enabled),
        engine.metrics.clone(),
        cfg.dashboard.argon2_parallelism,
    );
    let app_state = AppState {
        engine: engine.clone(),
        auth: Arc::new(auth.clone()),
    };
    (auth, app_state)
}

pub async fn serve(engine: Arc<Engine>, addr: &str, cfg: &Config) -> Result<(), String> {
    let (auth, app_state) = app_state(&engine, cfg);
    let login = auth::router().with_state(auth);
    let app = Router::new()
        .route("/", get(index))
        .route("/healthz", get(api_healthz))
        .route("/livez", get(api_livez))
        .route("/metrics", get(api_metrics))
        .route("/api/snapshot", get(api_snapshot))
        .route("/api/stream", get(api_stream))
        .route("/api/history/batches", get(api_history_batches))
        .route("/api/history/blocks", get(api_history_blocks))
        .route("/api/blocks/active", get(api_blocks_active))
        .route("/api/traffic/subnets", get(api_traffic_subnets))
        .route("/api/status/modules", get(api_status_modules))
        .route("/api/config", get(api_get_config).post(api_set_config))
        .merge(login)
        .with_state(app_state.clone())
        // CORS must be OUTERMOST layer (applied first in chain) so that
        // preflight OPTIONS and Origin/Referer checks run before auth middleware.
        // Auth on → same-origin only. Open dashboard (loopback, no password)
        // keeps permissive CORS for local tooling.
        .layer(axum_mw::from_fn_with_state(
            app_state.clone(),
            auth::require_auth,
        ))
        .layer(if app_state.auth.enabled() {
            CorsLayer::new()
        } else {
            CorsLayer::permissive()
        });

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
            "xdp_active": snapshot.xdp_active,
            "xdp_configured": snapshot.xdp_configured,
            "protection_state": snapshot.protection_state,
            // The native capture path is observation, not enforcement: a
            // dead capture leaves the service degraded, not unready, and
            // this is where an operator sees that.
            "native_capture_workers_running": snapshot.native_capture_workers_running,
            "native_capture_worker_failures": snapshot.native_capture_worker_failures,
        })),
    )
}

/// Process-alive endpoint for liveness probes.
///
/// This endpoint is used by Kubernetes liveness probes. It always returns
/// 200 OK while the process is running, regardless of RAM usage or XDP
/// state. This is critical because:
/// 1. A DDoS attack causes high RAM usage and stale XDP projection.
/// 2. Without this endpoint, Kubernetes would restart the pod during an
///    attack (since /healthz returns 503 under those conditions).
///
/// The endpoint is unauthenticated by design — it's a raw liveness check
/// that should only be reachable from the cluster's control plane, not
/// from external clients.
async fn api_livez() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "alive" })),
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

async fn api_blocks_active(State(state): State<AppState>) -> Json<Vec<BlockRecord>> {
    Json(state.engine.get_active_blocks())
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
        // On non-loopback binds, require Origin or Referer to prevent CSRF
        // On loopback (dev), allow missing Origin/Referer for curl/operator convenience
        if header.is_none() {
            // Check if this is a non-loopback bind by looking for non-localhost host
            let is_non_loopback = host
                .as_ref()
                .map(|h| {
                    !h.eq_ignore_ascii_case("localhost")
                        && !h.eq_ignore_ascii_case("127.0.0.1")
                        && !h.starts_with("[::1]")
                        && !h.starts_with("fe80:")
                })
                .unwrap_or(false);
            if is_non_loopback {
                return true; // fail-closed: treat as cross-origin
            }
            // Both missing (curl/operator) on loopback => fail-open, allow
            return false;
        }
        // Compare authority against Host header; empty => allow
        let Some(header) = header else { return false };
        // Parse as URL: take everything after :// up to next / (path/query)
        let header = header.trim();
        let scheme_end = header.find("://").map(|i| i + 3).unwrap_or(0);
        let authority = &header[scheme_end..];
        let authority = authority.split('/').next().unwrap_or("");
        let authority = authority.split('?').next().unwrap_or("");
        // Compare authority against Host header; empty => allow
        !host.is_some_and(|h| h.eq_ignore_ascii_case(authority))
    }

    let cross_origin = is_cross_origin(origin.as_deref(), referer.as_deref(), host.as_deref());
    if cross_origin {
        state.auth.metrics.inc_csrf_blocked();
        return (
            StatusCode::FORBIDDEN,
            Json(ConfigResponse {
                ok: false,
                config: ConfigView::from_config(&cfg),
            }),
        );
    }
    // The CSRF counter records the outcome of the CSRF check alone. It is
    // incremented before the session gate below, so a request that passed
    // CSRF and was then refused for a missing session counts as "CSRF
    // allowed" — which is exactly what it was. The session refusal is
    // counted separately.
    state.auth.metrics.inc_csrf_allowed();

    // P0: the config patch mutates the running daemon's configuration, so it
    // requires a valid session even when the dashboard has no password
    // configured.
    //
    // With `dashboard.admin_password_hash` set, the `require_auth` middleware
    // already gates every `/api/*` route, so this branch is reachable only
    // in the no-credentials configuration — where the patch would otherwise
    // be open to any local process, and the CSRF check (which fails open for
    // a request carrying neither `Origin` nor `Referer`) was the only thing
    // in the way.
    //
    // The CSRF check runs first, so a cross-origin request is still reported
    // as 403 rather than 401: the origin is the more specific fact, and the
    // status must not depend on whether the caller happens to hold a session.
    //
    // The patch is still validated below, so it cannot widen an exposure; it
    // can only change non-security configuration for the lifetime of the
    // process. The dashboard has no unblock route, so this is not the
    // severity of the IPC's equivalent — it is the same class of control,
    // decided together.
    if state.engine.config.load().dashboard.require_auth_for_config
        && !auth::session_valid(&state.auth, &headers)
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ConfigResponse {
                ok: false,
                config: ConfigView::from_config(&state.engine.config.load()),
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
        // A session-gated admin can relax the IPC mutation gate. That is
        // within an admin's authority, but it must not be silent: the
        // unauthenticated local mutator surface reopens for the lifetime of
        // the process.
        if v.require_auth_for_mutations
            && !state.engine.config.load().ipc.require_auth_for_mutations
        {
            warn!(
                "config patch disables ipc.require_auth_for_mutations — the unauthenticated \
                 local mutator surface is reopened for the lifetime of the process"
            );
        }
        cfg.ipc = v;
    }
    if let Some(v) = patch.forecasting {
        cfg.forecasting = v;
    }
    // D4: capture the new password hash before moving the dashboard config
    let new_pw_hash = patch
        .dashboard
        .as_ref()
        .and_then(|d| d.admin_password_hash.clone());
    if let Some(v) = patch.dashboard {
        if v.require_auth_for_config
            && !state.engine.config.load().dashboard.require_auth_for_config
        {
            warn!(
                "config patch disables dashboard.require_auth_for_config — the unauthenticated \
                 local config patch is reopened for the lifetime of the process"
            );
        }
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
    // D4 fix: hot-refresh dashboard password hash on config change
    if new_pw_hash.is_some() {
        state.auth.set_password_hash(new_pw_hash);
    }
    (
        StatusCode::OK,
        Json(ConfigResponse {
            ok: true,
            config: ConfigView::from_config(&cfg),
        }),
    )
}

/// Live rev2 telemetry stream. The server samples the existing atomic-backed
/// snapshot; it never enters the packet path or mutates engine state.
async fn api_stream(
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let stream = futures_util::stream::unfold(state, |state| async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let snapshot = state.engine.dashboard_snapshot();
        let modules = state.engine.get_module_stats();
        let pipeline_stages = vec![
            serde_json::json!({"stage":"wire","unit":"events","ingress":snapshot.events_ingested+snapshot.events_rejected+snapshot.events_shed,"drops":0}),
            serde_json::json!({"stage":"xdp","unit":"packets","ingress":state.engine.metrics.xdp_wire_pass.load(std::sync::atomic::Ordering::Relaxed)+state.engine.metrics.xdp_v4_drops.load(std::sync::atomic::Ordering::Relaxed)+state.engine.metrics.xdp_v6_drops.load(std::sync::atomic::Ordering::Relaxed),"drops":state.engine.metrics.xdp_v4_drops.load(std::sync::atomic::Ordering::Relaxed)+state.engine.metrics.xdp_v6_drops.load(std::sync::atomic::Ordering::Relaxed)}),
            serde_json::json!({"stage":"ipc","unit":"events","ingress":snapshot.events_ingested,"drops":snapshot.events_rejected+snapshot.events_shed}),
            serde_json::json!({"stage":"cold","unit":"IPs","ingress":snapshot.pipeline.promoted+snapshot.cold_skipped,"drops":snapshot.cold_skipped}),
            serde_json::json!({"stage":"detection","unit":"IPs","ingress":snapshot.pipeline.promoted,"drops":0}),
            serde_json::json!({"stage":"enforcement","unit":"blocks","ingress":snapshot.pipeline.blocked,"drops":0}),
        ];
        let payload = serde_json::json!({
            "ts": snapshot.ts_ms,
            "ts_ms": snapshot.ts_ms,
            "xdp": {
                "v4_drops_total": state.engine.metrics.xdp_v4_drops.load(std::sync::atomic::Ordering::Relaxed),
                "v6_drops_total": state.engine.metrics.xdp_v6_drops.load(std::sync::atomic::Ordering::Relaxed),
                "wire_pass_total": state.engine.metrics.xdp_wire_pass.load(std::sync::atomic::Ordering::Relaxed),
                "parse_fails_total": state.engine.metrics.xdp_parse_fails.load(std::sync::atomic::Ordering::Relaxed),
                "apply_failures_total": state.engine.metrics.xdp_apply_failures.load(std::sync::atomic::Ordering::Relaxed),
                "active": snapshot.xdp_active,
                "map_capacity": 102400
            },
            "ipc": {
                "ingest_total": snapshot.events_ingested,
                "rejected_total": snapshot.events_rejected,
                "frames_rejected_total": snapshot.frames_rejected_total,
                "shed_total": snapshot.events_shed,
                "active_connections": 0,
                "ring_depth": snapshot.channel_depth,
                "ring_capacity": 64000
            },
            "detection": {
                "promotions_total": snapshot.promotions,
                "cold_skipped_total": snapshot.cold_skipped,
                "blocks_total": snapshot.blocks_applied,
                "blocks_detection": state.engine.metrics.blocks_detection.load(std::sync::atomic::Ordering::Relaxed),
                "capacity_exceeded_total": state.engine.metrics.capacity_exceeded_count.load(std::sync::atomic::Ordering::Relaxed),
                "capacity_refused_ips_total": state.engine.metrics.capacity_exceeded_ips.load(std::sync::atomic::Ordering::Relaxed),
                "subnet_blocks": state.engine.metrics.blocks_subnet.load(std::sync::atomic::Ordering::Relaxed),
                "forecast_blocks": state.engine.metrics.blocks_forecast.load(std::sync::atomic::Ordering::Relaxed),
                "modules": modules
            },
            "forecasting": modules.iter().find(|m| m.label == "Forecasting").map(|m| m.detail.clone()).unwrap_or_else(|| serde_json::json!({})),
            "cgnat": modules.iter().find(|m| m.label == "CGNAT").map(|m| m.detail.clone()).unwrap_or_else(|| serde_json::json!({})),
            "analytics": modules.iter().find(|m| m.label == "Analytics").map(|m| m.detail.clone()).unwrap_or_else(|| serde_json::json!({})),
            "mesh": modules.iter().find(|m| m.label == "Mesh").map(|m| m.detail.clone()).unwrap_or_else(|| serde_json::json!({})),
            "system": {
                "cpu_pct": snapshot.cpu_usage,
                "rss_mb": snapshot.memory_usage_mb,
                "total_ram_mb": snapshot.total_ram_mb,
                "store_ram_bytes": snapshot.ram_bytes,
                "store_ram_limit_mb": snapshot.ram_limit_mb,
                "ram_pct": snapshot.ram_pct,
                "ips_tracked": snapshot.ips_tracked
            },
            "durability": {
                "wal_lsn": snapshot.wal_lsn,
                "pending_expirations": snapshot.pending_expirations
            },
            "health": {
                "is_healthy": snapshot.is_healthy,
                "health_reason": snapshot.health_reason,
                "xdp_active": snapshot.xdp_active,
                "xdp_configured": snapshot.xdp_configured,
                "protection_state": snapshot.protection_state,
                "native_capture_workers_running": snapshot.native_capture_workers_running,
                "native_capture_worker_failures": snapshot.native_capture_worker_failures
            },
            "pipeline": pipeline_stages
        });
        let event = Event::default()
            .json_data(payload)
            .unwrap_or_else(|_| Event::default());
        Some((Ok(event), state))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
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

    /// I-020: the quarantine search input must be wired to the filter, and
    /// the filter must survive the feed's periodic rebuild. The feed
    /// repaints every 5s via `loadBlocks()`, which replaces the rows, so a
    /// filter that is not re-applied there silently reverts to "show all".
    #[test]
    fn quarantine_search_is_wired_and_survives_the_feed_rebuild() {
        let page = include_str!("static/index.html");

        let input = page
            .split("<input")
            .find(|chunk| chunk.contains("id=\"tableSearch\""))
            .expect("#tableSearch input must exist");
        let input = &input[..input.find('>').expect("input tag must end")];
        assert!(
            input.contains("oninput=\"filterTable()\""),
            "the search input must call filterTable on input: {input}"
        );

        let load_blocks = page
            .split("async function loadBlocks()")
            .nth(1)
            .expect("loadBlocks must exist");
        let load_blocks = &load_blocks[..load_blocks.find("\n  }").expect("loadBlocks must end")];
        assert!(
            load_blocks.contains("filterTable()"),
            "loadBlocks must re-apply the filter after it rebuilds the rows, or the filter reverts every refresh"
        );
    }

    /// The quarantine table's headers, the cells each row gets, the CSV
    /// export's columns and its slice bound must all agree.
    ///
    /// They did not: the table had 5 headers and 6 cells (the surplus was a
    /// trailing em dash), and the CSV declared 5 column names but sliced 4
    /// cells out of every row, so every exported row silently lost its
    /// Time. The mismatch came back on every 5 s rebuild.
    #[test]
    fn the_quarantine_table_and_its_csv_export_agree_on_the_column_count() {
        let page = include_str!("static/index.html");

        // The header cells of the quarantine table (the page has more than
        // one table, so scope to the quarantine one).
        let table = page
            .split("id=\"quarantineTable\"")
            .nth(1)
            .expect("the quarantine table must exist");
        let head_section = &table[..table.find("</thead>").expect("the table must have a head")];
        let head = head_section.matches("<th>").count();
        assert_eq!(
            head, 5,
            "the quarantine table's header count changed: {head}"
        );

        // The cells each row gets: the value list loadBlocks builds.
        let values = page
            .split("for(const v of [")
            .nth(1)
            .expect("loadBlocks must build rows from a value list");
        let values = &values[..values.find(']').expect("the value list must end")];
        assert_eq!(
            count_top_level_items(values),
            head,
            "the quarantine table builds a different number of cells than it has headers"
        );

        // The CSV export: the names it declares and the cells it takes.
        let csv = page
            .split("let csv=")
            .nth(1)
            .expect("exportCSV must declare a CSV header");
        let csv = &csv[..csv.find(';').expect("the CSV header must end")];
        let names = csv.trim_matches('\'').trim_matches('"');
        assert_eq!(
            names.split(',').count(),
            head,
            "the CSV export declares a different number of columns than the table has"
        );

        let export = page
            .split("function exportCSV()")
            .nth(1)
            .expect("exportCSV must exist");
        let slice = export
            .split("slice(0,")
            .nth(1)
            .expect("exportCSV must slice the row's cells");
        let bound: usize = slice
            .split(')')
            .next()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or_else(|| panic!("the CSV slice bound is not a number: {slice}"));
        assert_eq!(
            bound, head,
            "the CSV export takes {bound} cells of a {head}-column row"
        );

        // The placeholder rows must span the same number of columns.
        for span in ["colspan=\"", "colSpan="] {
            let mut rest = page;
            while let Some(i) = rest.find(span) {
                let after = &rest[i + span.len()..];
                let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    let n: usize = digits.parse().unwrap();
                    assert_eq!(
                        n, head,
                        "a quarantine placeholder row spans {n} columns, the table has {head}"
                    );
                }
                rest = &rest[i + span.len()..];
            }
        }
    }

    /// The number of items in a JavaScript array literal, ignoring the
    /// commas that sit inside a nested call or a quoted string.
    fn count_top_level_items(list: &str) -> usize {
        let list = list.trim();
        if list.is_empty() {
            return 0;
        }
        let mut depth = 0usize;
        let mut items = 1usize;
        let mut in_str = false;
        let mut quote = ' ';
        for c in list.chars() {
            if in_str {
                if c == quote {
                    in_str = false;
                }
                continue;
            }
            match c {
                '\'' | '"' | '`' => {
                    in_str = true;
                    quote = c;
                }
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => items += 1,
                _ => {}
            }
        }
        items
    }

    /// F-32: the auth state must share the engine's metrics instance.
    /// It used to get a fresh one, so the CSRF and lockout counters it
    /// writes never reached the instance `/metrics` exports, and the
    /// control-plane abuse signals were structurally always zero.
    #[test]
    fn the_auth_state_shares_the_engine_metrics() {
        use crate::metrics::Metrics;
        use crate::storage::Store;
        let engine = Arc::new(Engine::new(
            Config::default(),
            Arc::new(Store::new(16)),
            Arc::new(Metrics::new()),
        ));
        let (_auth, state) = app_state(&engine, &Config::default());
        assert!(
            Arc::ptr_eq(&state.auth.metrics, &state.engine.metrics),
            "the auth state must share the engine's metrics, or its counters are never exported"
        );
        state.auth.metrics.inc_csrf_blocked();
        let text = state.engine.metrics.render_prometheus();
        assert!(
            text.contains("ramshield_csrf_blocked_total 1"),
            "a blocked CSRF request must reach the exported series:\n{text}"
        );
    }

    fn test_app_state() -> AppState {
        use crate::metrics::Metrics;
        use crate::storage::Store;
        let engine = Arc::new(Engine::new(
            Config::default(),
            Arc::new(Store::new(16)),
            Arc::new(Metrics::new()),
        ));
        engine.mark_pipeline_ready_for_test();
        let auth = Arc::new(auth::AuthState::new(
            None,
            3600,
            50,
            1024,
            vec![],
            true,
            Arc::new(Metrics::new()),
            4,
        ));
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
        assert_eq!(json["protection_state"], "protected");
        assert_eq!(json["xdp_configured"], false);
        assert_eq!(json["xdp_active"], false);
    }

    #[tokio::test]
    async fn healthz_503_when_xdp_configured_without_fallback() {
        let mut cfg = Config::default();
        cfg.xdp.enabled = true;
        cfg.xdp.allow_inband_fallback = false;
        use crate::metrics::Metrics;
        use crate::storage::Store;
        let engine = Arc::new(Engine::new(
            cfg,
            Arc::new(Store::new(16)),
            Arc::new(Metrics::new()),
        ));
        engine.mark_pipeline_ready_for_test();
        let auth = Arc::new(auth::AuthState::new(
            None,
            3600,
            50,
            1024,
            vec![],
            true,
            Arc::new(Metrics::new()),
            4,
        ));
        let state = AppState { engine, auth };
        let app = Router::new()
            .route("/healthz", get(api_healthz))
            .with_state(state);
        let response = app
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), 10_000)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["protection_state"], "failed");
        assert_eq!(json["xdp_configured"], true);
        assert_eq!(json["xdp_active"], false);
    }

    #[tokio::test]
    async fn healthz_degraded_when_xdp_fallback_allowed() {
        let mut cfg = Config::default();
        cfg.xdp.enabled = true;
        cfg.xdp.allow_inband_fallback = true;
        use crate::metrics::Metrics;
        use crate::storage::Store;
        let engine = Arc::new(Engine::new(
            cfg,
            Arc::new(Store::new(16)),
            Arc::new(Metrics::new()),
        ));
        engine.mark_pipeline_ready_for_test();
        let auth = Arc::new(auth::AuthState::new(
            None,
            3600,
            50,
            1024,
            vec![],
            true,
            Arc::new(Metrics::new()),
            4,
        ));
        let state = AppState { engine, auth };
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
        assert_eq!(json["protection_state"], "degraded");
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

    #[tokio::test]
    async fn post_config_non_loopback_missing_origin_forbidden() {
        let state = test_app_state();
        let app = Router::new()
            .route("/api/config", post(api_set_config))
            .with_state(state);
        let body = serde_json::json!({"engine": {"worker_threads": 2, "ram_limit_mb": 128, "shard_count": 8}});
        let response = app
            .oneshot(
                Request::post("/api/config")
                    .header("content-type", "application/json")
                    .header("host", "10.0.0.1:9999")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    /// F1 regression: same-origin POST (Origin matches Host) is allowed —
    /// the guard must not break legitimate dashboard use.
    ///
    /// A legitimate caller holds a session: the config patch requires one
    /// even when the dashboard has no password configured, so that a config
    /// patch is not open to any local process.
    #[tokio::test]
    async fn post_config_same_origin_allowed() {
        let state = test_app_state();
        let token = "b".repeat(64);
        state.auth.register_session(&token);
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
                    .header("cookie", format!("{}={}", auth::COOKIE_NAME, token))
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

    /// Verifies the SSE route returns a streaming response with the correct content
    /// type. We read only the first chunk — the stream is infinite, so we must not
    /// await the body to completion.
    #[tokio::test]
    async fn api_stream_emits_sse_events() {
        let state = test_app_state();
        let app = Router::new()
            .route("/api/stream", get(api_stream))
            .with_state(state);

        let response = app
            .oneshot(Request::get("/api/stream").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let ct = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok());
        assert_eq!(
            ct,
            Some("text/event-stream"),
            "SSE endpoint must set content-type text/event-stream"
        );
    }

    /// P0: `POST /api/config` must require a valid session even when the
    /// dashboard has no password configured.
    ///
    /// With `dashboard.admin_password_hash` unset the dashboard has no
    /// authentication at all, so the CSRF check (which fails open for a
    /// request carrying neither `Origin` nor `Referer`) was the only thing
    /// between any local process and a full configuration patch. The patch is
    /// validated, so it cannot widen an exposure — but it can change the
    /// daemon's behaviour for the lifetime of the process.
    ///
    /// With a password hash configured, the `require_auth` middleware already
    /// gates every `/api/*` route, so this branch is reachable only in the
    /// no-credentials configuration.
    #[tokio::test]
    async fn config_patch_requires_a_session_without_a_configured_password() {
        let state = test_app_state();
        let app = Router::new()
            .route("/api/config", post(api_set_config))
            .with_state(state);

        let response = app
            .oneshot(
                Request::post("/api/config")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "the config patch must not be open to any local process"
        );
    }

    /// The operator escape hatch: `dashboard.require_auth_for_config = false`
    /// restores the unauthenticated config patch.
    #[tokio::test]
    async fn config_patch_is_allowed_when_the_gate_is_disabled() {
        use crate::metrics::Metrics;
        use crate::storage::Store;
        let mut cfg = Config::default();
        cfg.dashboard.require_auth_for_config = false;
        let engine = Arc::new(Engine::new(
            cfg,
            Arc::new(Store::new(16)),
            Arc::new(Metrics::new()),
        ));
        engine.mark_pipeline_ready_for_test();
        let auth = Arc::new(auth::AuthState::new(
            None,
            3600,
            50,
            1024,
            vec![],
            true,
            Arc::new(Metrics::new()),
            4,
        ));
        let state = AppState { engine, auth };
        let app = Router::new()
            .route("/api/config", post(api_set_config))
            .with_state(state);

        let response = app
            .oneshot(
                Request::post("/api/config")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "the escape hatch must let the patch past the gate"
        );
    }

    /// The route-authorization matrix, **authenticated mode**: with an admin
    /// password hash configured, the auth middleware refuses an
    /// unauthenticated `POST /api/config` before the handler runs.
    ///
    /// The hash value is never verified here — the middleware only checks
    /// that a session cookie is present and valid — so a placeholder is
    /// enough to make the middleware active.
    #[tokio::test]
    async fn config_patch_is_refused_without_a_session_when_auth_is_enabled() {
        use crate::metrics::Metrics;
        use crate::storage::Store;
        let engine = Arc::new(Engine::new(
            Config::default(),
            Arc::new(Store::new(16)),
            Arc::new(Metrics::new()),
        ));
        engine.mark_pipeline_ready_for_test();
        let auth = Arc::new(auth::AuthState::new(
            Some("not-a-real-phc-string".to_string()),
            3600,
            50,
            1024,
            vec![],
            true,
            Arc::new(Metrics::new()),
            4,
        ));
        assert!(auth.enabled(), "the auth middleware must be active");
        let state = AppState { engine, auth };
        let app = Router::new()
            .route("/api/config", post(api_set_config))
            .layer(axum_mw::from_fn_with_state(
                state.clone(),
                auth::require_auth,
            ))
            .with_state(state);

        let response = app
            .oneshot(
                Request::post("/api/config")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "an unauthenticated config patch must be refused in authenticated mode"
        );

        // Distinguish the middleware's refusal from the handler's own gate:
        // the middleware answers `{"error":"unauthorized"}`, the handler
        // answers a `ConfigResponse`. Both are 401, so the status alone
        // would not prove which one refused the request.
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("read body");
        let body = String::from_utf8_lossy(&body);
        assert!(
            body.contains(r#""error":"unauthorized""#),
            "the auth middleware must be the one that refused, not the route gate; body: {body}"
        );
    }
}
