//! F-43: the unit the installer writes must not be looser than the
//! tracked one.
//!
//! `scripts/install.sh` wrote a `ReadWritePaths=` line for three
//! `/proc/sys` tunables. systemd's `ReadWritePaths=` is additive, so the
//! unit the installer produced granted the daemon write access to
//! `/proc/sys` - access the daemon never uses (it only *reads* the
//! tunables, which `deploy/sysctl.d/99-ramshield-synproxy.conf` manages
//! at boot) and that the tracked unit `deploy/systemd/ramshield.service`
//! does not grant.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel} must be readable: {e}"))
}

/// The unit body the installer writes.
fn installer_unit() -> String {
    let script = read("scripts/install.sh");
    let start = script
        .lines()
        .position(|l| l.contains("<<UNIT"))
        .expect("install.sh must write the unit through a heredoc");
    let end = script
        .lines()
        .skip(start + 1)
        .position(|l| l.trim() == "UNIT")
        .expect("the unit heredoc must be closed");
    script
        .lines()
        .skip(start + 1)
        .take(end)
        .collect::<Vec<_>>()
        .join("\n")
}

fn write_paths(unit: &str) -> Vec<String> {
    unit.lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("ReadWritePaths="))
        .flat_map(|v| v.split_whitespace())
        .map(str::to_string)
        .collect()
}

#[test]
fn the_installed_unit_grants_no_write_access_to_proc_sys() {
    let paths = write_paths(&installer_unit());
    assert!(
        !paths.is_empty(),
        "the installer must still declare its ReadWritePaths"
    );
    for path in &paths {
        assert!(
            !path.starts_with("/proc/sys"),
            "the installer must not grant write access to {path}: the daemon only \
             reads the tunables and deploy/systemd/ramshield.service does not \
             grant it either"
        );
    }
}

#[test]
fn the_installed_unit_and_the_tracked_unit_agree_on_the_write_paths() {
    let installed = write_paths(&installer_unit());
    let tracked = write_paths(&read("deploy/systemd/ramshield.service"));
    for path in &installed {
        let in_tracked = tracked
            .iter()
            .any(|t| t == path || (path == "${STATE_DIR}" && t == "/var/lib/ramshield"));
        assert!(
            in_tracked,
            "{path} is writable in the installed unit but not in \
             deploy/systemd/ramshield.service"
        );
    }
}
