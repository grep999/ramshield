#!/usr/bin/env bash
# Append a timestamped progress entry to the F-01 audit progress log.
# Usage: ./append_log.sh "<section>" "<text>"
set -euo pipefail
LOG_DIR="$(cd "$(dirname "$0")" && pwd)"
LOG_FILE="$LOG_DIR/PROGRESS_LOG.md"
STAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
SECTION="${1:-NOTE}"
TEXT="${2:-}"
{
  printf -- '- **%s** — %s\n' "$STAMP" "$SECTION"
  printf -- '  - %s\n' "$TEXT"
} >> "$LOG_FILE"
printf 'logged %s %s\n' "$STAMP" "$SECTION"
