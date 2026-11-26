#!/usr/bin/env bash
# ponytail: one-shot capture; per-session isolation when E2 runs matter
set -euo pipefail

if [[ "${1:-}" == "--check" ]]; then
  bash -n "$0"
  echo "bench_capture.sh syntax ok"
  exit 0
fi

out="${1:-evidence/$(date +%Y%m%d)}"
mkdir -p "$out"

{
  echo "sha=$(git rev-parse HEAD)"
  echo "kernel=$(uname -r)"
  echo "cpu=$(lscpu | sed -n 's/^Model name:[[:space:]]*//p' | head -1)"
  echo "cores=$(nproc)"
  echo "rustc=$(rustc --version)"
} > "$out/env.txt"

cargo run --example attack_demo --quiet > "$out/demo.txt" 2>&1
cargo bench --bench hot_paths > "$out/hot_paths.txt" 2>&1
cargo bench --bench field_day > "$out/field_day.txt" 2>&1

echo "captured to $out"
