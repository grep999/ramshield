//! F-31, F-39, F-40, F-46, F-47: the dashboard must show the numbers the
//! daemon actually has.
//!
//! - F-31: the SSE writer stored the rates under `drops_total_pps`,
//!   `ingest_total_pps` and `shed_total_pps`, but the readouts read
//!   `drops_pps`, `ingest_pps` and `shed_pps`. `fmt` returns `'0'` for an
//!   undefined key, so every rate on the page was 0 at any traffic - the
//!   headline KPIs and the overview rows.
//! - F-39: the "Drops vs Ingress" chart pushed a literal 0 for drops, so
//!   the red series stayed glued to the axis.
//! - F-40: the "Blocklist" row compared a lifetime counter with a
//!   hardcoded capacity, so it pinned at 100% after 102,400 lifetime
//!   blocks.
//! - F-46: the pipeline stage bars mixed units - a flush count against
//!   an IP count, and a current gauge against cumulative counters.
//! - F-47: the quarantine table hardcoded `CRITICAL` and `Tier 3 (XDP)`
//!   for every row while the record carries a real reason.
//!
//! The JavaScript has no test harness in this repo, so the guard reads
//! the shipped page and the stream payload and pins what they must not
//! say.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel} must be readable: {e}"))
}

const PAGE: &str = "src/dashboard/static/index.html";

#[test]
fn the_rate_readouts_use_the_keys_the_stream_writes() {
    let html = read(PAGE);
    // The writer: dSlots[k+'_pps'] with k in {drops_total, ingest_total, ...}.
    for key in ["drops_total_pps", "ingest_total_pps", "shed_total_pps"] {
        assert!(
            html.contains(&format!("dSlots.{key}")),
            "the rate readout must read dSlots.{key}, the key the stream writes"
        );
    }
    for stale in ["dSlots.drops_pps", "dSlots.ingest_pps", "dSlots.shed_pps"] {
        assert!(
            !html.contains(stale),
            "{stale} is never written by the stream writer, so the readout would always be 0"
        );
    }
    // The EWMA table must use the same key names the writer uses.
    assert!(
        html.contains("drops_total_pps: 0"),
        "the EWMA table must key the rates the writer produces"
    );
}

#[test]
fn the_chart_plots_the_real_drop_rate() {
    let html = read(PAGE);
    assert!(
        !html.contains("pushTelemetry(0,"),
        "the chart must not push a literal 0 for drops"
    );
    assert!(
        html.contains("pushTelemetry(dSlots.drops_total_pps||0"),
        "the chart must push the stream's drop rate"
    );
}

#[test]
fn the_blocklist_row_is_not_a_lifetime_counter_over_a_capacity() {
    let html = read(PAGE);
    assert!(
        !html.contains("od-blcap').textContent=fmt(snap.blocked_total,0)+' / '"),
        "the blocklist row must not divide a lifetime counter by a capacity"
    );
    assert!(
        html.contains("Blocks (lifetime)"),
        "the row must be labelled as what it shows"
    );
}

#[test]
fn the_quarantine_table_does_not_fabricate_the_severity_and_the_tier() {
    let html = read(PAGE);
    assert!(
        !html.contains("'CRITICAL'"),
        "the quarantine table must not hardcode the severity"
    );
    assert!(
        !html.contains("'Tier 3 (XDP)'"),
        "the quarantine table must not hardcode the tier"
    );
    assert!(
        html.contains("b.reason||'-'"),
        "the severity column must show the record's reason"
    );
}

#[test]
fn the_pipeline_stages_do_not_mix_units() {
    let dashboard = read("src/dashboard/mod.rs");
    // The cold stage's ingress was a flush count (pipeline.batched) while
    // its drops were an IP count; the detection stage added a current
    // gauge (pipeline.merged) to cumulative counters.
    assert!(
        !dashboard.contains("\"ingress\":snapshot.pipeline.batched"),
        "the cold stage's ingress must be an IP count, not a flush count"
    );
    assert!(
        !dashboard.contains("snapshot.pipeline.merged"),
        "a current gauge must not be added to the cumulative stage counters"
    );
}
