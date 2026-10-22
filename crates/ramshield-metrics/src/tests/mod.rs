use super::*;

#[test]
fn xdp_projection_stale_gauge_is_exported() {
    let m = Metrics::new();
    m.set_xdp_projection_active(true);
    assert!(
        m.render_prometheus()
            .contains("ramshield_xdp_projection_stale 1")
    );
    m.set_xdp_projection_active(false);
    assert!(
        m.render_prometheus()
            .contains("ramshield_xdp_projection_stale 0")
    );
}

#[test]
fn enforcement_drops_counter_is_exported() {
    let m = Metrics::new();
    m.inc_enforcement_dropped();
    m.inc_enforcement_dropped();
    assert_eq!(m.enforcement_dropped.load(Ordering::Relaxed), 2);
    let out = m.render_prometheus();
    assert!(
        out.contains("ramshield_enforcement_dropped_total 2"),
        "enforcement drops must be scrapeable — a silently dropped security \
             command is otherwise invisible to the operator"
    );
}

#[test]
fn xdp_apply_failures_counter_is_exported() {
    // The mirror of enforcement_dropped for the dataplane leg: when the
    // CIDR LPM trie fills (no LRU, hard cap) a subnet block silently stops
    // reaching the wire. This counter is the only scrapeable signal that
    // happened.
    let m = Metrics::new();
    m.inc_xdp_apply_failures();
    m.inc_xdp_apply_failures();
    m.inc_xdp_apply_failures();
    assert_eq!(m.xdp_apply_failures.load(Ordering::Relaxed), 3);
    let out = m.render_prometheus();
    assert!(
        out.contains("ramshield_xdp_apply_failures_total 3"),
        "XDP apply failures must be scrapeable — a silently full CIDR trie \
             otherwise looks like the subnet mitigation is simply not firing"
    );
}

#[test]
fn block_log_evicts_at_configured_cap() {
    let m = Metrics::with_block_log(5);
    for i in 0..12 {
        m.record_block(&format!("10.0.0.{i}"), "high_rps", "detection");
    }
    let log = m.block_log.lock().unwrap();
    assert_eq!(log.len(), 5, "ring must evict oldest beyond cap");
    // newest survives, oldest gone
    assert_eq!(log.back().unwrap().ip, "10.0.0.11");
    assert_eq!(log.front().unwrap().ip, "10.0.0.7");
}

#[test]
fn block_log_cap_floor_is_one() {
    // zero/nonsense config must not produce a zero-capacity deadlock ring
    let m = Metrics::with_block_log(0);
    m.record_block("10.0.0.1", "high_rps", "detection");
    assert_eq!(m.block_log.lock().unwrap().len(), 1);
}

#[test]
fn native_ingest_drop_counters_are_exported() {
    let m = Metrics::new();
    let rendered = m.render_prometheus();
    assert!(
        rendered.contains("ramshield_native_ingest_drops_total 0"),
        "the native ingest drop counter must be exported:\n{rendered}"
    );
    assert!(
        rendered.contains("ramshield_native_ingest_drops_disconnected_total 0"),
        "the native ingest disconnect counter must be exported:\n{rendered}"
    );

    m.inc_native_ingest_drop(3);
    m.inc_native_ingest_disconnect(1);
    let rendered = m.render_prometheus();
    assert!(
        rendered.contains("ramshield_native_ingest_drops_total 3"),
        "the counter must reflect the drops:\n{rendered}"
    );
    assert!(
        rendered.contains("ramshield_native_ingest_drops_disconnected_total 1"),
        "the disconnect counter must be broken out:\n{rendered}"
    );
}

#[test]
fn the_capture_worker_counters_are_exported() {
    // F-2: the native capture path used to lose its workers without a
    // word. The counters are how an operator sees it.
    let m = Metrics::new();
    let rendered = m.render_prometheus();
    for series in [
        "ramshield_native_capture_workers_expected 0",
        "ramshield_native_capture_workers_started 0",
        "ramshield_native_capture_workers_running 0",
        "ramshield_native_capture_worker_failures_total 0",
    ] {
        assert!(
            rendered.contains(series),
            "the capture counter `{series}` must be exported:\n{rendered}"
        );
    }

    m.set_native_capture_workers_expected(8);
    m.set_native_capture_workers_started(7);
    m.inc_native_capture_workers_running();
    m.inc_native_capture_worker_failure();
    let rendered = m.render_prometheus();
    assert!(
        rendered.contains("ramshield_native_capture_workers_expected 8"),
        "the planned worker count must be exported:\n{rendered}"
    );
    assert!(
        rendered.contains("ramshield_native_capture_workers_started 7"),
        "the started worker count must be exported:\n{rendered}"
    );
    assert!(
        rendered.contains("ramshield_native_capture_workers_running 1"),
        "the running worker count must be exported:\n{rendered}"
    );
    assert!(
        rendered.contains("ramshield_native_capture_worker_failures_total 1"),
        "the failure counter must be exported:\n{rendered}"
    );

    m.dec_native_capture_workers_running();
    assert_eq!(m.native_capture_workers_running.load(Ordering::Relaxed), 0);
}

#[test]
fn ingest_channel_depth_peak_is_monotonic_and_exported() {
    let m = Metrics::new();
    m.set_channel_depth(10);
    m.set_channel_depth(25);
    // A later, lower reading must not lower the high-water mark.
    m.set_channel_depth(3);
    assert_eq!(m.ingest_channel_depth.load(Ordering::Relaxed), 3);
    assert_eq!(m.ingest_channel_depth_peak.load(Ordering::Relaxed), 25);

    let rendered = m.render_prometheus();
    assert!(
        rendered.contains("ramshield_ingest_channel_depth 3"),
        "the current depth must be exported:\n{rendered}"
    );
    assert!(
        rendered.contains("ramshield_ingest_channel_depth_peak 25"),
        "the high-water mark must be exported:\n{rendered}"
    );
}

#[test]
fn a_mutation_failure_keeps_the_projection_stale_until_a_successful_reconcile() {
    use std::sync::atomic::Ordering;
    let m = Metrics::new();
    m.set_xdp_projection_active(true);
    m.record_reconcile_success(1_000);

    // A failed XDP apply raises the flag on its own.
    m.mark_xdp_projection_stale();
    assert_eq!(
        m.xdp_projection_stale.load(Ordering::Relaxed),
        1,
        "a failed apply must raise the stale flag"
    );

    // The 250 ms tick recomputes the age. It must not clear the flag.
    m.tick_reconcile_age(1_001);
    assert_eq!(
        m.xdp_projection_stale.load(Ordering::Relaxed),
        1,
        "the age tick must not clear a stale flag raised by a failed apply"
    );

    // A successful reconcile is what clears it.
    m.record_reconcile_success(1_002);
    assert_eq!(
        m.xdp_projection_stale.load(Ordering::Relaxed),
        0,
        "a successful reconcile must clear the stale flag"
    );
}
