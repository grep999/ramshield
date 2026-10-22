//! I-034 and F-23: the forecasting subsystem must run, and its
//! `min_entropy` knob must be read.
//!
//! I-034: the `Forecaster` was implemented and unit-tested, but
//! `src/engine/boot.rs` never constructed it, so the Holt-Winters
//! forecast, the CUSUM drift detector, the Bayesian hypothesis tracker
//! and the entropy signal never ran in production.
//!
//! F-23: `forecasting.min_entropy` was validated by the config crate
//! and documented, and nothing read it.
//!
//! The knob's effect is a log line, so the guard is a source check: the
//! daemon must construct the forecaster and the forecaster must read the
//! knob.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel} must be readable: {e}"))
}

#[test]
fn the_daemon_constructs_the_forecaster() {
    let boot = read("src/engine/boot.rs");
    assert!(
        boot.contains("ramshield_forecasting::Forecaster::new("),
        "the daemon must construct the forecaster, or the whole forecasting subsystem \
         never runs"
    );
    assert!(
        boot.contains("forecaster.run().await"),
        "the forecaster must be run"
    );
    assert!(
        boot.contains("cfg_snapshot.forecasting.enabled"),
        "the forecaster must follow the config's forecasting.enabled"
    );
}

#[test]
fn the_forecaster_reads_the_min_entropy_knob() {
    let forecaster = read("crates/ramshield-forecasting/src/forecaster.rs");
    assert!(
        forecaster.contains("self.config.min_entropy"),
        "the forecaster must read forecasting.min_entropy, or the knob is validated and \
         never read"
    );
}
