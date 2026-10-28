//! The types README must agree with the crate.
//!
//! Three of the claims this file checks were wrong for a long time: the
//! dependency list named one crate instead of three, "read by every other
//! crate in the workspace" was false, and the type list did not match the
//! crate's exports. A README that cannot be checked drifts, so the
//! claims that CAN be checked are checked here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is the package dir: crates/ramshield-types.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/ramshield-types is two levels under the repo root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()))
}

/// The crate names in a backticked list on the line that starts with
/// `prefix`, e.g. "**Dependencies:** `thiserror`, `serde`".
fn listed_names(readme: &str, prefix: &str) -> BTreeSet<String> {
    let line = readme
        .lines()
        .find(|l| l.trim_start().starts_with(prefix))
        .unwrap_or_else(|| panic!("the README has no line starting with {prefix:?}"));
    // The claim is the list right after the label, up to the end of the
    // sentence. The rest of the line may mention other backticked names
    // that are not dependencies.
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

#[test]
fn the_readme_dependency_list_is_the_crate_dependency_list() {
    let toml = read("crates/ramshield-types/Cargo.toml");
    let deps_section = toml
        .split("[dependencies]")
        .nth(1)
        .expect("Cargo.toml has a [dependencies] section")
        .split(
            "
[",
        )
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

    let readme = read("crates/ramshield-types/README.md");
    let claimed = listed_names(&readme, "**Dependencies:**");
    assert_eq!(
        claimed, actual,
        "the README's dependency list does not match crates/ramshield-types/Cargo.toml"
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
        if text.contains("ramshield-types")
            && let Some(name) = text.lines().find_map(|l| {
                l.trim()
                    .strip_prefix("name")
                    .and_then(|r| r.split_once('=').map(|(_, v)| v))
            })
        {
            actual.insert(name.trim().trim_matches('"').to_string());
        }
    }
    // the root package too
    let root_manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    if root_manifest.contains("ramshield-types") {
        actual.insert("ramshield".to_string());
    }
    // the crate's own manifest names the crate too
    actual.remove("ramshield-types");
    assert!(
        actual.contains("ramshield-storage"),
        "the scan found no dependents at all"
    );

    let readme = read("crates/ramshield-types/README.md");
    let claimed = listed_names(&readme, "**Read by:**");
    assert_eq!(
        claimed, actual,
        "the README's \"read by\" list does not match the crates that depend on ramshield-types"
    );
}

#[test]
fn every_type_the_readme_shows_is_exported() {
    let lib = read("crates/ramshield-types/src/lib.rs");
    let readme = read("crates/ramshield-types/README.md");

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
        shown.len() >= 5,
        "the README shows almost no types: {shown:?}"
    );

    // A type is reachable if the crate re-exports it at the root, or if it
    // is declared public in one of the crate's public modules.
    let sources: String = [
        "crates/ramshield-types/src/command.rs",
        "crates/ramshield-types/src/error.rs",
        "crates/ramshield-types/src/events.rs",
        "crates/ramshield-types/src/ip_network.rs",
    ]
    .iter()
    .map(|f| read(f))
    .collect::<Vec<_>>()
    .join("\n");

    for name in shown {
        let reexported = lib.contains(&format!("{name},")) || lib.contains(&format!("{name} "));
        let declared = sources.contains(&format!("pub struct {name}"))
            || sources.contains(&format!("pub enum {name}"));
        assert!(
            reexported || declared,
            "the README documents `{name}`, but the crate neither re-exports it nor declares it public"
        );
    }
}

#[test]
fn the_readme_test_count_is_the_real_test_count() {
    let mut count = 0usize;
    for file in [
        "crates/ramshield-types/src/lib.rs",
        "crates/ramshield-types/src/command.rs",
        "crates/ramshield-types/src/error.rs",
        "crates/ramshield-types/src/events.rs",
        "crates/ramshield-types/src/ip_network.rs",
    ] {
        count += read(file).matches("#[test]").count();
        count += read(file).matches("#[tokio::test]").count();
    }
    assert!(count > 0, "no tests were counted");

    let readme = read("crates/ramshield-types/README.md");
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
