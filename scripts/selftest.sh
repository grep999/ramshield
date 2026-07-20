#!/usr/bin/env bash
set -euo pipefail

BIN="${CARGO_TARGET_DIR:-target}/release/ramshield"
[[ -z "${CARGO_TARGET_DIR:-}" ]] && BIN="target/release/ramshield"

PASS=0 FAIL=0

check() {
    local desc="$1" rc="$2"
    if [[ "$rc" -eq 0 ]]; then
        echo "  PASS  $desc"
        ((PASS++))
    else
        echo "  FAIL  $desc"
        ((FAIL++))
    fi
}

echo "=== RamShield Self-Test ==="
echo ""

# 1. Binary exists
[[ -x "$BIN" ]]; rc=$?
check "binary exists ($BIN)" "$rc"

if [[ "$rc" -ne 0 ]]; then
    echo ""
    echo "Summary: ${PASS} passed, ${FAIL} failed"
    exit 1
fi

# 2. --version
"$BIN" --version >/dev/null 2>&1; check "--version flag" $?

# 3. --help
"$BIN" --help >/dev/null 2>&1; check "--help output" $?

echo ""
echo "Summary: ${PASS} passed, ${FAIL} failed"
exit $FAIL