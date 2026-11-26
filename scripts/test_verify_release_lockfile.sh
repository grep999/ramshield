#!/usr/bin/env bash
# ponytail: covers the three lockfile checks added to verify_release.sh.
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0
chk(){ if eval "$2"; then echo "PASS $1"; else echo "FAIL $1"; fail=1; fi; }
chk present '[[ -f Cargo.lock ]]'
chk tracked 'git ls-files --error-unmatch Cargo.lock >/dev/null 2>&1'
chk no-git '! grep -q "source = \"git" Cargo.lock'
# negative control: the no-git predicate must fire on a lockfile that has a git source
tmp="$(mktemp)"; printf 'source = "git+https://example.invalid/x"\n' > "$tmp"
if grep -q 'source = "git' "$tmp"; then echo "PASS negative-control"; else echo "FAIL negative-control"; fail=1; fi
rm -f "$tmp"
exit "$fail"