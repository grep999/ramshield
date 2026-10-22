//! F-35: `ramshield-cli` must be the CLI the release ships.
//!
//! The binary was a five-line stub that printed "RamShield CLI" and
//! exited 0, but the release tarball ships it, `scripts/install.sh` fails
//! the install without it, and the README and `docs/OPERATIONS.md`
//! document its subcommands. An operator running
//! `ramshield-cli block 1.2.3.4 --reason manual --ttl 300` was told
//! nothing and nothing happened.
//!
//! The implementation was restored from `59d4bc6` (the commit before it
//! was deleted) and made to work against today's API. The restored file
//! carries its own unit tests for the response parser and the HMAC key
//! rules. This test pins that the shipped source is a CLI with the
//! documented subcommands, and not a stub.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel} must be readable: {e}"))
}

#[test]
fn the_cli_is_a_cli_and_not_a_stub() {
    let cli = read("src/cli.rs");
    assert!(
        !cli.contains("println!(\"RamShield CLI\")"),
        "the CLI must not be the stub that printed a greeting and exited"
    );
    for command in [
        "Check {",
        "Block {",
        "Unblock {",
        "UnblockCidr {",
        "Stats",
        "Status {",
        "Info {",
    ] {
        assert!(
            cli.contains(command),
            "the CLI must implement the documented subcommand {command}"
        );
    }
    assert!(
        cli.contains("enum Cmd"),
        "the CLI must define its subcommands"
    );
}

#[test]
fn the_documented_cli_commands_exist_in_the_operations_doc() {
    let doc = read("docs/OPERATIONS.md");
    for command in ["status", "stats", "check", "info", "block", "unblock"] {
        assert!(
            doc.contains(command),
            "the operations doc documents {command}; the CLI must implement it"
        );
    }
}
