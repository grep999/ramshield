//! The forecasting README must agree with the crate.
//!
//! The first guard was deliberately narrow: it checked two claims that had
//! been wrong. An audit of all 64 claims in the file then found 32 false
//! ones - a `HoltWinters` struct with four public fields where the crate
//! has four public fields and four private ones, a period that defaults to
//! 120 ticks where the config says 3,600, "three independent models" where
//! the models are chained, a `CusumState` with four public fields where
//! all four are private, an `EwmAVar` described as "3 floats" where it is
//! four fields, a threshold of 0.95 where the code uses 0.75, and a
//! dependency on `ramshield-enforcement` the crate does not have.
//!
//! The guard below checks the claims that CAN be checked mechanically.

use ramshield_forecasting::Hypothesis;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/ramshield-forecasting is two levels under the repo root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()))
}

fn readme() -> String {
    read("crates/ramshield-forecasting/README.md")
}

/// The paragraph of the README that contains `needle`: the matching line
/// plus the lines that follow it until a blank line.
fn paragraph_containing(text: &str, needle: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("the README has no paragraph mentioning {needle:?}"));
    let mut end = start;
    while end < lines.len() && !lines[end].trim().is_empty() {
        end += 1;
    }
    lines[start..end].join(" ")
}

/// The backticked names in a paragraph that look like crate names:
/// lowercase, no `::`.
fn crate_names(paragraph: &str) -> BTreeSet<String> {
    paragraph
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|s| {
            !s.is_empty()
                && !s.contains("::")
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        })
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn the_readme_names_no_method_that_does_not_exist() {
    let text = readme();
    assert!(
        !text.contains("Forecaster::tick()"),
        "the crate has no `tick()` method; the loop is `run()`"
    );
    assert!(
        !text.contains("get_batch_history"),
        "`get_batch_history` is not on `Store`; the forecaster reads the live traffic counters"
    );
}

#[test]
fn the_readme_lists_the_real_hypothesis_variants() {
    let text = readme();
    for variant in [
        Hypothesis::Normal,
        Hypothesis::VolumetricDoS,
        Hypothesis::SlowRampDoS,
        Hypothesis::FlashCrowd,
    ] {
        let name = format!("{variant:?}");
        assert!(
            text.contains(&name),
            "the README must name the `{name}` variant; the enum has it"
        );
    }
    // the old names are not variants and must not come back
    for stale in ["Volumetric,", "SlowRamp,"] {
        assert!(
            !text.contains(stale),
            "`{stale}` is not a variant of `Hypothesis`"
        );
    }
}

#[test]
fn the_readme_says_the_daemon_runs_the_crate() {
    let text = readme();
    assert!(
        text.contains("The daemon runs this crate"),
        "the README must say the daemon constructs a `Forecaster`"
    );
    assert!(
        !text.contains("does not run this crate"),
        "the README must not claim the daemon does not construct a `Forecaster`"
    );
}

#[test]
fn the_readme_dependency_list_is_the_crate_dependency_list() {
    let toml = read("crates/ramshield-forecasting/Cargo.toml");
    let deps_section = toml
        .split("[dependencies]")
        .nth(1)
        .expect("Cargo.toml has a [dependencies] section")
        .split("\n[")
        .next()
        .unwrap();
    let mut actual = BTreeSet::new();
    for line in deps_section.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            actual.insert(name.trim().to_string());
        }
    }
    assert!(
        !actual.is_empty(),
        "no dependencies were parsed from Cargo.toml"
    );

    let claimed = crate_names(&paragraph_containing(&readme(), "**Depends on:**"));
    assert_eq!(
        claimed, actual,
        "the README's dependency list does not match crates/ramshield-forecasting/Cargo.toml"
    );
}

#[test]
fn the_readme_read_by_list_is_the_actual_dependent_list() {
    let root = repo_root();
    let mut actual = BTreeSet::new();
    let mut manifests = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        for entry in entries.flatten() {
            let manifest = entry.path().join("Cargo.toml");
            if manifest.is_file() {
                manifests.push(manifest);
            }
        }
    }
    for manifest in manifests {
        let text = std::fs::read_to_string(&manifest).unwrap();
        if text.contains("ramshield-forecasting")
            && let Some(name) = text.lines().find_map(|l| {
                l.trim()
                    .strip_prefix("name")
                    .and_then(|r| r.split_once('=').map(|(_, v)| v))
            })
        {
            actual.insert(name.trim().trim_matches('"').to_string());
        }
    }
    let root_manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    if root_manifest.contains("ramshield-forecasting") {
        actual.insert("ramshield".to_string());
    }
    actual.remove("ramshield-forecasting");
    assert!(
        actual.contains("ramshield"),
        "the scan found no dependents at all"
    );

    let claimed = crate_names(&paragraph_containing(&readme(), "**Read by:**"));
    assert_eq!(
        claimed, actual,
        "the README's \"read by\" list does not match the crates that depend on ramshield-forecasting"
    );
}

#[test]
fn the_readme_unit_test_count_is_the_real_count() {
    let count = read("crates/ramshield-forecasting/src/tests/mod.rs")
        .matches("#[test]")
        .count()
        + read("crates/ramshield-forecasting/src/tests/mod.rs")
            .matches("#[tokio::test]")
            .count();
    assert!(count > 0, "no tests were counted");

    let readme = readme();
    let line = readme
        .lines()
        .find(|l| {
            l.split_whitespace()
                .next()
                .is_some_and(|w| w.trim_end_matches(':').parse::<usize>().is_ok())
                && l.contains("test")
        })
        .expect("the README states a test count");
    let claimed: usize = line
        .split_whitespace()
        .next()
        .and_then(|w| w.trim_end_matches(':').parse().ok())
        .unwrap_or_else(|| panic!("no test count in {line:?}"));
    assert_eq!(
        claimed, count,
        "the README says {claimed} unit tests; the crate has {count}"
    );
}

#[test]
fn the_readme_config_defaults_are_the_real_defaults() {
    let config = read("crates/ramshield-config/src/sections/forecasting.rs");
    let readme = readme();

    for (field, value) in [("seasonality_period", "3_600"), ("anomaly_zscore", "2.5")] {
        assert!(
            config
                .lines()
                .any(|l| l.trim_start().starts_with(&format!("{field}: {value}"))),
            "the config no longer defaults {field} to {value}"
        );
        let number: String = value
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let printed = if number.contains('.') {
            number.clone()
        } else {
            let digits: String = number.chars().collect();
            match digits.len() {
                4 => format!("{},{}", &digits[..1], &digits[1..]),
                _ => digits.clone(),
            }
        };
        assert!(
            readme.contains(&printed),
            "the README does not state the real default for {field} ({value})"
        );
    }
}

#[test]
fn the_readme_shows_the_real_holt_winters_fields() {
    let lib = read("crates/ramshield-forecasting/src/lib.rs");
    let mut source_fields = BTreeSet::new();
    let body = lib
        .split("pub struct HoltWinters {")
        .nth(1)
        .expect("the crate has a HoltWinters struct")
        .split('}')
        .next()
        .unwrap();
    for line in body.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("pub ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                source_fields.insert(name);
            }
        }
    }
    assert!(
        source_fields.len() >= 4,
        "the source fields were not parsed: {source_fields:?}"
    );

    let readme = readme();
    let block = readme
        .split("pub struct HoltWinters {")
        .nth(1)
        .expect("the README shows a HoltWinters struct")
        .split('}')
        .next()
        .unwrap();
    let mut shown = BTreeSet::new();
    for line in block.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("pub ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                shown.insert(name);
            }
        }
    }
    assert_eq!(
        shown, source_fields,
        "the README's HoltWinters fields do not match the crate's public fields"
    );
}

#[test]
fn the_readme_makes_none_of_the_old_false_claims() {
    let readme = readme();
    for false_claim in [
        "three independent models",
        "Not publicly exposed",
        "pre-computed likelihood tables",
        "requires agreement across multiple statistical methods",
        "default: 120 ticks",
        "events per 500ms window",
        "top-N event rates",
        "distribution of source IPs",
        "3 floats",
        "default 0.95",
        "every 10 seconds",
        "O(period)",
        "cusum_value",
        "warmup_samples",
    ] {
        assert!(
            !readme.contains(false_claim),
            "the README still makes the false claim {false_claim:?}"
        );
    }
}
