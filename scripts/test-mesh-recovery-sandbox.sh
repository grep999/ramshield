#!/usr/bin/env bash
# Create a disposable checkout of the committed source and run the smallest
# Rust test set that exercises mesh transport, WAL recovery integration, and
# the public TCP acknowledgement path.
set -Eeuo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/test-mesh-recovery-sandbox.sh [--keep]

Creates a fresh temporary source tree from the current Git commit, then runs:
  1. the captured-boundary full-sync regression test;
  2. mesh TCP acceptance tests;
  3. enforcement tests with the mesh feature enabled.

Requirements: git, tar, cargo, and the Rust toolchain selected by rust-toolchain.toml.
No Docker, network namespace, or production configuration is used.
Set CARGO_NET_OFFLINE=true to force Cargo offline mode.
Pass --keep (or set RAMSHIELD_SANDBOX_KEEP=1) to preserve the sandbox for inspection.
USAGE
}

keep_sandbox="${RAMSHIELD_SANDBOX_KEEP:-0}"
case "${1:-}" in
  "") ;;
  --keep) keep_sandbox=1 ;;
  -h|--help) usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac

for command in git tar cargo; do
  command -v "$command" >/dev/null 2>&1 || {
    printf 'ERROR: required command not found: %s\n' "$command" >&2
    exit 127
  }
done

repo_root="$(git rev-parse --show-toplevel)"
source_ref="$(git -C "$repo_root" rev-parse --verify HEAD)"
sandbox="$(mktemp -d "${TMPDIR:-/tmp}/ramshield-mesh-recovery.XXXXXX")"

cleanup() {
  if [[ "$keep_sandbox" == "1" ]]; then
    printf 'Sandbox preserved at: %s\n' "$sandbox"
  else
    rm -rf -- "$sandbox"
  fi
}
trap cleanup EXIT

printf 'Source commit: %s\n' "$source_ref"
printf 'Creating disposable sandbox...\n'
git -C "$repo_root" archive --format=tar "$source_ref" | tar -xf - -C "$sandbox"
cd "$sandbox"

if [[ ! -f Cargo.lock ]]; then
  printf 'ERROR: Cargo.lock is missing; refusing a non-reproducible sandbox run.\n' >&2
  exit 2
fi

printf '\n[1/3] Full-sync captured-boundary regression\n'
cargo test --locked -p ramshield-mesh full_sync_recovery_waits_for_receiver_completion_after_frame_acks

printf '\n[2/3] Real TCP mesh transport acceptance tests\n'
cargo test --locked -p ramshield-mesh --test mesh_acceptance

printf '\n[3/3] Enforcement with mesh integration enabled\n'
cargo test --locked -p ramshield-enforcement --features mesh

printf '\nPASS: all minimal mesh-recovery sandbox gates passed.\n'
