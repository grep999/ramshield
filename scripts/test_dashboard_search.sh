#!/usr/bin/env bash
set -euo pipefail

html="src/dashboard/static/index.html"
test -f "$html"

# Keep the visible search control connected to the filter function.
grep -Eq '<input[^>]*id="tableSearch"[^>]*oninput="filterTable\(\)"' "$html"
grep -Fq "function filterTable()" "$html"
grep -Fq "document.querySelectorAll('#quarantineBody tr')" "$html"

echo "Dashboard quarantine search wiring: PASS"
