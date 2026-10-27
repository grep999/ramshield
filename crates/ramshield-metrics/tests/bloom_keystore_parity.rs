//! E07: bloom metrics keystore ↔ Prometheus export parity.
//!
//! The bloom advisory-cache series are written via the Metrics public API
//! and must appear under the `ramshield_` namespace with the values the
//! counters hold. A series that is counted but never exported is a silent
//! observability gap (same class as F-36/F-37 for authz/capacity).

use ramshield_metrics::Metrics;
use std::sync::atomic::Ordering;

/// Canonical bloom keystore: every name the counters module documents must
/// be emitted by `render_prometheus`. Keep this list in lock-step with
/// `src/prometheus.rs` bloom block and `src/counters.rs` bloom helpers.
const BLOOM_KEYSTORE: &[&str] = &[
    "ramshield_bloom_bits",
    "ramshield_bloom_inserts_epoch",
    "ramshield_bloom_clears_total",
    "ramshield_bloom_saturation_clears_total",
    "ramshield_bloom_fp_ppm",
];

#[test]
fn bloom_keystore_series_are_all_exported() {
    let text = Metrics::new().render_prometheus();
    for series in BLOOM_KEYSTORE {
        assert!(
            text.contains(&format!("# TYPE {series} ")),
            "keystore series `{series}` missing TYPE line in export:\n{text}"
        );
        // A TYPE line alone is not enough — the sample line must exist.
        let has_sample = text.lines().any(|l| {
            l.starts_with(series) && !l.starts_with('#')
        });
        assert!(
            has_sample,
            "keystore series `{series}` has TYPE but no sample value:\n{text}"
        );
    }
}

#[test]
fn bloom_keystore_export_matches_counter_values() {
    let m = Metrics::new();
    m.set_bloom_bits(1_000_000);
    m.record_bloom_inserts(50_000);
    m.bloom_epoch_clear(); // inserts_epoch -> 0, clears_total += 1
    m.record_bloom_inserts(25_000);
    m.record_bloom_saturation_clear();

    let text = m.render_prometheus();

    assert!(
        text.contains("ramshield_bloom_bits 1000000"),
        "bloom_bits export mismatch:\n{text}"
    );
    assert!(
        text.contains("ramshield_bloom_inserts_epoch 25000"),
        "inserts_epoch must reflect post-clear inserts:\n{text}"
    );
    assert!(
        text.contains("ramshield_bloom_clears_total 1"),
        "clears_total must count epoch clears:\n{text}"
    );
    assert!(
        text.contains("ramshield_bloom_saturation_clears_total 1"),
        "saturation clears must be exported:\n{text}"
    );

    // fp_ppm = (1 - e^(-2n/m))^2 * 1e6 with n=25000, m=1e6, k=2
    let bits = m.bloom_bits.load(Ordering::Relaxed);
    let n = m.bloom_inserts_epoch.load(Ordering::Relaxed);
    let ratio = 2.0 * n as f64 / bits as f64;
    let expected_ppm = ((1.0 - (-ratio).exp()).powi(2) * 1_000_000.0) as u64;
    assert!(
        text.contains(&format!("ramshield_bloom_fp_ppm {expected_ppm}")),
        "bloom_fp_ppm must match derived estimate ({expected_ppm}):\n{text}"
    );
}

#[test]
fn bloom_keystore_is_namespaced() {
    let text = Metrics::new().render_prometheus();
    for line in text.lines() {
        if line.contains("bloom") && line.starts_with("# TYPE ") {
            let series = line
                .strip_prefix("# TYPE ")
                .and_then(|r| r.split(' ').next())
                .unwrap_or("");
            assert!(
                series.starts_with("ramshield_"),
                "bloom series `{series}` must use the ramshield_ namespace"
            );
        }
    }
}
