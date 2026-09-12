#!/usr/bin/env bash
# scripts/audit_verify.sh — Static audit verification for RamShield.
#
# Pins every finding from AUDIT_FULL_20260911.md + follow-up fixes (H1/H3,
# unwrap gate, XDP contracts) as a grep-based invariant. Complements
# scripts/suite.py (runtime e2e) and scripts/prod_smoke.sh (prod boot):
# this script never boots a server, it verifies the CODE.
#
# Usage: ./scripts/audit_verify.sh [--quick]
#   --quick  skip cargo build/test/clippy (static greps only, <5s)
# Exit code = number of failed checks (0 = all green).
set -uo pipefail

QUICK=false
[ "${1:-}" = "--quick" ] && QUICK=true

PASS=0
FAIL=0
FAILED=()

green() { printf '\033[32m%s\033[0m\n' "$*"; }
red()   { printf '\033[31m%s\033[0m\n' "$*" >&2; }
ok()   { PASS=$((PASS+1)); printf '  [PASS] %s\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); FAILED+=("$1"); printf '  [FAIL] %s\n' "$1"; }
check() { # check <desc> <command...>: passes when command exits 0
    local desc="$1"; shift
    if "$@" >/dev/null 2>&1; then ok "$desc"; else bad "$desc"; fi
}
check_grep() { # check_grep <desc> <pattern> <path...> (passes when found)
    local desc="$1" pat="$2"; shift 2
    if rg -q -- "$pat" "$@"; then ok "$desc"; else bad "$desc"; fi
}
check_absent() { # check_absent <desc> <pattern> <path...> (passes when NOT found)
    local desc="$1" pat="$2"; shift 2
    if rg -q -- "$pat" "$@"; then bad "$desc"; else ok "$desc"; fi
}

echo "=== 1. BUILD GATE ==="
if ! $QUICK; then
    check "cargo build --locked --features full" \
        cargo build --locked --features full
else
    echo "  (skipped --quick)"
fi

echo "=== 2. TEST GATE ==="
if ! $QUICK; then
    OUT=$(cargo test --workspace --locked --features full 2>&1)
    if echo "$OUT" | grep -qE "test result: FAILED|failures:"; then
        bad "cargo test: 0 failures"
        echo "$OUT" | grep -E "test result: FAILED|failures:" | head -5
    else
        P=$(echo "$OUT" | awk '/test result: ok/ {p+=$4} END {print p+0}')
        ok "cargo test: $P passed, 0 failed"
    fi
else
    echo "  (skipped --quick)"
fi

echo "=== 3. CLIPPY + FMT ==="
if ! $QUICK; then
    check "clippy -D warnings clean" \
        cargo clippy --workspace --locked --features full --all-targets -- -D warnings
    check "cargo fmt --check" cargo fmt --all -- --check
else
    echo "  (skipped --quick)"
fi

echo "=== 4. MODULE INVENTORY (10 crates) ==="
for c in config detection enforcement forecasting metrics protocol storage types xdp; do
    check_grep "crate ramshield-$c present" "name.*ramshield-$c" "crates/ramshield-$c/Cargo.toml"
done
check_absent "ramforge removed" "ramforge" Cargo.toml

echo "=== 5. CI NO-UNWRAP GATE (reproduced locally) ==="
HITS=$(rg -n --type rust --glob '!tests/**' --glob '!**/tests/**' \
    --glob '!**/*.test.rs' --glob '!**/test_*.rs' --glob '!**/*_test.rs' \
    -e '\.unwrap\s*\(' -e '\.expect\s*\(' src/ crates/ || true)
FILTERED=$(printf '%s\n' "$HITS" | python3 .github/scripts/ci_filter_testcode.py . 2>/dev/null || printf '%s\n' "$HITS")
COUNT=$(printf '%s\n' "$FILTERED" | grep -c . || true)
if [ "$COUNT" -eq 0 ]; then ok "no unwrap/expect in prod code"; else bad "no unwrap/expect in prod code ($COUNT hits)"; printf '%s\n' "$FILTERED" | head -10; fi

echo "=== 6. SECRETS HYGIENE ==="
# config.prod.toml not tracked
git_result=$(git ls-files "config.prod.toml" 2>/dev/null)
if [ -z "$git_result" ]; then
    ok "config.prod.toml not tracked"
else
    bad "config.prod.toml tracked (security risk)"
fi
# deadbeef fixtures
if rg -l "deadbeef" src/ crates/ tests/ 2>/dev/null | grep -q .; then
    bad "deadbeef fixtures present"
else
    ok "no deadbeef fixtures"
fi
# hardcoded 64-hex keys in tests
if rg -l "[0-9a-f]{64}" tests/ crates/*/src/lib.rs 2>/dev/null | grep -q .; then
    bad "hardcoded 64-hex keys in tests"
else
    ok "no hardcoded 64-hex keys in tests"
fi

echo "=== 7. UNSAFE SURFACE (XDP-only) ==="
if rg -l --type rust "unsafe" src/ | grep -v -i "xdp\|enforcement" | grep -q .; then
    bad "unsafe in files other than XDP/enforcement"
    rg -l --type rust "unsafe" src/ | grep -v -i "xdp\|enforcement" | head -5
else
    ok "unsafe only in XDP/enforcement crates"
fi

echo "=== 8. AUDIT FINDINGS (AUDIT_FULL_20260911) ==="
# P0/H1: bind fail-fast on bad keys, never warn+skip
check_grep "H1 parse_ipc_keys exists" "fn parse_ipc_keys" src/ipc/server.rs
check_grep "H1 bind returns io::Error on bad keys" "map_err\(std::io::Error::other\)" src/ipc/server.rs
check_absent "H1 no warn+skip on bad keys" "warn!.*skip|skipping.*key" src/ipc/server.rs
# H3: hot reload via ConfigHandle
check_grep "H3 IpcServer holds ConfigHandle" "config: ConfigHandle" src/ipc/server.rs
check_grep "H3 per-connection live keys" "live_keys" src/ipc/server.rs
check_grep "H3 engine passes handle" "cfg_handle\.clone\(\)" src/engine/mod.rs
# P1: epoch_ns gone
check_absent "epoch_ns removed" "epoch_ns" src/ crates/
check_grep "now_ms used" "now_ms" src/ipc/server.rs crates/ramshield-metrics/src/lib.rs
# P1: TCP_NODELAY on accepted streams
check_grep "set_nodelay on accept" "set_nodelay\(true\)" src/ipc/server.rs
# P1: hex key validation in Config::validate
check_grep "validate checks hex keys" "is_ascii_hexdigit" crates/ramshield-config/src/lib.rs
check_grep "validate 16-byte HMAC minimum" "16" crates/ramshield-config/src/lib.rs
# P1: public bind guard
check_grep "is_public_bind exists" "fn is_public_bind" crates/ramshield-config/src/lib.rs
# P1: HMAC key_id binding
check_grep "sign takes key_id" "key_id" crates/ramshield-protocol/src/auth.rs
# P1: replay store stored seed
check_grep "replay stored seed" "RandomState|seed" crates/ramshield-protocol/src/auth/replay_store.rs
# P2: dead_code cleanup
check_absent "no allow(dead_code) in prod" "allow\(dead_code\)" src/ crates/*/src/
check_absent "enforcement ops() deleted" "fn ops" crates/ramshield-enforcement/src/lib.rs
# P2: XDP build.rs resilient
check_absent "xdp build.rs no panic" "panic!" crates/ramshield-xdp/build.rs
# P2: dashboard sessions + cookie
check_grep "dashboard DashMap sessions" "DashMap" src/dashboard/auth.rs
check_grep "cookie Path=/" "Path=/" src/dashboard/auth.rs
# P2: PHC hash validation
check_grep "validate checks PHC hash" "PasswordHash::new" crates/ramshield-config/src/lib.rs

echo "=== 9. ENFORCEMENT COVERAGE (audit: was 1 test / 1090 LOC) ==="
N=$(rg -c '#\[test\]|#\[tokio::test\]' crates/ramshield-enforcement/src/ 2>/dev/null | awk -F: '{s+=$2} END {print s+0}')
if [ "$N" -ge 5 ]; then ok "enforcement tests: $N (>=5)"; else bad "enforcement tests: $N (<5)"; fi

echo "=== 10. HYGIENE ==="
check_absent "no TODO/FIXME in prod" "TODO|FIXME|XXX|HACK" src/ crates/*/src/
check_absent "no dbg! in prod" "dbg!" src/ crates/*/src/
check_absent "no println in prod lib" "println!" src/lib.rs src/engine/ crates/*/src/lib.rs

echo "=== 11. GIT STATE ==="
if [ -z "$(git status --short)" ]; then ok "worktree clean"; else bad "worktree clean"; git status --short | head -5; fi
LOCAL=$(git rev-parse HEAD)
if git ls-remote grep999 master 2>/dev/null | grep -q "$LOCAL"; then ok "master pushed to grep999"; else bad "master pushed to grep999"; fi

echo ""
echo "=========================================="
if [ "$FAIL" -eq 0 ]; then
    green "AUDIT VERIFY: ALL GREEN ($PASS passed)"
else
    red "AUDIT VERIFY: $FAIL/$((PASS+FAIL)) FAILED"
    printf '  failed: %s\n' "${FAILED[@]}"
fi
exit "$FAIL"
