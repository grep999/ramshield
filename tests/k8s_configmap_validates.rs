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

/// The ConfigMaps the shipped manifests load, and the workload that
/// loads each one.
const SHIPPED: [(&str, &str); 2] = [
    ("deploy/k8s/configmap.yaml", "the server Deployment"),
    ("deploy/k8s/configmap-node.yaml", "the node-guard DaemonSet"),
];

#[test]
fn the_shipped_configmaps_are_configs_the_daemon_accepts() {
    for (path, who) in SHIPPED {
        let config: ramshield_config::Config = toml::from_str(&configmap_toml(path))
            .unwrap_or_else(|e| panic!("{path} must parse as a config: {e}"));
        if let Err(err) = config.validate() {
            panic!(
                "{path} must pass Config::validate(), the daemon refuses to start otherwise: {err}"
            );
        }
        // The config must actually carry the settings the manifest
        // exists to set. A block that was gutted to comments parses to
        // `Config::default()` and validates, so without this the test
        // would pass on a file that says nothing.
        assert!(
            config.ipc.max_connections > 0,
            "{path} must set ipc.max_connections ({who} loads it)"
        );
        assert!(
            !config.ipc.tcp_addr.is_empty(),
            "{path} must set ipc.tcp_addr ({who} loads it)"
        );
        assert!(
            !config.dashboard.http_addr.is_empty(),
            "{path} must set the dashboard address ({who} loads it)"
        );
    }
}
