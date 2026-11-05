//! The metrics README must agree with the crate.
//!
//! An independent audit checked all 47 claims in the file against the
//! source: 16 true, 27 false, 4 unverifiable. The false ones were a
//! `Metrics` struct with six fields where the crate has 81, a
//! `BatchRecord` with seven fields where it has nine, a `DashboardSnapshot`
//! with thirteen fields where it has thirty, a `get_dashboard_snapshot()`
//! that does not exist, a `ramshield_events_total` series that is not
//! exported, a dependency on `ramshield-storage` that the crate does not
//! have, and a list of "written by" and "read by" crates that do not
//! exist.
//!
//! A README that cannot be checked drifts, so the claims that CAN be
//! checked are checked here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is the package dir: crates/ramshield-metrics.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/ramshield-metrics is two levels under the repo root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()))
}

fn readme() -> String {
    read("crates/ramshield-metrics/README.md")
}

/// The backticked names on the line that starts with `prefix`, e.g.
/// "**Depends on:** `serde`, `serde_json`".
fn listed_names(text: &str, prefix: &str) -> BTreeSet<String> {
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with(prefix))
        .unwrap_or_else(|| panic!("the README has no line starting with {prefix:?}"));
    let claim = line
        .trim_start()
        .strip_prefix(prefix)
        .unwrap_or_default()
        .split(". ")
        .next()
        .unwrap_or_default();
    claim
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|s| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .map(|s| s.to_string())
        .collect()
}

/// Every `.rs` file in the workspace, recursively.
fn workspace_rust_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let root = repo_root();
    let mut out = Vec::new();
    walk(&root.join("crates"), &mut out);
    walk(&root.join("src"), &mut out);
    out
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

/// The `module` argument of every `record_block_ip` call in the workspace.
fn block_modules_in_the_workspace() -> BTreeSet<String> {
    let mut modules = BTreeSet::new();
    for file in workspace_rust_files() {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for line in text.lines() {
            if let Some(idx) = line.find("record_block_ip(") {
                let rest = &line[idx + "record_block_ip(".len()..];
                // the module is the last argument and is always a literal
                if let Some(close) = rest.rfind(')') {
                    let args = &rest[..close];
                    if let Some(module) = args.rsplit(", ").next()
                        && let Some(m) = module.strip_prefix('"')
                        && let Some(m) = m.strip_suffix('"')
                    {
                        modules.insert(m.to_string());
                    }
                }
            }
        }
    }
    modules
}

#[test]
fn the_readme_dependency_list_is_the_crate_dependency_list() {
    let toml = read("crates/ramshield-metrics/Cargo.toml");
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

    let claimed = listed_names(&readme(), "**Depends on:**");
    assert_eq!(
        claimed, actual,
        "the README's dependency list does not match crates/ramshield-metrics/Cargo.toml"
    );
}

#[test]
fn the_readme_read_by_list_is_the_actual_dependent_list() {
    let root = repo_root();
    let root_manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let mut actual = BTreeSet::new();
    let mut manifests = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        for entry in entries.flatten() {
            let manifest = entry.path().join("Cargo.toml");
            // Only count crates that are workspace members: an untracked
            // directory sitting under crates/ is not part of the committed
            // workspace and must not change the README's read-by list.
            // ponytail: explicit member paths, add glob expansion if members
            // ever use wildcards.
            let member = format!("crates/{}", entry.file_name().to_string_lossy());
            if manifest.is_file() && root_manifest.contains(&format!("\"{member}\"")) {
                manifests.push(manifest);
            }
        }
    }
    for manifest in manifests {
        let text = std::fs::read_to_string(&manifest).unwrap();
        if text.contains("ramshield-metrics")
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
    if root_manifest.contains("ramshield-metrics") {
        actual.insert("ramshield".to_string());
    }
    actual.remove("ramshield-metrics");
    assert!(
        actual.contains("ramshield-detection"),
        "the scan found no dependents at all"
    );

    let claimed = listed_names(&readme(), "**Read by:**");
    assert_eq!(
        claimed, actual,
        "the README's \"read by\" list does not match the crates that depend on ramshield-metrics"
    );
}

#[test]
fn every_type_the_readme_shows_is_exported() {
    let readme = readme();
    let mut shown = BTreeSet::new();
    for block in readme.split("```rust").skip(1) {
        let block = block.split("```").next().unwrap();
        for line in block.lines() {
            let line = line.trim();
            for kw in ["pub struct ", "pub enum "] {
                if let Some(rest) = line.strip_prefix(kw) {
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        shown.insert(name);
                    }
                }
            }
        }
    }
    assert!(
        shown.len() >= 4,
        "the README shows almost no types: {shown:?}"
    );

    let sources: String = [
        "crates/ramshield-metrics/src/lib.rs",
        "crates/ramshield-metrics/src/types.rs",
        "crates/ramshield-metrics/src/history.rs",
        "crates/ramshield-metrics/src/counters.rs",
        "crates/ramshield-metrics/src/cache.rs",
        "crates/ramshield-metrics/src/prometheus.rs",
        "crates/ramshield-metrics/src/system.rs",
        "crates/ramshield-metrics/src/xdp_health.rs",
    ]
    .iter()
    .map(|f| read(f))
    .collect::<Vec<_>>()
    .join("\n");

    for name in shown {
        assert!(
            sources.contains(&format!("pub struct {name}"))
                || sources.contains(&format!("pub enum {name}")),
            "the README documents `{name}`, but the crate does not declare it public"
        );
    }
}

#[test]
fn the_readme_test_count_is_the_real_test_count() {
    let mut count = 0usize;
    for file in [
        "crates/ramshield-metrics/src/lib.rs",
        "crates/ramshield-metrics/src/tests/mod.rs",
        "crates/ramshield-metrics/src/tests/cache.rs",
    ] {
        count += read(file).matches("#[test]").count();
        count += read(file).matches("#[tokio::test]").count();
    }
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
        "the README says {claimed} tests; the crate has {count}"
    );
}

#[test]
fn every_prometheus_series_the_readme_names_is_exported() {
    let prometheus = read("crates/ramshield-metrics/src/prometheus.rs");
    let readme = readme();
    let mut named = BTreeSet::new();
    let mut rest = readme.as_str();
    while let Some(open) = rest.find('`') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('`') else { break };
        let token = &rest[..close];
        rest = &rest[close + 1..];
        if token.starts_with("ramshield_") {
            named.insert(token.to_string());
        }
    }
    assert!(
        named.len() >= 5,
        "the README names almost no Prometheus series: {named:?}"
    );
    for series in named {
        assert!(
            prometheus.contains(&format!("\"{series}\"")),
            "the README names the Prometheus series `{series}`, but the exporter does not emit it"
        );
    }
}

#[test]
fn the_readme_block_modules_are_the_ones_actually_passed() {
    let actual = block_modules_in_the_workspace();
    assert!(
        actual.contains("detection") && actual.contains("ipc"),
        "the workspace scan found no module arguments: {actual:?}"
    );
    let readme = readme();
    let line = paragraph_containing(&readme, "subsystem that issued the block:");
    let claimed: BTreeSet<String> = line
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|s| !s.is_empty() && !s.contains(' ') && *s != "module")
        .map(|s| s.to_string())
        .collect();
    let claimed: BTreeSet<String> = claimed
        .into_iter()
        .filter(|s| s.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        .collect();
    assert_eq!(
        claimed, actual,
        "the README's block module values do not match the modules passed to record_block_ip"
    );
}

#[test]
fn the_readme_reason_tokens_are_real_reason_tokens() {
    // the stable tokens
    let error = read("crates/ramshield-types/src/error.rs");
    let mut allowed: BTreeSet<String> = BTreeSet::new();
    let body = error
        .split("pub fn as_str")
        .nth(1)
        .expect("BlockReason::as_str exists")
        .split("pub fn from_reason_str")
        .next()
        .unwrap();
    for token in body.split('"').skip(1).step_by(2) {
        allowed.insert(token.to_string());
    }
    // plus the literals the workspace passes directly
    for file in workspace_rust_files() {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for line in text.lines() {
            if let Some(idx) = line.find("record_block_ip(") {
                let rest = &line[idx + "record_block_ip(".len()..];
                if let Some(close) = rest.rfind(')') {
                    let args = &rest[..close];
                    let parts: Vec<&str> = args.split(", ").collect();
                    if parts.len() >= 2
                        && let Some(tok) = parts[1].strip_prefix('"')
                        && let Some(tok) = tok.strip_suffix('"')
                    {
                        allowed.insert(tok.to_string());
                    }
                }
            }
        }
    }

    let readme = readme();
    let line = paragraph_containing(&readme, "`high_rps`");
    let claimed: BTreeSet<String> = line
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        .map(|s| s.to_string())
        .collect();
    assert!(!claimed.is_empty(), "no reason tokens were parsed");
    for token in claimed {
        assert!(
            allowed.contains(&token),
            "the README lists the reason token `{token}`, which nothing produces"
        );
    }
}

#[test]
fn the_readme_batch_history_size_is_the_real_history_size() {
    let lib = read("crates/ramshield-metrics/src/lib.rs");
    let history: usize = lib
        .split("const HISTORY: usize = ")
        .nth(1)
        .and_then(|s| s.split(';').next())
        .and_then(|s| s.trim().parse().ok())
        .expect("the crate has a HISTORY constant");
    let readme = readme();
    let line = readme
        .lines()
        .find(|l| l.contains("HISTORY"))
        .expect("the README states the history size");
    let digits: String = line.chars().filter(|c| c.is_ascii_digit()).collect();
    let claimed: usize = digits
        .parse()
        .unwrap_or_else(|_| panic!("no number in {line:?}"));
    assert_eq!(
        claimed, history,
        "the README says the batch history keeps {claimed} records; the crate keeps {history}"
    );
}
