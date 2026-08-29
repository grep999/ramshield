//! Property-based fuzz harness for the config loader.
//!
//! Run: `cargo test -p ramshield-config --test fuzz`
//!
//! The TOML parser is exercised against arbitrary byte streams. A correct
//! loader must:
//! - never panic, never abort;
//! - return Err for invalid syntax (no crash);
//! - return Err for out-of-range values (validated by `Config::validate`);
//! - return Ok only for parseable, valid configs.

use proptest::prelude::*;
use std::io::Write;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// Arbitrary bytes → `Config::from_toml_file` (via a temp file).
    /// The parser must not panic.
    #[test]
    fn toml_loader_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..8192)) {
        let dir = std::env::temp_dir();
        let prefix: String = bytes.iter().take(8).map(|b| format!("{b:02x}")).collect();
        let path = dir.join(format!("ramshield-fuzz-{prefix}.toml"));
        let _ = std::fs::File::create(&path).and_then(|mut f| f.write_all(&bytes));
        let _ = ramshield_config::Config::from_toml_file(path.to_str().unwrap());
        let _ = std::fs::remove_file(&path);
    }

    /// Garbage strings that look almost-config must still fail gracefully.
    #[test]
    fn partial_toml_fails_cleanly(
        prefix in "[a-z_=0-9\\[\\]\\.\" ]{0,200}",
        suffix in "[a-z_=0-9\\[\\]\\.\" ]{0,200}",
    ) {
        let path = std::env::temp_dir().join("ramshield-fuzz-partial.toml");
        let _ = std::fs::write(&path, format!("{prefix}{suffix}"));
        let res = ramshield_config::Config::from_toml_file(path.to_str().unwrap());
        let _ = std::fs::remove_file(&path);
        let _: Result<ramshield_config::Config, anyhow::Error> = res;
    }
}

#[test]
fn validate_rejects_zero_ram() {
    let toml = r#"
        [engine]
        worker_threads = 0
        ram_limit_mb = 0
        shard_count = 256
    "#;
    let path = std::env::temp_dir().join("ramshield-fuzz-ram0.toml");
    std::fs::write(&path, toml).unwrap();
    let res = ramshield_config::Config::from_toml_file(path.to_str().unwrap());
    let _ = std::fs::remove_file(&path);
    assert!(res.is_err(), "ram_limit_mb=0 must be rejected");
}

#[test]
fn validate_rejects_non_power_of_two_shards() {
    let toml = r#"
        [engine]
        worker_threads = 0
        ram_limit_mb = 512
        shard_count = 100
    "#;
    let path = std::env::temp_dir().join("ramshield-fuzz-shard.toml");
    std::fs::write(&path, toml).unwrap();
    let res = ramshield_config::Config::from_toml_file(path.to_str().unwrap());
    let _ = std::fs::remove_file(&path);
    assert!(
        res.is_err(),
        "shard_count=100 (not power of 2) must be rejected"
    );
}
