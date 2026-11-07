//! Part 3 gate A: documented QUICKSTART commands must be supported by the
//! shipped binary. Extracts every `ramshield ...` invocation from
//! QUICKSTART.md and checks its flags against the CLI parser, then runs the
//! one safe executable probe (`--validate`) end to end. Drift between the
//! docs and the binary fails CI instead of failing an operator.

use std::path::PathBuf;
#[cfg(feature = "full")]
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
#[cfg(feature = "full")]
fn quickstart_validate_invocation_passes_end_to_end() {
    let bin = env!("CARGO_BIN_EXE_ramshield");
    let root = env!("CARGO_MANIFEST_DIR");

    // The raw baseline ships the well-known dev key so the installer can
    // replace it; validating it directly is rejected by design (a local
    // process must not be able to run with the published key).
    let raw = Command::new(bin)
        .args(["--config", "config.baseline.toml", "--validate"])
        .current_dir(root)
        .output()
        .expect("failed to execute ramshield binary");
    assert!(
        !raw.status.success(),
        "the raw baseline carries the well-known dev key and must not validate"
    );

    // A config with a fresh key (what the installer produces) must validate.
    const DEV: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const FRESH: &str = "9f29c0d1e2b3a4958677a8b9c0d1e2f3a4958677a8b9c0d1e2f3a4958677a8b9";
    let baseline =
        std::fs::read_to_string(format!("{root}/config.baseline.toml")).expect("baseline readable");
    let scratch = baseline.replace(DEV, FRESH);
    assert!(
        scratch.contains(FRESH),
        "baseline must contain the dev key to replace"
    );

    let path =
        std::env::temp_dir().join(format!("ramshield-quickstart-{}.toml", std::process::id()));
    std::fs::write(&path, &scratch).expect("scratch config written");

    let out = Command::new(bin)
        .args(["--config", path.to_str().expect("utf-8 path"), "--validate"])
        .current_dir(root)
        .output()
        .expect("failed to execute ramshield binary");
    let _ = std::fs::remove_file(&path);

    assert!(
        out.status.success(),
        "a config with a fresh key must validate: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("config validation passed"),
        "--validate must print its verdict"
    );
}

#[test]
#[cfg(feature = "full")]
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
