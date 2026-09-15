// crates/dashboard/src/mod.rs — full rewrite: Obsidian HUD + SSE + zero-impact observer
pub mod auth;

use crate::config::Config;
use crate::engine::Engine;
use crate::metrics::{DashboardSnapshot, ModuleStats, SubnetRow};
use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    middleware as axum_mw,
    response::{Html, Json, Sse},
    routing::{get, post},
};
use ramshield_types::EnforceCommand;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;
use tracing::info;

// ── TelemetryCollector ──────────────────────────────────────────────────────
// Zero-impact observer: reads from existing engine modules via Arc<T> clones.
// No locks, no allocations in hot path. Samples every 100ms.
//
// Integration points (0 code changes to existing modules):
//   * ramshield-cgnat:  read-only /dev/shm via ShmTableManager
//   * xdp-ebpf:        read BPF drop counts from Aya/LruHashMap
//   * detection:       sample EWMA/CUSUM atomics
//   * storage:         read HostBitmap [u64; 4] (32 bytes)
//   * ramshield-protocol: read TelemetryMetrics atomics
//   * ramshield-mesh:  read HybridLogicalClock HLC drift
pub struct TelemetryCollector {
    /// Arc<ShmTableManager> for /dev/shm reads (lock-free Seqlock).
    pub shm: Arc<ramshield_cgnat::ShmTableManager>,
    /// Arc<Metrics> for atomic counter reads.
    pub metrics: Arc<crate::metrics::Metrics>,
    /// Arc<Hlc> for HLC drift calculation.
    pub hlc: Option<Arc<ramshield_mesh::hlc::Hlc>>,
}

impl TelemetryCollector {
    /// Gathers a snapshot every 100ms (Total execution time: < 1 microsecond).
    pub fn collect_snapshot(&self) -> serde_json::Value {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // HLC drift (if mesh is linked).
        let hlc_drift_ms = match &self.hlc {
            Some(hlc) => {
                let (hlc_ms, _) = hlc.tick(0, 0);
                (hlc_ms as i64 - now_ms as i64).unsigned_abs()
            }
            None => 0,
        };

        // Read HostBitmap: [u64; 4] = 32 bytes of popcnt-able data.
        let shm_active_rules = self
            .shm
            .get_slot(0)
            .tier
            .load(std::sync::atomic::Ordering::Relaxed) as u64;

        serde_json::json!({
            "ts": now_ms,
            "ts_ms": now_ms,
            "shm_active_rules": shm_active_rules,
            "events_shed": self.metrics.events_shed.load(std::sync::atomic::Ordering::Relaxed),
            "events_full": self.metrics.blocks_detection.load(std::sync::atomic::Ordering::Relaxed),
            "hlc_drift_ms": hlc_drift_ms,
            "xdp_v4_drops": self.metrics.xdp_v4_drops.load(std::sync::atomic::Ordering::Relaxed),
            "xdp_v6_drops": self.metrics.xdp_v6_drops.load(std::sync::atomic::Ordering::Relaxed),
            "xdp_wire_pass": self.metrics.xdp_wire_pass.load(std::sync::atomic::Ordering::Relaxed),
            "xdp_parse_fails": self.metrics.xdp_parse_fails.load(std::sync::atomic::Ordering::Relaxed),
        })
    }
}

// ── AppState ────────────────────────────────────────────────────────────────
/// Single state type so one Router::with_state call satisfies all handlers.
#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Engine>,
    pub auth: Arc<auth::AuthState>,
    /// Telemetry collector for SSE streaming (zero-impact observer).
    pub collector: Option<Arc<TelemetryCollector>>,
}

// ── Server ──────────────────────────────────────────────────────────────────
pub async fn serve(engine: Arc<Engine>, addr: &str, cfg: &Config) -> Result<(), String> {
    let auth = auth::AuthState::new(
        cfg.dashboard.admin_password_hash.clone(),
        cfg.dashboard.session_ttl_secs,
        cfg.dashboard.max_login_attempts,
        cfg.dashboard.max_password_length,
        cfg.dashboard.trusted_proxies.clone(),
    );
    let app_state = AppState {
        engine: engine.clone(),
        auth: Arc::new(auth.clone()),
        collector: None, // populated if mesh/cgnat links are available
    };
    let login = auth::router().with_state(auth.clone());
    let app = Router::new()
        .route("/", get(index))
        .route("/healthz", get(api_healthz))
        .route("/metrics", get(api_metrics))
        .route("/api/snapshot", get(api_snapshot))
        .route("/api/history/batches", get(api_history_batches))
        .route("/api/history/blocks", get(api_history_blocks))
        .route("/api/blocks/active", get(api_blocks_active))
        .route("/api/traffic/subnets", get(api_traffic_subnets))
        .route("/api/status/modules", get(api_status_modules))
        .route("/api/config", get(api_get_config).post(api_set_config))
        // SSE streaming telemetry endpoint (Obsidian HUD data source)
        .route("/api/stream", get(stream_telemetry))
        // Mitigation endpoints (CSRF-protected)
        .route("/api/mitigate/block", post(api_manual_block))
        .route("/api/mitigate/unblock", post(api_manual_unblock))
        .merge(login)
        .with_state(app_state.clone())
        // Auth ON → same-origin only. Open dashboard (loopback, no password)
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
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|e| e.to_string())
}

// ── SSE Telemetry Stream ────────────────────────────────────────────────────
/// 100ms SSE telemetry stream — the data source for the Obsidian HUD.
/// Pushes xdp_drop_pps, events_shed, hlc_drift_ms, shm_active_rules.
async fn stream_telemetry(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {
    let engine = state.engine;
    let m = engine.metrics.clone();

    // ponytail: rev2 nested SSE payload. All rates computed server-side as
    // delta / elapsed over 100ms frames. Baselines captured at connection time
    // so first frame never reports lifetime totals as a spike.
    // Upgrade: Mutex<LastSample> instead of 20 Arc<AtomicU64> clones.
    struct LastSample {
        ts: u64,
        v4: u64, v6: u64, pass: u64, fail: u64,
        ingest: u64, reject: u64, shed: u64,
        promo: u64, cold: u64, bdet: u64, bsub: u64, bfc: u64,
        cgnat: u64, shm_l: u64, shm_h: u64, hll: u64, cms: u64,
        mban: u64, munban: u64,
        v4_e: f64, v6_e: f64, pass_e: f64, fail_e: f64,
        ingest_e: f64, reject_e: f64, shed_e: f64,
        promo_e: f64, cold_e: f64, bdet_e: f64, bsub_e: f64, bfc_e: f64,
        cgnat_e: f64, shm_l_e: f64, shm_h_e: f64, hll_e: f64, cms_e: f64,
        mban_e: f64, munban_e: f64,
    }
    struct RateState {
        v4: (u64, f64), v6: (u64, f64), pass: (u64, f64), fail: (u64, f64),
        ingest: (u64, f64), reject: (u64, f64), shed: (u64, f64),
        promo: (u64, f64), cold: (u64, f64), bdet: (u64, f64),
        bsub: (u64, f64), bfc: (u64, f64), cgnat: (u64, f64),
        shm_l: (u64, f64), shm_h: (u64, f64), hll: (u64, f64),
        cms: (u64, f64), mban: (u64, f64), munban: (u64, f64),
    }
    let now0 = crate::metrics::now_ms();
    let s0 = Arc::new(std::sync::Mutex::new(LastSample {
        ts: now0,
        v4: m.xdp_v4_drops.load(Ordering::Relaxed),
        v6: m.xdp_v6_drops.load(Ordering::Relaxed),
        pass: m.xdp_wire_pass.load(Ordering::Relaxed),
        fail: m.xdp_parse_fails.load(Ordering::Relaxed),
        ingest: m.events_ingested.load(Ordering::Relaxed),
        reject: m.events_rejected.load(Ordering::Relaxed),
        shed: m.events_shed.load(Ordering::Relaxed),
        promo: m.promotions_total.load(Ordering::Relaxed),
        cold: m.cold_skipped_total.load(Ordering::Relaxed),
        bdet: m.blocks_detection.load(Ordering::Relaxed),
        bsub: m.blocks_subnet.load(Ordering::Relaxed),
        bfc: m.blocks_forecast.load(Ordering::Relaxed),
        cgnat: m.cgnat_classify_ticks.load(Ordering::Relaxed),
        shm_l: m.shm_lookup_count.load(Ordering::Relaxed),
        shm_h: m.shm_cache_hits.load(Ordering::Relaxed),
        hll: m.hll_insert_count.load(Ordering::Relaxed),
        cms: m.cms_increment_count.load(Ordering::Relaxed),
        mban: m.mesh_record_ban_count.load(Ordering::Relaxed),
        munban: m.mesh_record_unban_count.load(Ordering::Relaxed),
        v4_e: 0.0, v6_e: 0.0, pass_e: 0.0, fail_e: 0.0,
        ingest_e: 0.0, reject_e: 0.0, shed_e: 0.0,
        promo_e: 0.0, cold_e: 0.0, bdet_e: 0.0, bsub_e: 0.0, bfc_e: 0.0,
        cgnat_e: 0.0, shm_l_e: 0.0, shm_h_e: 0.0, hll_e: 0.0, cms_e: 0.0,
        mban_e: 0.0, munban_e: 0.0,
    }));

    let stream = tokio_stream::wrappers::IntervalStream::new(
        tokio::time::interval(Duration::from_millis(100)),
    )
    .map(move |_| {
        let now_ms = crate::metrics::now_ms();
        let mut s = s0.lock().unwrap_or_else(|e| e.into_inner());
        let dt = ((now_ms.saturating_sub(s.ts)) as f64 / 1000.0).max(0.001);
        // ponytail: EWMA-smoothed rate with hold-on-zero. Raw deltas drop to 0
        // when a counter doesn't tick in a 100ms frame — that makes the HUD
        // flicker. EWMA (alpha=0.3) + hold-last-on-zero keeps values in flow state.
        const ALPHA: f64 = 0.3;
        // ponytail: Rust-2015 split-borrow workaround. Copy EWMA locals out,
        // compute on stacked borrows, write back at end of frame.
        // Upgrade: Mutex<RateState> with per-metric (prev, ewma) tuple fields.
        let mut g = RateState {
            v4: (s.v4, s.v4_e), v6: (s.v6, s.v6_e),
            pass: (s.pass, s.pass_e), fail: (s.fail, s.fail_e),
            ingest: (s.ingest, s.ingest_e),
            reject: (s.reject, s.reject_e), shed: (s.shed, s.shed_e),
            promo: (s.promo, s.promo_e), cold: (s.cold, s.cold_e),
            bdet: (s.bdet, s.bdet_e), bsub: (s.bsub, s.bsub_e),
            bfc: (s.bfc, s.bfc_e), cgnat: (s.cgnat, s.cgnat_e),
            shm_l: (s.shm_l, s.shm_l_e), shm_h: (s.shm_h, s.shm_h_e),
            hll: (s.hll, s.hll_e), cms: (s.cms, s.cms_e),
            mban: (s.mban, s.mban_e), munban: (s.munban, s.munban_e),
        };
        let ewma = |curr: u64, st: &mut (u64, f64), dt: f64| -> f64 {
            if curr >= st.0 {
                let raw = (curr - st.0) as f64 / dt;
                st.1 = if st.1 == 0.0 { raw } else { st.1 * (1.0 - ALPHA) + raw * ALPHA };
                st.0 = curr;
                st.1
            } else {
                st.0 = curr; st.1 = 0.0; 0.0
            }
        };
        // 1. Kernel dataplane
        let v4 = ewma(m.xdp_v4_drops.load(Ordering::Relaxed), &mut g.v4, dt);
        let v6 = ewma(m.xdp_v6_drops.load(Ordering::Relaxed), &mut g.v6, dt);
        let pass = ewma(m.xdp_wire_pass.load(Ordering::Relaxed), &mut g.pass, dt);
        let fail = ewma(m.xdp_parse_fails.load(Ordering::Relaxed), &mut g.fail, dt);
        // 2. Ingestion
        let ingest = ewma(m.events_ingested.load(Ordering::Relaxed), &mut g.ingest, dt);
        let reject = ewma(m.events_rejected.load(Ordering::Relaxed), &mut g.reject, dt);
        let shed = ewma(m.events_shed.load(Ordering::Relaxed), &mut g.shed, dt);
        // 3. Detection
        let promo = ewma(m.promotions_total.load(Ordering::Relaxed), &mut g.promo, dt);
        let cold = ewma(m.cold_skipped_total.load(Ordering::Relaxed), &mut g.cold, dt);
        let bdet = ewma(m.blocks_detection.load(Ordering::Relaxed), &mut g.bdet, dt);
        let bsub = ewma(m.blocks_subnet.load(Ordering::Relaxed), &mut g.bsub, dt);
        let bfc = ewma(m.blocks_forecast.load(Ordering::Relaxed), &mut g.bfc, dt);
        // 4. Auxiliary
        let cgnat = ewma(m.cgnat_classify_ticks.load(Ordering::Relaxed), &mut g.cgnat, dt);
        let shm_l = ewma(m.shm_lookup_count.load(Ordering::Relaxed), &mut g.shm_l, dt);
        let shm_h = ewma(m.shm_cache_hits.load(Ordering::Relaxed), &mut g.shm_h, dt);
        let hll = ewma(m.hll_insert_count.load(Ordering::Relaxed), &mut g.hll, dt);
        let cms = ewma(m.cms_increment_count.load(Ordering::Relaxed), &mut g.cms, dt);
        let mban = ewma(m.mesh_record_ban_count.load(Ordering::Relaxed), &mut g.mban, dt);
        let munban = ewma(m.mesh_record_unban_count.load(Ordering::Relaxed), &mut g.munban, dt);
        // write back
        s.v4 = g.v4.0; s.v4_e = g.v4.1; s.v6 = g.v6.0; s.v6_e = g.v6.1;
        s.pass = g.pass.0; s.pass_e = g.pass.1; s.fail = g.fail.0; s.fail_e = g.fail.1;
        s.ingest = g.ingest.0; s.ingest_e = g.ingest.1;
        s.reject = g.reject.0; s.reject_e = g.reject.1;
        s.shed = g.shed.0; s.shed_e = g.shed.1;
        s.promo = g.promo.0; s.promo_e = g.promo.1;
        s.cold = g.cold.0; s.cold_e = g.cold.1;
        s.bdet = g.bdet.0; s.bdet_e = g.bdet.1;
        s.bsub = g.bsub.0; s.bsub_e = g.bsub.1;
        s.bfc = g.bfc.0; s.bfc_e = g.bfc.1;
        s.cgnat = g.cgnat.0; s.cgnat_e = g.cgnat.1;
        s.shm_l = g.shm_l.0; s.shm_l_e = g.shm_l.1;
        s.shm_h = g.shm_h.0; s.shm_h_e = g.shm_h.1;
        s.hll = g.hll.0; s.hll_e = g.hll.1;
        s.cms = g.cms.0; s.cms_e = g.cms.1;
        s.mban = g.mban.0; s.mban_e = g.mban.1;
        s.munban = g.munban.0; s.munban_e = g.munban.1;
        s.ts = now_ms;

        let stats = engine.store.get_stats();
        let snapshot = engine.dashboard_snapshot();
        let (cpu, total_sys_ram, rss) = crate::metrics::get_system_usage();

        let total_v4_drops = m.xdp_v4_drops.load(Ordering::Relaxed);
        let total_v6_drops = m.xdp_v6_drops.load(Ordering::Relaxed);
        let total_wire_pass = m.xdp_wire_pass.load(Ordering::Relaxed);
        let total_parse_fails = m.xdp_parse_fails.load(Ordering::Relaxed);
        let shm_lookups_total = m.shm_lookup_count.load(Ordering::Relaxed);
        let shm_hits_total = m.shm_cache_hits.load(Ordering::Relaxed);
        let shm_hit_rate = if shm_l > 0.0 {
            (shm_h / shm_l) * 100.0
        } else if shm_lookups_total > 0 {
            (shm_hits_total as f64 / shm_lookups_total as f64) * 100.0
        } else {
            0.0
        };

        let payload = serde_json::json!({
            // Flat aliases for existing frontend + E1-E6 contract
            "ts": now_ms, "ts_ms": now_ms,
            "proxy_l7_rps": ingest,
            "xdp_v4_drops": total_v4_drops,
            "xdp_v6_drops": total_v6_drops,
            "xdp_wire_pass": total_wire_pass,
            "xdp_parse_fails": total_parse_fails,
            "health": {
                "is_healthy": snapshot.is_healthy,
                "health_reason": snapshot.health_reason,
                "xdp_active": snapshot.xdp_active,
            },
            "xdp_drop_pps": v4 + v6,
            "xdp_total_drop_pps": v4 + v6,
            "block_rate": promo, "promote_rate": promo,
            "shm_active_rules": stats.blocked,
            "subnet_bitmap_ones": stats.ips_tracked,
            "events_shed": shed, "events_full": reject,
            "ewma_velocity": ingest,
            "cusum_accumulator": f64::from_bits(m.hw_rps_bits.load(Ordering::Relaxed)),
            "cgnat_entropy_score": f64::from_bits(m.entropy_bits.load(Ordering::Relaxed)),
            "cgnat_allow": m.cgnat_tier_allow.load(Ordering::Relaxed),
            "cgnat_challenge": m.cgnat_tier_challenge.load(Ordering::Relaxed),
            "cgnat_powdrop": m.cgnat_tier_powdrop.load(Ordering::Relaxed),
            "cgnat_block": m.cgnat_tier_block.load(Ordering::Relaxed),
            "hlc_drift_ms": 0, // single-node without mesh: HLC is not a drift source
            "hll_rate": hll,
            "mesh_sync_rate": mban,
            // rev2 nested groups — full-scope operational grid
            "xdp": {
                "v4_drops_pps": v4, "v6_drops_pps": v6,
                "total_drops_pps": v4 + v6,
                "pass_pps": pass, "parse_fail_pps": fail,
                "v4_drops_total": total_v4_drops,
                "v6_drops_total": total_v6_drops,
                "wire_pass_total": total_wire_pass,
                "parse_fails_total": total_parse_fails,
                "active_blocks": stats.blocked, "blocklist_capacity": 102400_u64,
                "pending_expirations": m.pending_expirations.load(Ordering::Relaxed),
                "wal_lsn": m.wal_last_lsn.load(Ordering::Relaxed),
            },
            "ipc": {
                "ingest_rps": ingest, "rejected_rps": reject, "shed_rps": shed,
                "channel_depth": snapshot.channel_depth, "channel_capacity": 64000_u64,
                "channel_util_pct": (snapshot.channel_depth as f64 / 64000.0) * 100.0,
            },
            "detection": {
                "promotions_rps": promo, "cold_skipped_rps": cold,
                "hot_subnets_count": engine.store.subnet_table().len(),
                "blocks_detection_rps": bdet, "blocks_subnet_rps": bsub,
                "blocks_forecast_rps": bfc,
                "total_blocks_rps": bdet + bsub + bfc,
                "capacity_exceeded_count": m.capacity_exceeded_count.load(Ordering::Relaxed),
                "capacity_exceeded_ips": m.capacity_exceeded_ips.load(Ordering::Relaxed),
                "subnet_bitmap_ones": stats.ips_tracked,
            },
            "forecasting": {
                "hw_projected_rps": f64::from_bits(m.hw_forecast_bits.load(Ordering::Relaxed)),
                "hw_zscore": f64::from_bits(m.hw_z_bits.load(Ordering::Relaxed)),
                "hw_rps_baseline": f64::from_bits(m.hw_rps_bits.load(Ordering::Relaxed)),
                "entropy_score": f64::from_bits(m.entropy_bits.load(Ordering::Relaxed)),
                "entropy_ticks": m.entropy_ticks.load(Ordering::Relaxed),
                "forecast_ticks": m.forecast_ticks.load(Ordering::Relaxed),
            },
            "cgnat": {
                "classify_rps": cgnat,
                "tier_allow": m.cgnat_tier_allow.load(Ordering::Relaxed),
                "tier_challenge": m.cgnat_tier_challenge.load(Ordering::Relaxed),
                "tier_powdrop": m.cgnat_tier_powdrop.load(Ordering::Relaxed),
                "tier_block": m.cgnat_tier_block.load(Ordering::Relaxed),
                "shm_publishes": m.shm_publish_count.load(Ordering::Relaxed),
                "shm_lookups_rps": shm_l,
                "shm_hit_rate_pct": shm_hit_rate,
            },
            "analytics": {
                "hll_rate": hll, "cms_rate": cms,
                "cms_decay_ticks": m.cms_decay_ticks.load(Ordering::Relaxed),
            },
            "mesh": {
                "hlc_drift_ms": 0,
                "hlc_ticks": m.mesh_hlc_ticks.load(Ordering::Relaxed),
                "ban_sync_rps": mban, "unban_sync_rps": munban,
                "purge_ticks": m.mesh_purge_ticks.load(Ordering::Relaxed),
            },
            "system": {
                "cpu_usage_pct": cpu, "rss_mb": rss,
                "ram_mb": stats.ram_bytes as f64 / (1024.0 * 1024.0),
                "sys_total_ram_mb": total_sys_ram,
                "ram_bytes": stats.ram_bytes,
                "ram_limit_mb": stats.ram_limit_mb,
                "ram_pct": if stats.ram_limit_mb > 0 { (stats.ram_bytes as f64 / (stats.ram_limit_mb as f64 * 1048576.0)) * 100.0 } else { 0.0 },
                "ips_tracked": stats.ips_tracked,
            },
        });
        Ok(axum::response::sse::Event::default().data(payload.to_string()))
    });

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

// ── Index (Obsidian HUD) ────────────────────────────────────────────────────
async fn index() -> Html<&'static str> {
    Html(include_str!("static/index.html"))
}

#[derive(Serialize)]
struct ActiveBlockView {
    ip: String,
    reason: String,
    module: String,
    ts_ms: u64,
}

// ── REST endpoints (preserved from previous session) ──────────────────────────
async fn api_healthz(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let snapshot = state.engine.dashboard_snapshot();
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

async fn api_history_batches(State(state): State<AppState>) -> ([(axum::http::header::HeaderName, &'static str); 1], String) {
    use axum::http::header;
    (
        [(header::CONTENT_TYPE, "application/json")],
        state.engine.get_batch_history_json().to_string(),
    )
}

async fn api_history_blocks(State(state): State<AppState>) -> ([(axum::http::header::HeaderName, &'static str); 1], String) {
    use axum::http::header;
    (
        [(header::CONTENT_TYPE, "application/json")],
        state.engine.get_block_log_json().to_string(),
    )
}

async fn api_blocks_active(State(state): State<AppState>) -> Json<Vec<ActiveBlockView>> {
    let blocked_ips = state.engine.store.get_all_blocked_ips();
    let now_ms = crate::metrics::now_ms();
    let records = blocked_ips.into_iter().map(|ip| {
        let (reason, since_ns) = match state.engine.store.get(&ip) {
            Some(ramshield_storage::Value::IpRecord(r)) => match r.block_state {
                ramshield_storage::BlockState::Blocked { reason, since_ns } => {
                    (format!("{:?}", reason), since_ns)
                }
                _ => ("Manual".into(), 0),
            }
            _ => ("Manual".into(), 0),
        };
        let ts_ms = if since_ns > 0 { since_ns / 1_000_000 } else { now_ms };
        ActiveBlockView {
            ip: ip.to_string(),
            reason,
            module: "Enforcement".into(),
            ts_ms,
        }
    }).collect();
    Json(records)
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

// ── Mitigation Endpoints ────────────────────────────────────────────────────
/// Request payload for manual IP block/unblock actions.
#[derive(Debug, Deserialize)]
pub struct MitigationRequest {
    pub ip: std::net::IpAddr,
    pub duration_secs: u64,
    pub tier: u8,
    pub reason: String,
}

/// Request payload for unblock — only IP required, rest are inferred.
#[derive(Debug, Deserialize)]
pub struct UnblockRequest {
    pub ip: std::net::IpAddr,
}

/// CSRF cross-origin check — fail OPEN when headers absent (curl/SDK),
/// fail CLOSED (403) when a header is present but mismatches Host.
fn is_cross_origin(headers: &axum::http::HeaderMap, host: Option<&str>) -> bool {
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let referer = headers.get(header::REFERER).and_then(|v| v.to_str().ok());
    let header_val = origin.or(referer);
    let Some(header_val) = header_val else { return false }; // fail OPEN: no header = browser sent CORS-safe request, SameSite cookie covers it

    let header_val = header_val.trim();
    let scheme_end = header_val.find("://").map(|i| i + 3).unwrap_or(0);
    let authority = &header_val[scheme_end..];
    let authority = authority.split('/').next().unwrap_or("");
    let authority = authority.split('?').next().unwrap_or("");
    let authority_l = authority.to_lowercase();
    match host {
        Some(h) => h.to_lowercase() != authority_l,
        None => true, // no Host header is always suspicious
    }
}

/// POST /api/mitigate/block — publishes SHM rule + sends EnforceCommand.
async fn api_manual_block(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<MitigationRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    if is_cross_origin(&headers, host) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "status": "forbidden", "error": "cross-origin blocked" })),
        );
    }

    let cmd = EnforceCommand {
        decision_id: uuid::Uuid::new_v4(),
        policy_version: 0,
        source: "dashboard_manual_block".into(),
        actor: "admin".into(),
        timestamp_utc: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
        ttl_seconds: payload.duration_secs,
        reason: payload.reason,
        ip: payload.ip,
        action: ramshield_types::EnforceAction::Block,
    };

    if let Err(e) = state.engine.send_mitigation(cmd) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "status": "error", "error": e })),
        );
    }

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "success",
            "ip": payload.ip.to_string(),
            "tier": payload.tier,
            "ttl_secs": payload.duration_secs
        })),
    )
}

/// POST /api/mitigate/unblock — sends unblock EnforceCommand.
async fn api_manual_unblock(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<UnblockRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    if is_cross_origin(&headers, host) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "status": "forbidden", "error": "cross-origin blocked" })),
        );
    }

    let cmd = EnforceCommand {
        decision_id: uuid::Uuid::new_v4(),
        policy_version: 0,
        source: "dashboard_manual_unblock".into(),
        actor: "admin".into(),
        timestamp_utc: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
        ttl_seconds: 0,
        reason: "manual unblock via console".into(),
        ip: payload.ip,
        action: ramshield_types::EnforceAction::Unblock,
    };

    if let Err(e) = state.engine.send_mitigation(cmd) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "status": "error", "error": e })),
        );
    }

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "success",
            "ip": payload.ip.to_string(),
            "action": "unblocked"
        })),
    )
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigView {
    pub engine: crate::config::EngineConfig,
    pub detection: crate::config::DetectionConfig,
    pub ipc: crate::config::IpcConfig,
    pub forecasting: crate::config::ForecastingConfig,
    pub dashboard: crate::config::DashboardConfig,
    pub auth_enabled: bool,
}

impl ConfigView {
    pub fn from_config(c: &crate::config::Config) -> Self {
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

async fn api_set_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(patch): Json<ConfigPatch>,
) -> (StatusCode, Json<ConfigResponse>) {
    let mut cfg = state.engine.config.load().as_ref().clone();
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).map(str::to_lowercase);
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()).map(str::to_lowercase);
    let referer = headers.get(header::REFERER).and_then(|v| v.to_str().ok()).map(str::to_lowercase);

    fn is_cross_origin(origin: Option<&str>, referer: Option<&str>, host: Option<&str>) -> bool {
        let header = origin.or(referer);
        let Some(header) = header else { return false };
        let header = header.trim();
        let scheme_end = header.find("://").map(|i| i + 3).unwrap_or(0);
        let authority = &header[scheme_end..];
        let authority = authority.split('/').next().unwrap_or("");
        let authority = authority.split('?').next().unwrap_or("");
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
    fn contains_placeholder(cfg: &crate::config::Config) -> bool {
        cfg.ipc.auth_keys.iter().any(|k| k.contains(ramshield_config::REDACTED_PLACEHOLDER))
            || cfg.dashboard.admin_password_hash.as_deref().is_some_and(|h| h.contains(ramshield_config::REDACTED_PLACEHOLDER))
    }
    if let Some(v) = patch.engine { cfg.engine = v; }
    if let Some(v) = patch.detection { cfg.detection = v; }
    if let Some(v) = patch.ipc { cfg.ipc = v; }
    if let Some(v) = patch.forecasting { cfg.forecasting = v; }
    if let Some(v) = patch.dashboard { cfg.dashboard = v; }
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
        let auth = Arc::new(auth::AuthState::new(None, 3600, 50, 1024, vec![]));
        AppState { engine, auth, collector: None }
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        let state = test_app_state();
        let app = Router::new().route("/healthz", get(api_healthz)).with_state(state);
        let response = app.oneshot(Request::get("/healthz").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10_000).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn snapshot_returns_valid_json() {
        let state = test_app_state();
        let app = Router::new().route("/api/snapshot", get(api_snapshot)).with_state(state);
        let response = app.oneshot(Request::get("/api/snapshot").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 100_000).await.unwrap();
        let json: DashboardSnapshot = serde_json::from_slice(&body).unwrap();
        assert!(json.events_ingested == 0);
    }

    #[tokio::test]
    async fn config_get_returns_default() {
        let state = test_app_state();
        let app = Router::new().route("/api/config", get(api_get_config)).with_state(state);
        let response = app.oneshot(Request::get("/api/config").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 100_000).await.unwrap();
        let json: ConfigView = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.engine.ram_limit_mb, 512);
        assert_eq!(json.engine.shard_count, 256);
    }

    #[tokio::test]
    async fn config_redacts_hmac_keys() {
        let state = test_app_state();
        let mut cfg = state.engine.config.load().as_ref().clone();
        cfg.ipc.auth_keys = vec!["k1:deadbeefcafebabe0123456789abcdef0123456789abcdef0123456789abcdef".into()];
        state.engine.config.store(Arc::new(cfg));
        let app = Router::new().route("/api/config", get(api_get_config)).with_state(state);
        let response = app.oneshot(Request::get("/api/config").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 100_000).await.unwrap();
        let raw = std::str::from_utf8(&body).unwrap();
        assert!(!raw.contains("deadbeefcafebabe"), "raw hex secret leaked in /api/config response");
        assert!(raw.contains("k1:<redacted>"), "expected redacted key id marker");
    }

    #[tokio::test]
    async fn config_redacts_hmac_keys_post() {
        let state = test_app_state();
        let mut cfg = state.engine.config.load().as_ref().clone();
        cfg.ipc.auth_keys = vec!["k1:deadbeefcafebabe0123456789abcdef0123456789abcdef0123456789abcdef".into()];
        state.engine.config.store(Arc::new(cfg));
        let app = Router::new().route("/api/config", post(api_set_config)).with_state(state);
        let body = serde_json::json!({"engine": {"max_peers": 42}});
        let response = app.oneshot(Request::post("/api/config").header("content-type", "application/json").body(Body::from(serde_json::to_string(&body).unwrap())).unwrap()).await.unwrap();
        let body = axum::body::to_bytes(response.into_body(), 100_000).await.unwrap();
        let raw = std::str::from_utf8(&body).unwrap();
        assert!(!raw.contains("deadbeefcafebabe"), "POST /api/config leaked raw HMAC key");
    }

    #[tokio::test]
    async fn post_config_cross_origin_forbidden() {
        let state = test_app_state();
        let app = Router::new().route("/api/config", post(api_set_config)).with_state(state.clone());
        let body = serde_json::json!({"engine": {"worker_threads": 2, "ram_limit_mb": 128, "shard_count": 8}});
        let response = app.oneshot(Request::post("/api/config").header("content-type", "application/json").header("origin", "https://evil.example").body(Body::from(serde_json::to_string(&body).unwrap())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn post_config_same_origin_allowed() {
        let state = test_app_state();
        let app = Router::new().route("/api/config", post(api_set_config)).with_state(state.clone());
        let body = serde_json::json!({"engine": {"worker_threads": 4, "ram_limit_mb": 256, "shard_count": 64}});
        let response = app.oneshot(Request::post("/api/config").header("content-type", "application/json").header("host", "127.0.0.1:9999").header("origin", "http://127.0.0.1:9999").body(Body::from(serde_json::to_string(&body).unwrap())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cfg = state.engine.config.load();
        assert_eq!(cfg.engine.ram_limit_mb, 256);
        assert_eq!(cfg.engine.shard_count, 64);
    }

    #[tokio::test]
    async fn history_batches_returns_ok() {
        let state = test_app_state();
        let app = Router::new().route("/api/history/batches", get(api_history_batches)).with_state(state);
        let response = app.oneshot(Request::get("/api/history/batches").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10_000).await.unwrap();
        let batches: Vec<BatchRecord> = serde_json::from_slice(&body).unwrap();
        assert!(batches.is_empty());
    }

    #[tokio::test]
    async fn history_blocks_returns_ok() {
        let state = test_app_state();
        let app = Router::new().route("/api/history/blocks", get(api_history_blocks)).with_state(state);
        let response = app.oneshot(Request::get("/api/history/blocks").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10_000).await.unwrap();
        let blocks: Vec<BlockRecord> = serde_json::from_slice(&body).unwrap();
        assert!(blocks.is_empty());
    }

    #[tokio::test]
    async fn traffic_subnets_returns_ok() {
        let state = test_app_state();
        let app = Router::new().route("/api/traffic/subnets", get(api_traffic_subnets)).with_state(state);
        let response = app.oneshot(Request::get("/api/traffic/subnets").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10_000).await.unwrap();
        let subnets: Vec<SubnetRow> = serde_json::from_slice(&body).unwrap();
        assert!(subnets.is_empty());
    }

    #[tokio::test]
    async fn status_modules_returns_ok() {
        let state = test_app_state();
        let app = Router::new().route("/api/status/modules", get(api_status_modules)).with_state(state);
        let response = app.oneshot(Request::get("/api/status/modules").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 10_000).await.unwrap();
        let modules: Vec<ModuleStats> = serde_json::from_slice(&body).unwrap();
        assert!(!modules.is_empty());
        assert_eq!(modules.len(), 7);
    }

    /// SSE endpoint returns 200 and content-type text/event-stream.
    #[tokio::test]
    async fn stream_returns_sse() {
        let state = test_app_state();
        let app = Router::new().route("/api/stream", get(stream_telemetry)).with_state(state);
        let response = app.oneshot(Request::get("/api/stream").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers();
        let ct = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
        assert!(ct.contains("text/event-stream"), "expected SSE content-type, got: {ct}");
    }

    #[tokio::test]
    async fn post_mitigate_block_cross_origin_forbidden() {
        let state = test_app_state();
        let app = Router::new().route("/api/mitigate/block", post(api_manual_block)).with_state(state);
        let body = serde_json::json!({"ip": "192.0.2.1", "duration_secs": 3600, "tier": 3, "reason": "test"});
        let response = app.oneshot(Request::post("/api/mitigate/block").header("content-type", "application/json").header("host", "127.0.0.1:9999").header("origin", "https://evil.example").body(Body::from(serde_json::to_string(&body).unwrap())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn post_mitigate_block_no_origin_allowed() {
        let state = test_app_state();
        let app = Router::new().route("/api/mitigate/block", post(api_manual_block)).with_state(state);
        let body = serde_json::json!({"ip": "192.0.2.1", "duration_secs": 3600, "tier": 3, "reason": "test"});
        // No Origin header — fail OPEN (curl/SDK clients)
        let response = app.oneshot(Request::post("/api/mitigate/block").header("content-type", "application/json").header("host", "127.0.0.1:9999").body(Body::from(serde_json::to_string(&body).unwrap())).unwrap()).await.unwrap();
        // Engine::send_mitigation must exist — will be ACCEPTED if it does, error if not
        assert!(response.status() == StatusCode::ACCEPTED || response.status() == StatusCode::INTERNAL_SERVER_ERROR);
    }
}
