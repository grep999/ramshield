use ramshield::doctor::{check_config_and_schema, check_statistical_brains, diagnose, load_config, CheckStatus};
use ramshield::Config;
use std::path::PathBuf;

fn temp_config(contents: &str, name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ramshield-doctor-{}-{name}.toml",
        std::process::id()
    ));
    std::fs::write(&path, contents).expect("write test config");
    path
}

#[test]
fn valid_default_config_passes_schema_check() {
    let config = Config::default();
    let checks = check_config_and_schema(None, Some(&config));
    assert_eq!(checks[0].status, CheckStatus::Pass);
}

#[test]
fn corrupted_config_is_reported_not_panicked() {
    let path = temp_config("[forecasting\n ewma_alpha = 4.2", "bad");
    let report = diagnose(Some(&path), Some("config"));
    let _ = std::fs::remove_file(&path);
    assert!(report.has_failures());
    assert!(report.checks.iter().any(|c| c.subsystem == "config" && c.status == CheckStatus::Fail));
}

#[test]
fn valid_toml_with_invalid_schema_fails() {
    let mut config = Config::default();
    config.forecasting.ewma_alpha = 4.2;
    let checks = check_config_and_schema(None, Some(&config));
    assert!(checks.iter().any(|c| c.status == CheckStatus::Fail));
}

#[test]
fn zero_rate_threshold_is_flagged() {
    let mut config = Config::default();
    config.detection.rps_threshold = 0;
    let checks = check_statistical_brains(&config);
    assert!(checks.iter().any(|c| c.name == "Rate/CUSUM prerequisites" && c.status == CheckStatus::Fail));
}

#[test]
fn invalid_subsystem_is_a_failure() {
    let report = diagnose(None, Some("made-up"));
    assert!(report.has_failures());
}

#[test]
fn missing_config_path_returns_an_error() {
    let path = PathBuf::from("/definitely/not/a/ramshield/config.toml");
    assert!(load_config(&path).is_err());
}
