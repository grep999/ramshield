//! F-42: the release gate must check the dependency-audit flags the
//! workflow actually runs.
//!
//! `scripts/verify_release.sh` grepped for `cargo audit --locked`, but
//! `.github/workflows/dependency_audit.yml` runs `cargo audit --no-yanked`
//! (the deliberate policy: `deny.toml` sets `yanked = "deny"`). The
//! grep therefore never matched and the release gate reported a
//! dependency-audit failure on a healthy tree.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel} must be readable: {e}"))
}

#[test]
fn the_release_gate_audits_the_flags_the_workflow_runs() {
    let script = read("scripts/verify_release.sh");
    let workflow = read(".github/workflows/dependency_audit.yml");

    let flags = script
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("cargo audit "))
        .and_then(|rest| rest.split_whitespace().next())
        .expect("the release gate must run cargo audit with flags");
    assert!(
        workflow.contains(&format!("cargo audit {flags}")),
        "the workflow must run the flags the release gate audits: cargo audit {flags}"
    );
    assert!(
        script.contains(&format!("grep -q 'cargo audit {flags}'")),
        "the release gate must check the flags the workflow runs: cargo audit {flags}"
    );
}
