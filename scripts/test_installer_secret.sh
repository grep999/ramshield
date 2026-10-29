#!/usr/bin/env bash
# Regression test for the installers' secret-handling contract.
#
# The generated IPC authentication key is a secret. Neither installer may
# print it, embed it in a log message, or hand it to any command whose
# output is captured into the install log. The key must exist only in the
# written configuration file.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
fail() { echo "FAIL: $*" >&2; exit 1; }

for installer in install.sh install-user.sh; do
  path="$root/scripts/$installer"
  [[ -f "$path" ]] || fail "missing installer: $installer"
  # The key variable must only ever be consumed by: the format check, the
  # sed substitution into config.toml. Any other use (echo/log/printf
  # with the variable) is a disclosure.
  if grep -nE '\$(\{)?IPC_KEY' "$path" | grep -vE '(od -An|IPC_KEY=|\"\$IPC_KEY\" =~|k1:\$IPC_KEY/)'; then
    fail "$installer uses IPC_KEY outside the generation/check/substitution path"
  fi
  # Belt and braces: no log line may interpolate the key.
  if grep -E 'log .*IPC_KEY' "$path" >/dev/null; then
    fail "$installer logs the IPC key"
  fi
  echo "PASS: $installer never prints the IPC key"
done

echo 'PASS: installer secret-handling contract holds'
