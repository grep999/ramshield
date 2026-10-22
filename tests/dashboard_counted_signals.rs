//! F-36, F-37, F-41, F-45: the counters that are counted but never
//! reach the operator.
//!
//! - F-36: the 403 rejections were counted in `ipc_authz_rejections` and
//!   never exported, so the breakdown the field's own doc comment
//!   promises did not sum to the aggregate.
//! - F-37: the store's RAM refusals were counted in
//!   `capacity_exceeded_count` / `capacity_exceeded_ips` and no surface
//!   exposed them - and the IP was dropped from tracking entirely.
//! - F-41: the 100 ms SSE path overwrote the "Cap Exceeded" and "Subnet
//!   Bitmap" rows with the literal `n/a`, so the 5 s refresh's values
//!   were visible for at most one frame.
//! - F-45: two exported series carried no `ramshield_` namespace.
//!
//! The Prometheus side is pinned by
//! `crates/ramshield-metrics/tests/prometheus_series.rs`. This test pins
//! the dashboard side: the SSE payload must carry the counters, and the
//! rows must not be overwritten with `n/a`.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel} must be readable: {e}"))
}

#[test]
fn the_dashboard_rows_are_not_overwritten_with_na() {
    let html = read("src/dashboard/static/index.html");
    for id in ["od-capex", "od-submap"] {
        assert!(
            !html.contains(&format!("$('{id}').textContent='n/a'")),
            "{id} must not be overwritten with the literal 'n/a' by the SSE path"
        );
    }
    assert!(
        html.contains("$('od-capex').textContent=fmt(det.capacity_exceeded_total"),
        "the Cap Exceeded row must show the store's RAM refusals"
    );
}

#[test]
fn the_stream_payload_carries_the_counters() {
    let dashboard = read("src/dashboard/mod.rs");
    assert!(
        dashboard.contains("\"capacity_exceeded_total\""),
        "the SSE payload must carry the store's RAM refusals"
    );
    assert!(
        dashboard.contains("\"capacity_refused_ips_total\""),
        "the SSE payload must carry the refused IP count"
    );
}
