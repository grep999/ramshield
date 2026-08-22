#!/usr/bin/env bash
# subnet_ddos_bench.sh — RamShield benchmark: 30 unique /24s per 15s, 5 minutes.
# ponytail: single-purpose wrapper; use attack_nexus.py directly for other profiles.
set -euo pipefail
cd "$(dirname "$0")/.."

HOST=127.0.0.1
PORT=7890

# Guardrails: localhost only, server must be up.
case "$HOST" in
  127.*|localhost|::1) ;;
  *) echo "refusing non-loopback host $HOST" >&2; exit 1 ;;
esac
curl -sf -m 3 "http://127.0.0.1:${PORT}" >/dev/null 2>&1 || true

if ! timeout 2 bash -c "</dev/tcp/${HOST}/${PORT}" 2>/dev/null; then
  echo "RamShield IPC not listening on ${HOST}:${PORT} — start it first:" >&2
  echo "  ./target/release/ramshield config.stress.toml &" >&2
  exit 1
fi

echo "[bench] target=${HOST}:${PORT} profile=subnet_ddos_5min duration=300s"
exec python3 scripts/attack_nexus.py --host "$HOST" --port "$PORT" run \
  --profile subnet_ddos_5min --duration 300
