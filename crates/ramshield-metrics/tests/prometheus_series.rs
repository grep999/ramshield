//! F-36, F-37, F-45: the exported Prometheus series.
//!
//! - F-45: `auth_lockout_total` and `auth_verification_wait_ms` were
//!   exported without the `ramshield_` prefix every other series
//!   carries, so they collided with another exporter's names in a
//!   shared Prometheus.
//! - F-36: the 403 rejections were counted in `ipc_authz_rejections`
//!   but never exported, so the breakdown the field's own doc comment
//!   promises did not sum to the aggregate.
//! - F-37: the store's RAM refusals were counted in
//!   `capacity_exceeded_count` / `capacity_exceeded_ips` but no surface
//!   exposed them, and the IP was dropped from tracking entirely.

use ramshield_metrics::Metrics;
use std::sync::atomic::Ordering;

#[test]
fn every_exported_series_carries_the_namespace() {
    let text = Metrics::new().render_prometheus();
    let mut checked = 0;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("# TYPE ") else {
            continue;
        };
        let series = rest.split(' ').next().unwrap_or_default();
        assert!(
            series.starts_with("ramshield_"),
            "the exported series {series} must carry the ramshield_ namespace"
        );
        checked += 1;
    }
    assert!(
        checked > 20,
        "the exporter must publish its series: {checked}"
    );
}

#[test]
fn the_counted_but_unexported_counters_are_exported() {
    let m = Metrics::new();
    m.ipc_authz_rejections.fetch_add(2, Ordering::Relaxed);
    m.capacity_exceeded_count.fetch_add(3, Ordering::Relaxed);
    m.capacity_exceeded_ips.fetch_add(3, Ordering::Relaxed);
    m.auth_lockout_total.fetch_add(1, Ordering::Relaxed);

    let text = m.render_prometheus();
    assert!(
        text.contains("ramshield_ipc_authz_rejections_total 2"),
        "the 403 rejections must be exported:\n{text}"
    );
    assert!(
        text.contains("ramshield_store_ram_refusals_total 3"),
        "the store's RAM refusals must be exported:\n{text}"
    );
    assert!(
        text.contains("ramshield_store_ram_refused_ips_total 3"),
        "the refused IPs must be exported:\n{text}"
    );
    assert!(
        text.contains("ramshield_auth_lockout_total 1"),
        "the lockout counter must be exported under the namespaced name:\n{text}"
    );
    assert!(
        !text.contains("\nauth_lockout_total"),
        "the unnamespaced series must be gone:\n{text}"
    );
}
