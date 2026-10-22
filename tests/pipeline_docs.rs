//! The daemon's pipeline description must match what the daemon actually
//! builds.
//!
//! This test used to assert the opposite: that `src/README.md` did not
//! claim forecasting was in the pipeline and that `boot_pipeline()` did
//! not construct a `Forecaster`. The daemon never constructed one, so a
//! reader who believed the README would have believed forecasting was an
//! active mitigation path when it was not.
//!
//! The forecasting subsystem is now wired (I-034), so the check is
//! inverted: the README must describe the forecaster as a pipeline step
//! and the boot path must construct one. The two sides move together -
//! the file that says forecasting runs and the code that runs it.

const README: &str = include_str!("../src/README.md");
const BOOT: &str = include_str!("../src/engine/boot.rs");

#[test]
fn readme_describes_forecasting_as_a_pipeline_step() {
    assert!(
        README.contains("**Forecasting** —"),
        "src/README.md must list Forecasting as a boot-pipeline step"
    );
    assert!(
        !README.contains("Forecasting is not in the pipeline"),
        "src/README.md must not say the daemon does not start the forecaster"
    );
    // The prose and the diagram are read separately, and a reader of
    // either one takes it at face value. A fix that corrected the prose
    // and left `Forecaster::new()` out of the pipeline diagram left the
    // file saying forecasting was in the pipeline while the code did not
    // run it.
    assert!(
        README.contains("Forecaster::new(store, config, enforcement_tx, metrics)"),
        "src/README.md must show how the forecaster is constructed"
    );
}

#[test]
fn the_daemon_constructs_a_forecaster() {
    assert!(
        BOOT.contains("Forecaster::new("),
        "the boot pipeline must construct a Forecaster - update src/README.md and this test together"
    );
    assert!(
        BOOT.contains("cfg_snapshot.forecasting.enabled"),
        "the forecaster must follow the config's forecasting.enabled"
    );
}
