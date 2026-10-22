//! F-27: `scripts/prod_smoke.sh` must do what it says and must not
//! delete a directory it did not create.
//!
//! The script's header advertised a coverage it did not have: it
//! queued an unblock without checking the IP was gone, never sent a CIDR
//! block, and never waited for a TTL to expire. It also ran
//! `rm -rf "$WAL_DIR"` on a path that came from the environment, and its
//! default config (`config.prod.toml`) was not in the tree, so the
//! script could not run at all.
//!
//! The script has no test harness, so the guard reads it and asserts
//! that the advertised scenarios are performed and that the delete is
//! guarded. The scenarios themselves were run end to end: with the
//! fixes, the script boots the daemon, blocks, unblocks and verifies,
//! blocks a CIDR and verifies, waits a short TTL out, kills the daemon
//! and sees the WAL restore.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel} must be readable: {e}"))
}

const SCRIPT: &str = "scripts/prod_smoke.sh";

#[test]
fn the_smoke_script_does_what_its_header_advertises() {
    let script = read(SCRIPT);
    // Unblock -> verify absent.
    assert!(
        script.contains("\"blocked\":false"),
        "the unblock must be verified: the IP's absence must be asserted"
    );
    // CIDR block -> verify -> unblock -> verify.
    assert!(
        script.contains("block_cidr"),
        "the header advertises a CIDR block: the script must send one"
    );
    assert!(
        script.contains("unblock_cidr"),
        "the CIDR block must be released again"
    );
    // A short TTL -> verify it expired.
    assert!(
        script.contains("\"ttl_secs\":2"),
        "a short TTL must be set so the expiry can be observed"
    );
}

#[test]
fn the_smoke_script_cannot_delete_a_directory_it_did_not_create() {
    let script = read(SCRIPT);
    assert!(
        script.contains("ramshield-prod-smoke-wal*"),
        "the delete must be guarded by the smoke test's own name"
    );
    assert!(
        script.contains("mktemp -d"),
        "the WAL directory must be a fresh temporary one by default"
    );
}

#[test]
fn the_smoke_scripts_default_config_exists() {
    let script = read(SCRIPT);
    let default = script
        .lines()
        .find_map(|l| l.trim().strip_prefix("CFG=\"${CFG:-"))
        .and_then(|l| l.split("}").next())
        .unwrap_or_default();
    assert!(!default.is_empty(), "the script must have a default config");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(default);
    assert!(
        path.exists(),
        "the default config {default} must exist in the tree"
    );
}
