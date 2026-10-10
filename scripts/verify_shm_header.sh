#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

command -v cc >/dev/null 2>&1 || {
  echo 'error: a C compiler is required for the SHM ABI gate' >&2
  exit 1
}

TMP="$(mktemp /tmp/ramshield-shm-abi-XXXXXX.o)"
trap 'rm -f "$TMP"' EXIT

cat > "${TMP%.o}.c" <<'EOF'
#include "crates/ramshield-cgnat/include/ramshield_shm.h"
int main(void) {
    return sizeof(RamshieldShmRuleEntry) == 64 ? 0 : 1;
}
EOF
cc -std=c11 -Wall -Wextra -Werror -I. -c "${TMP%.o}.c" -o "$TMP"
rm -f "${TMP%.o}.c"
printf 'SHM C ABI gate: PASS\n'
