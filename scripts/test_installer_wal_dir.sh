#!/usr/bin/env bash
# Regression test for the installers' WAL directory contract.
# The installed config must point to the persistent WAL directory, never /tmp.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
fail() { echo "FAIL: $*" >&2; exit 1; }

for installer in install.sh install-user.sh; do
  path="$root/scripts/$installer"
  [[ -f "$path" ]] || fail "missing installer: $installer"
  # The installer must substitute the baseline /tmp path with the persistent
  # state directory on fresh installs. Upgrades skip this block entirely,
  # preserving operator-managed configuration.
  if ! grep -q 'sed -i.*dir = \\"/tmp/ramshield-wal\\"' "$path"; then
    fail "$installer does not rewrite the WAL dir from /tmp/ramshield-wal"
  fi
  echo "PASS: $installer rewrites WAL dir to persistent path"
done

echo 'PASS: installer WAL directory contract holds'
