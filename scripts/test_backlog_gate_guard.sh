#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
guard="$root/scripts/backlog_gate_guard.sh"
workflow="$root/.github/workflows/backlog.yml"
tmp=$(mktemp -d)
pass=0
fail() { echo "FAIL: $*" >&2; exit 1; }
contains() { grep -Fq -- "$2" "$1" || fail "expected '$2' in $1"; }
run_case() {
  local name=$1 expected=$2 event=$3 full=$4 inv=$5 fmt=$6 tp=$7 core=$8 shm=$9 diff=${10}
  local dir="$tmp/$name" rc
  mkdir -p "$dir"
  if env INVENTORY_RESULT="$inv" FORMAT_RESULT="$fmt" TPACKET_RESULT="$tp" CORE_MATRIX_RESULT="$core" SHM_ABI_RESULT="$shm" DIFF_CHECK_RESULT="$diff" EVENT_NAME="$event" RUN_FULL_WORKSPACE="$full" EVIDENCE_STATUS_PATH="$dir/evidence.md" GITHUB_STEP_SUMMARY="$dir/summary.md" bash "$guard" >"$dir/log" 2>&1; then rc=0; else rc=$?; fi
  [[ $rc -eq $expected ]] || fail "$name expected exit $expected, got $rc"
  pass=$((pass+1)); echo "PASS: $name"
}
if [[ -f "$workflow" ]]; then
  contains "$workflow" 'bash scripts/backlog_gate_guard.sh'
  contains "$workflow" 'bash scripts/check_backlog_diff.sh origin/master HEAD'
  contains "$workflow" 'bash scripts/test_backlog_gate_guard.sh'
  contains "$workflow" 'bash scripts/test_backlog_diff_check.sh'
  if grep -Fq 'git diff --cached --check || true' "$workflow"; then fail 'suppressed fallback remains'; fi
  pass=$((pass+1)); echo 'PASS: workflow integration and fallback removal'
fi
run_case all-success 0 push '' success success success success success success
run_case inventory-failure 1 push '' failure success success success success success
run_case unexpected-skip 1 push '' success skipped success success success success
run_case core-skip-on-push 1 push false success success success skipped success success
run_case optout 0 workflow_dispatch false success success success skipped success success
contains "$tmp/optout/evidence.md" 'PARTIAL — NOT FULL ACCEPTANCE'
contains "$tmp/optout/summary.md" 'PARTIAL — NOT FULL ACCEPTANCE'
pass=$((pass+1)); echo 'PASS: opt-out is partial in evidence and summary'
run_case failed-optout 1 workflow_dispatch false failure success success skipped success success
contains "$tmp/failed-optout/evidence.md" 'PARTIAL — NOT FULL ACCEPTANCE'
pass=$((pass+1)); echo 'PASS: failed opt-out disclaims full acceptance'
run_case skip-when-full-requested 1 workflow_dispatch true success success success skipped success success
run_case core-failure 1 push '' success success success failure success success
run_case unknown-required 1 push '' success success success success success unknown
echo "All $pass gate-guard checks passed."
