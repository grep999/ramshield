#!/usr/bin/env bash
# Check committed changes from the merge base; fail closed without a valid baseline.
set -euo pipefail
base_ref=${1:-origin/master}
head_ref=${2:-HEAD}
if ! merge_base=$(git merge-base "$base_ref" "$head_ref" 2>/dev/null); then
  echo "ERROR: no merge base exists for diff-check between '$base_ref' and '$head_ref'; refusing to report a whitespace scan as passed." >&2
  exit 2
fi
printf 'Validated diff-check baseline: %s\nCandidate: %s\n' "$merge_base" "$head_ref"
git diff --check "$merge_base" "$head_ref"
