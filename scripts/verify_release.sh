#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
STATIC=0; [[ "${1:-}" == --static ]] && STATIC=1
PASS=0; FAIL=0
ok(){ PASS=$((PASS+1)); printf 'PASS %s\n' "$*"; }
bad(){ FAIL=$((FAIL+1)); printf 'FAIL %s\n' "$*"; }
V="$(scripts/release_version.sh)"; [[ -n "$V" ]] && ok "version $V" || bad version
grep -q "$V" deploy/k8s/deployment.yaml && ok 'Kubernetes deployment version' || bad 'Kubernetes deployment version'
grep -q "$V" deploy/k8s/daemonset.yaml && ok 'Kubernetes daemonset version' || bad 'Kubernetes daemonset version'
grep -q 'RAMSHIELD_VERSION' docker/Dockerfile Containerfile scripts/build_docker.sh && ok 'container builds consume authoritative version' || bad 'container version plumbing'
if grep -RInE 'ghcr.io/grep999/ramshield:(latest|0\.2|0\.3\.[0-3])([[:space:]"`]|$)' deploy docker Containerfile scripts --exclude='*.md' >/dev/null 2>&1; then bad 'stale image tag'; else ok 'active image tags pinned'; fi
grep -q 'verify-blob' scripts/install.sh && grep -q 'COSIGN_BIN' scripts/install.sh && ok 'installer verifies signatures' || bad 'installer signature gate'
grep -q 'cargo audit --no-yanked' .github/workflows/dependency_audit.yml && ok 'dependency audit hard gate' || bad 'dependency audit gate'
grep -q 'cargo deny check' .github/workflows/release.yml && ok 'cargo-deny release gate' || bad 'cargo-deny release gate'
grep -q 'actions/attest-build-provenance' .github/workflows/release.yml && ok 'provenance attestation' || bad 'provenance'
grep -q 'SPDX-2.3' scripts/generate_sbom.py && ok 'SPDX SBOM generator' || bad 'SBOM generator'
grep -q 'ramshield-${VERSION}-${TARGET}.tar.gz' scripts/install.sh && ok 'installer artifact naming' || bad 'installer artifact naming'
grep -q 'config.baseline.toml' .github/workflows/release.yml && ok 'release config asset' || bad 'release config asset'
grep -q 'ReadWritePaths=.*sys/fs/bpf' scripts/install.sh deploy/systemd/ramshield.service && ok 'XDP filesystem access' || bad 'XDP filesystem access'
grep -q 'RAMSHIELD_IPC_KEY' scripts/upgrade_qualification.sh && ok 'qualification IPC auth' || bad 'qualification IPC auth'
grep -Rq 'mark_xdp_projection_stale' crates/ramshield-metrics/src/ && grep -Rq 'mark_xdp_projection_stale' crates/ramshield-enforcement/src/ && ok 'XDP mutation failures mark projection stale' || bad 'XDP stale failure path'
grep -q 'SHM_PROBE_LIMIT' crates/ramshield-cgnat/src/shm.rs && grep -q 'RAMSHIELD_SHM_PROBE_LIMIT' crates/ramshield-cgnat/include/ramshield_shm.h && ok 'SHM Rust/C probe contract' || bad 'SHM probe contract'
grep -q 'mode(0o600)' crates/ramwal/src/segment.rs crates/ramwal/src/wal.rs crates/ramshield-cgnat/src/shm.rs && ok 'owner-only state files' || bad 'owner-only state files'
grep -q '^## \[0.4.0\]' CHANGELOG.md && grep -q '^## \[0.3.4\]' CHANGELOG.md && ok 'release history boundaries' || bad 'release history boundaries'
[[ ! -f src/entry.rs ]] && ok 'obsolete duplicate SHM ABI removed' || bad 'obsolete duplicate SHM ABI remains'
grep -q 'gcc-aarch64-linux-gnu' .github/workflows/release.yml && ok 'aarch64 linker setup' || bad 'aarch64 linker setup'
grep -q '127.0.0.1:7890' deploy/k8s/configmap.yaml && ok 'Kubernetes IPC loopback' || bad 'Kubernetes IPC loopback'
grep -q '127.0.0.1:9999' deploy/k8s/configmap.yaml && ok 'Kubernetes dashboard loopback' || bad 'Kubernetes dashboard loopback'
[[ ! -f deploy/k8s/service.yaml ]] && ok 'no insecure ClusterIP control-plane service' || bad 'insecure ClusterIP control-plane service'
python3 -c 'import pathlib,tomllib; [tomllib.loads(p.read_text()) for p in pathlib.Path(".").rglob("*.toml")]' && ok 'TOML' || bad 'TOML'
python3 -c 'import pathlib,yaml; [list(yaml.safe_load_all(p.read_text())) for p in pathlib.Path("deploy/k8s").glob("*.yaml")]' && ok 'Kubernetes YAML' || bad 'Kubernetes YAML'
bash -n scripts/*.sh && ok 'shell syntax' || bad 'shell syntax'
python3 -m compileall -q scripts && ok 'Python syntax' || bad 'Python syntax'
scripts/verify_shm_header.sh && ok 'SHM C ABI' || bad 'SHM C ABI'
if (( STATIC == 0 )); then
  cargo fmt --all -- --check && ok 'fmt' || bad 'fmt'
  cargo check --workspace --locked --all-targets --features full && ok 'check' || bad 'check'
  cargo clippy --workspace --locked --all-targets --features full -- -D warnings && ok 'clippy' || bad 'clippy'
  cargo test --workspace --locked --features full && ok 'tests' || bad 'tests'
  cargo audit --no-yanked && ok 'audit' || bad 'audit'
  cargo deny check && ok 'cargo-deny' || bad 'cargo-deny'
fi
printf 'release gate: %d pass, %d fail\n' "$PASS" "$FAIL"
(( FAIL == 0 ))
