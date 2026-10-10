#!/usr/bin/env bash
# Local parity with .github/workflows/backlog.yml (subset runnable offline).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== Backlog gate (local) =="
echo "SHA: $(git rev-parse HEAD)"
test -f BACKLOG.md

echo "-- OPEN task counts (text status in BACKLOG.md) --"
awk '
  /^## P0/ {sec=1; next}
  /^## / {sec=0}
  sec && /^\| A-[0-9]+ / && /\| OPEN[ |]*$/ {c++}
  END {print "OPEN P0:", c+0}
' BACKLOG.md
awk '
  /^## P1/ {sec=1; next}
  /^## / {sec=0}
  sec && /^\| A-[0-9]+ / && /\| OPEN[ |]*$/ {c++}
  END {print "OPEN P1:", c+0}
' BACKLOG.md

echo "-- A-030 fmt --"
cargo fmt --all -- --check

echo "-- A-016 tpacket --"
cargo test --locked --lib tpacket -- --nocapture

echo "-- A-031 check --"
if command -v bpf-linker >/dev/null 2>&1; then
  cargo check --workspace --locked --all-targets --all-features
else
  echo "bpf-linker missing: checking -p ramshield only"
  cargo check -p ramshield --locked --all-targets
fi

if git rev-parse origin/master >/dev/null 2>&1; then
  echo "-- git diff --check origin/master...HEAD --"
  git diff --check origin/master...HEAD || true
elif git rev-parse origin/main >/dev/null 2>&1; then
  echo "-- git diff --check origin/main...HEAD --"
  git diff --check origin/main...HEAD || true
else
  echo "No origin/master|main; skip diff --check"
fi

echo "Local backlog gate finished."
