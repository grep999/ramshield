#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

command -v cc >/dev/null 2>&1 || {
  echo 'error: a C compiler is required for the SHM ABI gate' >&2
  exit 1
}

TMP="$(mktemp -d /tmp/ramshield-shm-abi-XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

cc -std=c11 -Wall -Wextra -Werror -pthread -I. \
  scripts/shm_abi_stress.c -o "$TMP/shm-abi-stress"
"$TMP/shm-abi-stress"

# ThreadSanitizer is opt-in because some hosted/container kernels cannot
# reserve its shadow address space. Unsupported compiler/runtime is reported
# as SKIP; actual sanitizer findings remain fatal.
if [[ "${RAMSHIELD_SHM_TSAN:-0}" == "1" ]]; then
  if cc -std=c11 -Wall -Wextra -Werror -Wno-error=tsan -pthread -fsanitize=thread -g -O1 -I. \
      scripts/shm_abi_stress.c -o "$TMP/shm-abi-stress-tsan" 2>"$TMP/tsan-compile.log"; then
    set +e
    TSAN_OPTIONS=halt_on_error=1 "$TMP/shm-abi-stress-tsan" >"$TMP/tsan-run.log" 2>&1
    status=$?
    set -e
    if [[ "$status" -eq 0 ]]; then
      cat "$TMP/tsan-run.log"
      echo 'SHM ThreadSanitizer: PASS'
    elif grep -Eq 'FATAL: ThreadSanitizer: unexpected memory mapping|ThreadSanitizer: unsupported' "$TMP/tsan-run.log"; then
      echo 'SHM ThreadSanitizer: SKIP (runtime unsupported on this host)'
      cat "$TMP/tsan-run.log"
    else
      cat "$TMP/tsan-run.log" >&2
      exit "$status"
    fi
  else
    echo 'SHM ThreadSanitizer: SKIP (compiler/runtime unavailable)'
    cat "$TMP/tsan-compile.log"
  fi
fi
