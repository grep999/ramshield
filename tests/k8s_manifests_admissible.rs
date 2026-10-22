//! F-15, F-34, I-027: the shipped Kubernetes manifests must be
//! admissible and must actually reach the daemon.
//!
//! - I-027 / F-15: the node-guard DaemonSet set
//!   `mountPropagation: Bidirectional` on `/sys/fs/bpf` without
//!   `privileged: true`. The API server refuses that combination, so the
//!   DaemonSet was never admitted and the XDP profile could not be
//!   deployed. The owner decided: drop the Bidirectional propagation
//!   rather than ask for a privileged container.
//! - F-34: the probes connected to the pod IP while the daemon binds
//!   the dashboard on 127.0.0.1, so the pod never passed its startup
//!   probe and the kubelet killed it.
//!
//! The manifests are checked here as text: the repo has no cluster to
//! admit them against, and `kubectl apply --dry-run=client` does not run
//! the server-side validation that rejected the DaemonSet.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel} must be readable: {e}"))
}

const MANIFESTS: [&str; 4] = [
    "deploy/k8s/daemonset.yaml",
    "deploy/k8s/deployment.yaml",
    "deploy/k8s/configmap.yaml",
    "deploy/k8s/configmap-node.yaml",
];

/// The manifest's own lines, without the comments.
fn manifest_lines(path: &str) -> Vec<String> {
    read(path)
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn no_manifest_asks_for_a_privileged_container() {
    for path in MANIFESTS {
        for line in manifest_lines(path) {
            assert!(
                !line.contains("privileged: true"),
                "{path} must not ask for a privileged container: {line}"
            );
        }
    }
}

#[test]
fn no_manifest_uses_bidirectional_mount_propagation() {
    for path in MANIFESTS {
        for line in manifest_lines(path) {
            assert!(
                !line.contains("mountPropagation: Bidirectional"),
                "{path}: Bidirectional mount propagation requires a privileged container, and \
                 the API server refuses the combination - the workload is never admitted: {line}"
            );
        }
    }
}

#[test]
fn every_http_probe_names_the_host_the_daemon_binds() {
    let mut probes = 0usize;
    for path in MANIFESTS {
        let lines = manifest_lines(path);
        for (n, line) in lines.iter().enumerate() {
            if !line.ends_with("httpGet:") {
                continue;
            }
            probes += 1;
            // The probe's fields follow it until the next field that is
            // not part of the probe.
            let block: Vec<&String> = lines[n + 1..]
                .iter()
                .take_while(|l| {
                    l.starts_with("host:")
                        || l.starts_with("path:")
                        || l.starts_with("port:")
                        || l.starts_with("scheme:")
                        || l.starts_with("initialDelaySeconds")
                        || l.starts_with("periodSeconds")
                        || l.starts_with("failureThreshold")
                        || l.starts_with("timeoutSeconds")
                        || l.starts_with("successThreshold")
                })
                .collect();
            assert!(
                block.iter().any(|l| l.starts_with("host: 127.0.0.1")),
                "{path}: an httpGet probe without `host: 127.0.0.1` connects to the pod IP, \
                 but the daemon binds the dashboard on 127.0.0.1"
            );
        }
    }
    assert!(probes > 0, "the manifests must still carry probes");
}

#[test]
fn the_manifests_bind_the_dashboard_on_loopback() {
    for path in [
        "deploy/k8s/configmap.yaml",
        "deploy/k8s/configmap-node.yaml",
    ] {
        let text = read(path);
        assert!(
            text.contains("http_addr = \"127.0.0.1:9999\""),
            "{path} must bind the dashboard on loopback, which is what the probes now address"
        );
    }
}
