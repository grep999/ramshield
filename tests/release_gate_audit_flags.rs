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

    // The flags the gate audits: the whole tail of its `cargo audit`
    // invocation, not the first token. A gate that audited
    // `--no-yanked` while the workflow ran `--no-yanked --deny warnings`
    // would be checking something the workflow does not do.
    let invocation = script
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("cargo audit "))
        .expect("the release gate must run cargo audit with flags");
    let flags = invocation["cargo audit ".len()..]
        .split("&&")
        .next()
        .expect("split always yields one element")
        .trim();
    assert!(
        !flags.is_empty(),
        "the release gate must audit cargo audit with flags"
    );

    // The workflow must run exactly that invocation.
    let workflow_run = workflow
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("run: "))
        .filter(|l| l.starts_with("cargo audit"))
        .expect("the dependency-audit workflow must run cargo audit");
    assert_eq!(
        workflow_run,
        format!("cargo audit {flags}"),
        "the workflow must run exactly the invocation the release gate audits"
    );

    // The gate's grep must name the file it greps. A gate that pointed
    // the grep at a file without a `cargo audit` line would pass this
    // test while failing at runtime.
    let grep_line = script
        .lines()
        .find(|l| l.contains("grep") && l.contains("cargo audit"))
        .expect("the release gate must grep for the cargo audit invocation");
    let target = grep_line
        .split("&&")
        .next()
        .expect("split always yields one element")
        .trim()
        .split_whitespace()
        .last()
        .expect("the grep line must name a file");
    assert_eq!(
        target, ".github/workflows/dependency_audit.yml",
        "the release gate must grep the dependency-audit workflow"
    );
    assert!(
        grep_line.contains(&format!("cargo audit {flags}")),
        "the release gate must check the flags the workflow runs: cargo audit {flags}"
    );
    assert!(
        read(target).contains(&format!("cargo audit {flags}")),
        "the file the release gate greps must contain the invocation it checks"
    );
}
