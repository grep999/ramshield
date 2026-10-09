#!/usr/bin/env bash
set -Eeuo pipefail

# Review gate: source -> contract -> tests -> runtime artifacts.
# ponytail: Docker/runtime gates stay opt-in; CI must not need privileged XDP.
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
REPORT_DIR=${REVIEW_REPORT_DIR:-"$ROOT/.review"}
mkdir -p "$REPORT_DIR"
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
REPORT="$REPORT_DIR/review-$STAMP.log"
FAIL=0

run() {
  local name=$1; shift
  printf '\n[%s] %s\n' "$name" "$*" | tee -a "$REPORT"
  if "$@" >>"$REPORT" 2>&1; then
    printf '[PASS] %s\n' "$name" | tee -a "$REPORT"
  else
    printf '[FAIL] %s\n' "$name" | tee -a "$REPORT"
    FAIL=1
  fi
}

cd "$ROOT"
printf 'review_commit=%s\nstarted_utc=%s\n' "$(git rev-parse HEAD)" "$STAMP" | tee "$REPORT"

run diff-check git diff --check
if [[ -f docs/metrics/metric-keystore.json ]]; then
  run keystore-validate python3 scripts/validate_metric_keystore.py docs/metrics/metric-keystore.json
else
  printf '[SKIP] keystore-validate: no metric-keystore.json\n' | tee -a "$REPORT"
fi
TMP_JSONL=$(mktemp)
trap 'rm -f "$TMP_JSONL"' EXIT
if [[ -f docs/metrics/metric-keystore.json ]] && python3 scripts/export_metric_keystore.py --output "$TMP_JSONL" >>"$REPORT" 2>&1; then
  printf '[PASS] keystore-export\n' | tee -a "$REPORT"
else
  printf '[SKIP] keystore-export: metric-keystore.json not available\n' | tee -a "$REPORT"
fi
if [[ -f docs/metrics/metric-keystore.jsonl ]]; then
  if cmp -s "$TMP_JSONL" docs/metrics/metric-keystore.jsonl; then
    printf '[PASS] keystore-jsonl-current\n' | tee -a "$REPORT"
  else
    printf '[FAIL] keystore-jsonl-current: regenerate and commit docs/metrics/metric-keystore.jsonl\n' | tee -a "$REPORT"
    FAIL=1
  fi
else
  printf '[SKIP] keystore-jsonl-current: no metric-keystore.jsonl\n' | tee -a "$REPORT"
fi
run python-syntax python3 -m py_compile scripts/*.py
run rustfmt cargo fmt --all -- --check
run cargo-check cargo check --workspace --locked --all-targets --features full
run clippy cargo clippy --workspace --locked --all-targets --features full -- -D warnings
run cargo-test cargo test --workspace --locked --features full
if [[ -f scripts/validate_config_contract.py ]]; then
  run config-contract python3 scripts/validate_config_contract.py
else
  printf '[SKIP] config-contract: no validate_config_contract.py\n' | tee -a "$REPORT"
fi
run release-metadata bash -c 'cd "$ROOT" && fgrep -q "version = " Cargo.toml && echo "version defined"' | tee -a "$REPORT"

if [[ "${REVIEW_LIVE:-0}" == 1 ]]; then
  run metrics-smoke python3 scripts/metrics_smoke.py
  LOG=${REVIEW_TRACE_LOG:-}
  if [[ -n "$LOG" ]]; then
    run trace-audit python3 scripts/log_audit.py --log "$LOG"
  else
    printf '[FAIL] trace-audit: REVIEW_TRACE_LOG required when REVIEW_LIVE=1\n' | tee -a "$REPORT"
    FAIL=1
  fi
else
  printf '[SKIP] live runtime: set REVIEW_LIVE=1 with an isolated daemon\n' | tee -a "$REPORT"
fi

if [[ -n "${DOCKER_IMAGE:-}" ]]; then
  if command -v docker >/dev/null 2>&1; then
    run docker-image-inspect docker image inspect "$DOCKER_IMAGE"
    run docker-image-labels bash -c 'docker image inspect --format "{{json .Config.Labels}}" "$1" | grep -q "org.opencontainers.image.revision"' bash "$DOCKER_IMAGE"
  else
    printf '[FAIL] Docker image requested but docker is unavailable\n' | tee -a "$REPORT"
    FAIL=1
  fi
else
  printf '[SKIP] Docker image gate: set DOCKER_IMAGE=<immutable digest or tag>\n' | tee -a "$REPORT"
fi

printf '\nreport=%s\n' "$REPORT" | tee -a "$REPORT"
if (( FAIL )); then
  printf 'REVIEW FAILED\n' | tee -a "$REPORT"
  exit 1
fi
printf 'REVIEW PASSED\n' | tee -a "$REPORT"
