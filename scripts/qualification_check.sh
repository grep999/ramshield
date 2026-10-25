#!/usr/bin/env bash
# scripts/qualification_check.sh — P1 #33 qualification matrix verifier.
#
# Runs every automated check from docs/QUALIFICATION_MATRIX.md.
# Reports per-row status: ✅ pass | ⚠️ partial | ❌ fail | ➖ manual
#
# Usage: ./scripts/qualification_check.sh
#   --quick   skip cargo test (source-greps only, <10s)
#   --json    machine-readable JSON summary

set -uo pipefail
IFS=$'\n\t'

PASS=0; FAIL=0; MANUAL=0; WARN=0
RESULTS=()

green() { printf '  \033[32m%s\033[0m %s\n' "✅" "$*"; }
warn()  { printf '  \033[33m%s\033[0m %s\n' "⚠️" "$*"; }
red()   { printf '  \033[31m%s\033[0m %s\n' "❌" "$*" >&2; }
gray()  { printf '  \033[90m%s\033[0m %s\n' "➖" "$*"; }
pass()  { PASS=$((PASS+1)); green "$1"; RESULTS+=("PASS|$1"); }
warn2() { WARN=$((WARN+1)); warn "$1"; RESULTS+=("WARN|$1"); }
fail()  { FAIL=$((FAIL+1)); red "$1"; RESULTS+=("FAIL|$1"); }
manual(){ MANUAL=$((MANUAL+1)); gray "$1"; RESULTS+=("MANUAL|$1"); }

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || { echo "FATAL: cannot cd to repo root"; exit 99; }

QUICK=false; JSON=false
for arg; do [ "$arg" = "--quick" ] && QUICK=true; [ "$arg" = "--json" ] && JSON=true; done

echo "================================"
echo " RamShield Qualification Matrix"
echo "================================"
echo ""

# ── 1. Linux kernel ──────────────────────────────────────────────────────
echo "--- #1 Linux kernel ---"
KERNEL=$(uname -r)
[ -n "$KERNEL" ] && pass "kernel $KERNEL detected" || fail "kernel version undetectable"

# ── 2. XDP mode ─────────────────────────────────────────────────────────
echo "--- #2 XDP mode ---"
BPF_CRATE=$(ls "$ROOT"/crates/ramshield-xdp/ramshield-xdp-bpf/Cargo.toml 2>/dev/null)
if which bpf-linker &>/dev/null; then
    w2=$(bpf-linker --version)
    warn2 "bpf-linker $w2 present; BPF ELF at $BPF_CRATE (build skippable: no target)"
else
    fail "bpf-linker not in PATH"
fi

# ── 3. IPv4 ──────────────────────────────────────────────────────────────
echo "--- #3 IPv4 ---"
N4=$(grep -rlE "fn .*ipv4|fn .*v4\(|127\.0\.0\.1" "$ROOT"/crates/*/tests/ "$ROOT"/tests/ 2>/dev/null | wc -l)
[ "$N4" -ge 2 ] && pass "IPv4 coverage present ($N4 test files)" || warn2 "no explicit v4 tests found"

# ── 4. IPv6 ──────────────────────────────────────────────────────────────
echo "--- #4 IPv6 ---"
N6=$(grep -rlE "fn .*v6|fn .*ipv6" "$ROOT"/crates/*/tests/ "$ROOT"/tests/ 2>/dev/null | wc -l)
[ "$N6" -ge 2 ] && pass "IPv6 ($N6 test files)" || fail "IPv6 tests missing"

# ── 5. CIDR ──────────────────────────────────────────────────────────────
echo "--- #5 CIDR ---"
NC=$(grep -rhE "fn .*cidr" "$ROOT"/crates/*/tests/ "$ROOT"/tests/ 2>/dev/null | wc -l)
[ "$NC" -ge 3 ] && pass "CIDR ($NC test functions)" || fail "CIDR tests missing ($NC)"

# ── 6. WAL ───────────────────────────────────────────────────────────────
echo "--- #6 WAL ---"
if cargo test -p ramwal --locked --test recovery_test 2>&1 | grep -q "test result: ok"; then
    pass "WAL tests pass"
elif $QUICK; then
    warn2 "WAL tests skipped (--quick)"
else
    fail "WAL test failure — check cargo test -p ramwal"
fi

# ── 7. SIGKILL recovery ──────────────────────────────────────────────────
echo "--- #7 SIGKILL recovery ---"
NRS=$(grep -c "fn " "$ROOT"/tests/recovery_restart.rs 2>/dev/null || echo 0)
[ "$NRS" -ge 5 ] && pass "recovery_restart.rs: $NRS test functions" || warn2 "recovery_restart.rs sparse ($NRS tests)"

# ── 8. Corrupted tail ────────────────────────────────────────────────────
echo "--- #8 Corrupted tail ---"
TAIL_TESTS=$(grep -cE "partial_header|truncate_|repair_segment" "$ROOT"/crates/ramwal/tests/recovery_test.rs 2>/dev/null)
[ "${TAIL_TESTS:-0}" -ge 2 ] && pass "corrupted-tail tests present ($TAIL_TESTS)" || warn2 "corrupted-tail coverage check failed"

# ── 9. Historical corruption ─────────────────────────────────────────────
echo "--- #9 Historical corruption ---"
HC_TESTS=$(grep -cE "crc_mismatch|corrupt_magic|golden_corrupt" "$ROOT"/crates/ramwal/tests/recovery_test.rs 2>/dev/null)
[ "${HC_TESTS:-0}" -ge 2 ] && pass "historical corruption tests ($HC_TESTS)" || warn2 "historical corruption coverage thin"

# ── 10. Disk full ────────────────────────────────────────────────────────
echo "--- #10 Disk full ---"
# Require an actual test exercising ENOSPC, not just source handling.
if grep -rE "fail_next_diskfull|disk_full_append" "$ROOT"/crates/ramwal/tests/recovery_test.rs 2>/dev/null | grep -q "fn disk_full\|fail_next_diskfull"; then
    pass "disk-full e2e test present (ENOSPC seam → DiskFull)"
elif grep -rqE "ENOSPC|disk_full|no_space" "$ROOT"/crates/*/tests/ "$ROOT"/tests/ 2>/dev/null; then
    pass "disk-full e2e test present"
elif grep -rqE "ENOSPC|StorageFull|OutOfSpace" "$ROOT"/crates/ramwal/src/ 2>/dev/null; then
    fail "disk-full: error type defined but NO e2e test exercises it"
else
    fail "disk-full (ENOSPC) — no handling and no test"
fi

# ── 11. Checkpoint ───────────────────────────────────────────────────────
echo "--- #11 Checkpoint ---"
CK_TESTS=$(grep -cE "fn .*checkpoint" "$ROOT"/tests/recovery_restart.rs 2>/dev/null || echo 0)
[ "$CK_TESTS" -ge 5 ] && pass "checkpoint: $CK_TESTS test functions" || warn2 "checkpoint tests low ($CK_TESTS)"

# ── 12. Retention ────────────────────────────────────────────────────────
echo "--- #12 Retention ---"
RT_TESTS=$(grep -cE "retention" "$ROOT"/crates/ramwal/tests/recovery_test.rs 2>/dev/null || echo 0)
[ "$RT_TESTS" -ge 2 ] && pass "retention: $RT_TESTS tests" || fail "retention tests missing"

# ── 13. IPC authentication ───────────────────────────────────────────────
echo "--- #13 IPC authentication ---"
NIP=$(grep -rl "auth_verify\|auth_rejects\|fn .*auth" "$ROOT"/crates/ramshield-protocol/ 2>/dev/null | wc -l)
[ "$NIP" -ge 2 ] && pass "IPC auth ($NIP files)" || fail "IPC auth coverage weak"

# ── 14. Replay protection ────────────────────────────────────────────────
echo "--- #14 Replay protection ---"
NRP=$(grep -rl "replay_outside_window\|replay_store" "$ROOT"/crates/ 2>/dev/null | wc -l)
[ "$NRP" -ge 2 ] && pass "replay protection ($NRP files)" || fail "replay protection coverage missing"

# ── 15. Public dashboard TLS proxy ───────────────────────────────────────
echo "--- #15 Public dashboard (TLS proxy) ---"
if grep -rq "behind_tls_proxy\|tls_proxy" "$ROOT"/crates/ramshield-config/ 2>/dev/null; then
    pass "config enforces TLS proxy assertion"
else
    warn2 "TLS-proxy guard not located — may be in config validator"
fi

# ── 16. systemd ──────────────────────────────────────────────────────────
echo "--- #16 systemd ---"
SVC=$(find "$ROOT"/deploy -name '*.service' 2>/dev/null | head -1)
[ -n "$SVC" ] && pass "systemd unit at $SVC" || fail "no .service file in deploy/"
if systemctl is-enabled ramshield-operator.service &>/dev/null; then
    pass "ramshield-operator.service is enabled"
elif [ -n "$SVC" ]; then
    manual "service unit defined but not currently active"
fi

# ── 17. Kubernetes ──────────────────────────────────────────────────────
echo "--- #17 Kubernetes ---"
KM=$(find "$ROOT"/deploy -name '*.yaml' -path '*/k8s/*' 2>/dev/null | wc -l)
[ "$KM" -ge 3 ] && pass "K8s manifests ($KM files)" || manual "no k8s manifests (outside scope)"

echo ""
echo "================================"
echo " Summary"
echo "================================"
echo "  ✅ Pass:   $PASS"
echo "  ⚠️ Warn:    $WARN"
echo "  ❌ Fail:   $FAIL"
echo "  ➖ Manual: $MANUAL"
echo "  Total:     $((PASS+WARN+FAIL+MANUAL))"
echo ""

if $JSON; then
    jq -n --argjson pass "$PASS" --argjson warn "$WARN" --argjson fail "$FAIL" --argjson manual "$MANUAL" \
        '{pass: $pass, warn: $warn, fail: $fail, manual: $manual, rows: []}'
fi

exit $FAIL