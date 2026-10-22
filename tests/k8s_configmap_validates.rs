//! The shipped Kubernetes ConfigMap must be a config the daemon accepts.
//!
//! F-33: `deploy/k8s/configmap.yaml` set `ipc.max_connections = 4096`
//! while the config validator refuses anything above 1,024, so the
//! server pod exited 1 at boot and the release gate did not notice - it
//! only checked the loopback addresses in that file.

use std::path::PathBuf;

/// The TOML block the ConfigMap carries under `data.config.toml`.
fn configmap_toml(path: &str) -> String {
    let full = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    let text = std::fs::read_to_string(&full)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", full.display()));
    let start = text
        .lines()
        .position(|l| l.trim() == "config.toml: |")
        .expect("the ConfigMap must carry a config.toml block");
    let indent = 4usize;
    let mut out = String::new();
    for line in text.lines().skip(start + 1) {
        if line.trim().is_empty() {
            out.push('\n');
            continue;
        }
        assert!(
            line.starts_with(&" ".repeat(indent)),
            "the config.toml block must be indented by {indent} spaces: {line}"
        );
        out.push_str(&line[indent..]);
        out.push('\n');
    }
    assert!(!out.is_empty(), "the config.toml block must not be empty");
    out
}

#[test]
fn the_shipped_configmaps_are_configs_the_daemon_accepts() {
    let config: ramshield_config::Config = toml::from_str(&configmap_toml(
        "deploy/k8s/configmap.yaml",
    ))
    .expect("the server ConfigMap must parse as a config");
    if let Err(err) = config.validate() {
        panic!(
            "deploy/k8s/configmap.yaml must pass Config::validate(), the daemon \
             refuses to start otherwise: {err}"
        );
    }
}
