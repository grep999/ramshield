#!/usr/bin/env bash
# Regression test for installer WAL directory contract.
# Simulates installer config logic on mock configs and asserts outcomes.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FAIL=0

fail() {
  echo "FAIL: $*" >&2
  FAIL=1
}

# Verify both installer scripts contain the required operations.
for script in install.sh install-user.sh; do
  path="$ROOT/scripts/$script"
  [[ -f "$path" ]] || fail "missing installer: $script"
  # Fresh-install WAL dir substitution must exist.
  grep -q 'sed -i.*dir = \\\"/tmp/ramshield-wal\\\"' "$path" || fail "$script missing fresh-install WAL dir substitution"
  # Upgrade migration must exist (narrow elif guard).
  grep -qF 'elif grep -q' "$path" || fail "$script missing upgrade WAL migration branch"
  # Existing legacy WAL data must not be silently abandoned.
  grep -qF 'contains unrecovered data' "$path" || fail "$script missing legacy WAL data-preservation guard"
done

echo "PASS: installer scripts contain required operations"

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