#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
checker="$root/scripts/check_backlog_diff.sh"
tmp=$(mktemp -d)
fail() { echo "FAIL: $*" >&2; exit 1; }
repo="$tmp/repo"
mkdir -p "$repo"
git -C "$repo" init -q
git -C "$repo" config user.name 'Workflow Contract Test'
git -C "$repo" config user.email 'workflow-contract@example.invalid'
printf 'base\n' > "$repo/file.txt"
git -C "$repo" add file.txt
git -C "$repo" commit -qm base
base=$(git -C "$repo" rev-parse HEAD)
printf 'clean addition\n' >> "$repo/file.txt"
git -C "$repo" add file.txt
git -C "$repo" commit -qm clean-change
(cd "$repo" && bash "$checker" "$base" HEAD) >/dev/null
echo 'PASS: clean committed diff accepted'
printf 'bad trailing whitespace   \n' >> "$repo/file.txt"
git -C "$repo" add file.txt
git -C "$repo" commit -qm whitespace-defect
if (cd "$repo" && bash "$checker" "$base" HEAD) >"$tmp/whitespace.log" 2>&1; then fail 'committed whitespace defect was accepted'; fi
grep -Fq 'trailing whitespace' "$tmp/whitespace.log" || fail 'whitespace diagnostic missing'
echo 'PASS: committed whitespace defect rejected'
tree=$(git -C "$repo" rev-parse "$base^{tree}")
unrelated_root=$(cd "$repo" && git commit-tree "$tree" -m unrelated-root)
if (cd "$repo" && bash "$checker" "$base" "$unrelated_root") >"$tmp/no-base.log" 2>&1; then fail 'missing merge base was accepted'; fi
grep -Fq 'no merge base exists' "$tmp/no-base.log" || fail 'missing-base diagnostic absent'
echo 'PASS: missing merge base fails closed'
if (cd "$repo" && bash "$checker" does-not-exist HEAD) >"$tmp/no-ref.log" 2>&1; then fail 'missing baseline ref was accepted'; fi
grep -Fq 'no merge base exists' "$tmp/no-ref.log" || fail 'missing-ref diagnostic absent'
echo 'PASS: missing baseline ref fails closed'
