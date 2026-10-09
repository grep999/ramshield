#!/usr/bin/env bash
set -euo pipefail

if [[ ${EUID} -ne 0 ]]; then
  echo "error: run this integration test as root (e.g. sudo $0)" >&2
  exit 77
fi
command -v unshare >/dev/null || { echo "error: unshare is required" >&2; exit 77; }
command -v nft >/dev/null || { echo "error: nft is required" >&2; exit 77; }

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="${repo_root}/tests/fixtures/synproxy.nft"
[[ -r "${fixture}" ]] || { echo "error: missing fixture: ${fixture}" >&2; exit 1; }

# A fresh network namespace confines all nftables state changes to this test.
unshare --net --mount-proc bash -s -- "${fixture}" <<'NAMESPACE'
set -euo pipefail
fixture="$1"
table="ramshield_synproxy_fixture"

nft -c -f "$fixture"
nft -f "$fixture"
nft list chain inet "$table" input >/dev/null

# Exercise the same delete-and-recreate batch used by the daemon when replacing
# an existing ruleset. Validation and application must both succeed atomically.
{
  printf 'delete table inet %s\n' "$table"
  cat "$fixture"
} | nft -c -f -
{
  printf 'delete table inet %s\n' "$table"
  cat "$fixture"
} | nft -f -
nft list chain inet "$table" input >/dev/null

# The duplicate add is syntactically valid but must fail at transaction apply.
# nftables must roll back the preceding delete and first add atomically.
if printf '%s\n' \
  "delete table inet $table" \
  "add table inet $table" \
  "add table inet $table" | nft -f -; then
  echo "error: intentionally invalid nft transaction unexpectedly succeeded" >&2
  exit 1
fi

# Check the original chain still exists after the failed transaction.
nft list chain inet "$table" input >/dev/null

# Remove only the test-owned table before the namespace exits.
nft delete table inet "$table"
echo "PASS: isolated SYNPROXY fixture loaded; failed nft transaction preserved prior table"
NAMESPACE
