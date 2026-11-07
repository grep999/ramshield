#!/bin/bash
# Batch 6: Resource Qualification — run benches, report key metrics.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Run hot-path benchmarks (already exist)
echo "═══ Hot-Path Benchmarks ═══"
if cargo bench --bench hot_paths 2>&1 | tail -30; then
  echo "✅ hot_paths benchmark complete"
else
  echo "❌ hot_paths benchmark failed"
fi

echo
echo "═══ WAL Write Benchmark ═══"
cargo bench --bench field_day 2>&1 | tail -5 || true

echo
echo "═══ Resource Inventory ═══"
echo "Binary size: $(stat -c%s target/release/ramshield 2>/dev/null || echo 'N/A') bytes"
echo "Workspace crates: $(ls -d crates/*/src 2>/dev/null | wc -l)"
echo "Total Rust LOC: $(find crates src -name '*.rs' -exec wc -l {} + 2>/dev/null | tail -1 | awk '{print $1}')"
echo
echo "Batch 6 complete — review ns/op values above for regressions."