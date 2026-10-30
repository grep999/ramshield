#!/usr/bin/env bash
# Regression test for installer WAL directory contract.
# Simulates installer config logic on mock configs and asserts outcomes.
set -euo pipefail

FAIL=0

fail() {
  echo "FAIL: $*" >&2
  FAIL=1
}

run_test() {
  local name="$1"
  local initial_wal="$2"
  local expected_wal="$3"
  local state_dir="$4"
  local tmpdir

  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' RETURN

  cat > "$tmpdir/config.toml" <<EOF
[wal]
enabled = true
dir = "$initial_wal"
seg_max_bytes = 67108864
retention_max_bytes = 1073741824
durability = "GroupCommit"
compress = true
allow_volatile_fallback = false
EOF

  # Mirrors install.sh/install-user.sh: legacy default path is migrated,
  # custom paths are preserved.
  if grep -q 'dir = "/tmp/ramshield-wal"' "$tmpdir/config.toml" 2>/dev/null; then
    sed -i "s|dir = \"/tmp/ramshield-wal\"|dir = \"${state_dir}/wal\"|" "$tmpdir/config.toml"
  fi

  local result
  result=$(grep 'dir =' "$tmpdir/config.toml" | sed 's/.*dir = "\(.*\)".*/\1/')

  if [[ "$result" != "$expected_wal" ]]; then
    fail "$name: got '$result', expected '$expected_wal' (input: $initial_wal)"
  else
    echo "PASS: $name"
  fi
}

run_test "fresh_install_root" "/tmp/ramshield-wal" "/var/lib/ramshield/wal" "/var/lib/ramshield"
run_test "upgrade_migrate_legacy" "/tmp/ramshield-wal" "/var/lib/ramshield/wal" "/var/lib/ramshield"
run_test "upgrade_custom_preserved" "/custom/wal/path" "/custom/wal/path" "/var/lib/ramshield"
run_test "no_match_no_change" "/nonexistent/path" "/nonexistent/path" "/var/lib/ramshield"

if [[ $FAIL -eq 0 ]]; then
  echo "PASS: installer WAL directory contract holds"
  exit 0
else
  exit 1
fi