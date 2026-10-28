//! Part 3 gate A: documented QUICKSTART commands must be supported by the
//! shipped binary. Extracts every `ramshield ...` invocation from
//! QUICKSTART.md and checks its flags against the CLI parser, then runs the
//! one safe executable probe (`--validate`) end to end. Drift between the
//! docs and the binary fails CI instead of failing an operator.

use std::path::PathBuf;
use std::process::Command;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel} must be readable: {e}"))
}

/// Flags accepted by `parse_args` in src/main.rs. Keep in sync — the test
/// below fails if QUICKSTART documents a flag this set does not cover.
const SUPPORTED: &[&str] = &[
    "--config",
    "-c",
    "--no-xdp",
    "--validate",
    "--version",
    "-V",
    "doctor",
];

#[test]
fn quickstart_ramshield_invocations_use_supported_flags() {
    let doc = read("QUICKSTART.md");
    let mut checked = 0usize;
    for line in doc.lines() {
        let line = line.trim_start();
        // Only bare `ramshield` invocations (not ramshield-cli, not ./target/...).
        let Some(rest) = line.strip_prefix("ramshield ") else {
            continue;
        };
        for token in rest.split_whitespace() {
            if token.starts_with('-') || token == "doctor" {
                assert!(
                    SUPPORTED.contains(&token),
                    "QUICKSTART documents flag `{token}` but the CLI does not accept it"
                );
                checked += 1;
            }
        }
    }
    assert!(
        checked > 0,
        "QUICKSTART must document at least one ramshield invocation"
    );
}

#[test]
fn quickstart_validate_invocation_passes_end_to_end() {
    let bin = env!("CARGO_BIN_EXE_ramshield");
    let out = Command::new(bin)
        .args(["--config", "config.baseline.toml", "--validate"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to execute ramshield binary");
    assert!(
        out.status.success(),
        "documented --validate invocation must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("config validation passed"),
        "--validate must print its verdict"
    );
}

#[test]
fn unknown_flags_fail_fast_instead_of_booting() {
    let bin = env!("CARGO_BIN_EXE_ramshield");
    let out = Command::new(bin)
        .arg("--definitely-not-a-flag")
        .output()
        .expect("failed to execute ramshield binary");
    assert!(!out.status.success(), "unknown flags must be fatal");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("unknown argument"),
        "the failure must say WHY (unknown argument), not silently boot"
    );
}
