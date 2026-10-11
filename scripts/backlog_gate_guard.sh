#!/usr/bin/env bash
set -euo pipefail
inventory=${INVENTORY_RESULT:-unknown}
formatting=${FORMAT_RESULT:-unknown}
tpacket=${TPACKET_RESULT:-unknown}
core=${CORE_MATRIX_RESULT:-unknown}
shm_abi=${SHM_ABI_RESULT:-unknown}
diff_check=${DIFF_CHECK_RESULT:-unknown}
event_name=${EVENT_NAME:-unknown}
run_full_workspace=${RUN_FULL_WORKSPACE:-}
evidence_path=${EVIDENCE_STATUS_PATH:-evidence/acceptance-status.md}
summary_path=${GITHUB_STEP_SUMMARY:-}
mkdir -p "$(dirname "$evidence_path")"
failures=''
failed=0
for entry in "inventory:$inventory" "formatting:$formatting" "TPACKET:$tpacket" "SHM ABI:$shm_abi" "diff-check:$diff_check"; do
  name=${entry%%:*}
  result=${entry#*:}
  if [[ "$result" != success ]]; then
    failures+="- $name=$result (required result is success)"$'\n'
    failed=1
  fi
done
partial=0
case "$core" in
  success) ;;
  skipped)
    if [[ "$event_name" == workflow_dispatch && "$run_full_workspace" == false ]]; then partial=1
    else failures+="- core-matrix=skipped (only workflow_dispatch with run_full_workspace=false permits this)"$'\n'; failed=1; fi
    ;;
  *) failures+="- core-matrix=$core (expected success or the explicit opt-out skip)"$'\n'; failed=1 ;;
esac
if [[ "$failed" -eq 1 && "$partial" -eq 1 ]]; then
  status="FAIL — NOT ACCEPTANCE; PARTIAL — NOT FULL ACCEPTANCE"
  detail="The core workspace matrix was intentionally skipped by workflow_dispatch with run_full_workspace=false. This run is partial evidence only. One or more other required gates also failed."
elif [[ "$failed" -eq 1 ]]; then
  status="FAIL — NOT ACCEPTANCE"
  detail="Required gate results did not satisfy the acceptance contract."
elif [[ "$partial" -eq 1 ]]; then
  status="PARTIAL — NOT FULL ACCEPTANCE"
  detail="The core workspace matrix was intentionally skipped by workflow_dispatch with run_full_workspace=false. This run is partial evidence only and must not be represented as full acceptance."
else
  status="GATE PASS — all required gates succeeded"
  detail="All required Backlog Gate jobs succeeded. This workflow result alone is not a project-wide or release acceptance claim."
fi
{
  printf '# Backlog Gate acceptance status\n\n'
  printf -- '- Result: **%s**\n' "$status"
  printf -- '- Event: %s\n' "$event_name"
  printf -- '- run_full_workspace input: %s\n' "${run_full_workspace:-<not set>}"
  printf -- '- inventory: %s\n' "$inventory"
  printf -- '- formatting: %s\n' "$formatting"
  printf -- '- TPACKET: %s\n' "$tpacket"
  printf -- '- core-matrix: %s\n' "$core"
  printf -- '- SHM ABI: %s\n' "$shm_abi"
  printf -- '- diff-check: %s\n' "$diff_check"
  printf '\n%s\n' "$detail"
  if [[ "$failed" -eq 1 ]]; then printf '\n## Rejected results\n%b' "$failures"; fi
} > "$evidence_path"
if [[ -n "$summary_path" ]]; then cat "$evidence_path" >> "$summary_path"; fi
if [[ "$failed" -eq 1 ]]; then exit 1; fi
exit 0
