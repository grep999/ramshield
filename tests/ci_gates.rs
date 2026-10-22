//! F-49, F-50: the CI gates that can never pass.
//!
//! F-50: three scheduled workflows gated their job on
//! `refs/heads/feature/ramshield-advanced`, a branch that does not exist
//! on the remote. A `schedule` event evaluates `github.ref` as the
//! default branch, so the jobs never ran - every tick of the `*/5`,
//! `*/30` and `*/15` crons was skipped, and the only signal was the
//! absence of activity. Un-gating them would have let a bot commit to
//! master, which the repo's agent contracts forbid, so they are deleted.
//!
//! F-49: the Backlog Gate's open-row parser must read the table
//! `BACKLOG.md` actually has.
//!
//! The workflow's awk looked for a `## P0` heading and rows starting
//! with `| A-nnn ` whose last cell was exactly `OPEN`. `BACKLOG.md` has
//! no such heading - the priority is a column - and its ids are `I-nnn`,
//! and an open row's status cell is prose that starts with `OPEN`. The
//! parser therefore matched nothing and the gate's own summary reported
//! zero open P0/P1 rows while `A-038` was open.
//!
//! This test runs the workflow's own awk against the committed
//! `BACKLOG.md` and compares the counts with an independent count in
//! Rust, so the gate cannot report a number the file does not support.

use std::path::PathBuf;
use std::process::Command;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel} must be readable: {e}"))
}

/// The awk program the workflow uses to count one priority.
fn workflow_awk(var: &str) -> String {
    let workflow = read(".github/workflows/backlog.yml");
    let marker = format!("{var}=$(awk -F'|' '");
    assert!(
        workflow.contains(&marker),
        "the workflow must count {var} with the pipe field separator"
    );
    let start = workflow
        .find(&marker)
        .unwrap_or_else(|| panic!("the workflow must count {var}"))
        + marker.len();
    let end = start
        + workflow[start..]
            .find('\'')
            .unwrap_or_else(|| panic!("the awk program for {var} must be closed"));
    workflow[start..end].to_string()
}

/// The open rows of one priority, counted in Rust.
fn open_rows(priority: &str) -> usize {
    read("BACKLOG.md")
        .lines()
        .filter_map(|line| {
            let mut cells = line.split('|').map(str::trim);
            let _first = cells.next()?;
            let id = cells.next()?;
            if !id.starts_with("I-") {
                return None;
            }
            let pri = cells.next()?;
            let status = line.rsplit('|').nth(1)?.trim();
            if pri == priority && status.starts_with("OPEN") {
                Some(id)
            } else {
                None
            }
        })
        .count()
}

fn awk_count(program: &str) -> usize {
    let out = Command::new("awk")
        .arg("-F|")
        .arg(program)
        .arg("BACKLOG.md")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("awk must run");
    assert!(
        out.status.success(),
        "the workflow's awk must run: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("the awk must print a count: {e}"))
}

#[test]
fn the_backlog_gate_counts_the_open_rows_the_file_has() {
    for priority in ["P0", "P1"] {
        let expected = open_rows(priority);
        let got = awk_count(&workflow_awk(&format!("OPEN_{priority}")));
        assert_eq!(
            got, expected,
            "the Backlog Gate's {priority} count must match the file: the workflow says \
             {got}, the file has {expected}"
        );
    }
}

#[test]
fn the_open_rows_are_not_all_zero() {
    // A parser that matches nothing reports zero for every priority, and
    // that is exactly the failure this test guards. The backlog has open
    // P1 rows, so the count must be non-zero.
    assert!(
        open_rows("P1") > 0,
        "BACKLOG.md must have open P1 rows for this guard to mean anything"
    );
    assert_eq!(awk_count(&workflow_awk("OPEN_P1")), open_rows("P1"));
}

#[test]
fn no_scheduled_workflow_is_gated_on_a_branch_that_cannot_exist() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("the workflows directory must exist") {
        let path = entry.expect("a workflow entry must be readable").path();
        if path.extension().and_then(|e| e.to_str()) != Some("yml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("a workflow must be readable");
        if !text.contains("schedule:") {
            continue;
        }
        checked += 1;
        for line in text.lines() {
            let line = line.trim();
            if !line.starts_with("if:") {
                continue;
            }
            assert!(
                !line.contains("github.ref == 'refs/heads/feature/"),
                "{} gates a scheduled job on a feature branch: a schedule event \
                 evaluates github.ref as the default branch, so the job never runs: {line}",
                path.display()
            );
        }
    }
    assert!(
        checked > 0,
        "the repository must still have scheduled workflows"
    );
}

#[test]
fn no_jobs_last_step_masks_its_own_failure() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    for entry in std::fs::read_dir(&dir).expect("the workflows directory must exist") {
        let path = entry.expect("a workflow entry must be readable").path();
        if path.extension().and_then(|e| e.to_str()) != Some("yml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("a workflow must be readable");
        // The run steps of each job, in order. A step's exit code is the
        // job's only when it is the last one, so a last step that masks
        // its own failure makes the job green whatever happened.
        let mut job = String::new();
        let mut steps: Vec<(usize, String)> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            if line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':')
            {
                job = line.trim().trim_end_matches(':').to_string();
                steps.clear();
                continue;
            }
            if let Some(step) = line.trim().strip_prefix("- run:") {
                steps.push((n + 1, step.trim().to_string()));
            }
        }
        if let Some((n, step)) = steps.last() {
            assert!(
                !step.ends_with("|| true"),
                "{}:{n} - the last step of the job `{job}` masks its own failure, so \
                 the job is green whatever happened: {step}",
                path.display()
            );
        }
    }
}
