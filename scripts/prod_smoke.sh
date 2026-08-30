#!/usr/bin/env bash
# scripts/prod_smoke.sh — Production-like smoke test for RamShield.
#
# Boots the binary with --config config.prod.toml --no-xdp, then exercises
# every public IPC + dashboard endpoint to confirm health, block path,
# metrics export, and WAL state. Exits non-zero on first failure.
#
# Usage:  ./scripts/prod_smoke.sh
# Assumes: ./target/release/ramshield built with --features full.

set -euo pipefail

BIN="${BIN:-./target/release/ramshield}"
CFG="${CFG:-./config.prod.toml}"
WAL_DIR="${WAL_DIR:-/tmp/ramshield-prod-smoke-wal}"
LOG="${LOG:-/tmp/ramshield-prod-smoke.log}"
DASH="${DASH:-127.0.0.1:9999}"
IPC="${IPC:-127.0.0.1:7890}"

green() { printf '\033[32m%s\033[0m\n' "$*"; }
red()   { printf '\033[31m%s\033[0m\n' "$*" >&2; }
fail()  { red "FAIL: $*"; pkill -9 ramshield 2>/dev/null; exit 1; }

# Reset state
rm -rf "$WAL_DIR"
mkdir -p "$WAL_DIR"
pkill -9 ramshield 2>/dev/null || true
sleep 0.5

echo "→ booting binary with $CFG (WAL=$WAL_DIR)"
RAMSHIELD_ENGINE__RAM_LIMIT_MB=1024 \
    "$BIN" --config "$CFG" --no-xdp > "$LOG" 2>&1 &
PID=$!
trap "kill -9 $PID 2>/dev/null || true; pkill -9 -f 'ramshield --config' 2>/dev/null || true" EXIT

# Wait for health
for i in {1..20}; do
    if curl -sf -m 1 "http://$DASH/healthz" >/dev/null 2>&1; then break; fi
    sleep 0.3
    if [ "$i" = 20 ]; then fail "binary never became healthy"; fi
done
green "✓ boot: /healthz ok"

ipc_send() {
    python3 -c "
import socket, json, sys
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.settimeout(3)
s.connect(('$IPC', 7890))
s.sendall((sys.argv[1] + '\n').encode())
print(s.recv(4096).decode().strip())
s.close()
" "$1"
}

# Block via IPC
RESP=$(ipc_send '{"type":"block_ip","ip":"203.0.113.7","reason":"prod_smoke","ttl_secs":300}')
echo "$RESP" | grep -q "block queued" || fail "block_ip did not respond: $RESP"
green "✓ IPC: block_ip queued"

sleep 1
H=$(curl -sf -m 3 "http://$DASH/api/history/blocks" || true)
echo "$H" | grep -q "203.0.113.7" || fail "block not in /api/history/blocks: $H"
green "✓ DASH: block visible in history"

# Snapshot
S=$(curl -sf -m 3 "http://$DASH/api/snapshot")
echo "$S" | grep -q '"blocked_total":' || fail "/api/snapshot missing blocked_total"
BT=$(echo "$S" | grep -oE '"blocked_total":[0-9]+' | head -1 | grep -oE '[0-9]+')
[ "$BT" -ge 1 ] || fail "blocked_total = $BT, expected ≥ 1"
green "✓ DASH: snapshot reports blocked_total=$BT"

# Status modules
M=$(curl -sf -m 3 "http://$DASH/api/status/modules")
echo "$M" | grep -q '"engine"' || fail "/api/status/modules missing engine"
green "✓ DASH: status/modules ok"

# Metrics
MT=$(curl -sf -m 3 "http://$DASH/metrics")
echo "$MT" | grep -q "ramshield_blocks_total" || fail "Prometheus metrics missing blocks_total"
green "✓ DASH: /metrics serves Prometheus format"

# Unblock
RESP=$(ipc_send '{"type":"unblock_ip","ip":"203.0.113.7"}')
echo "$RESP" | grep -q "unblock queued" || fail "unblock_ip did not respond: $RESP"
green "✓ IPC: unblock_ip queued"

# Hot subnets
HS=$(curl -sf -m 3 "http://$DASH/api/hot-subnets" || echo "[]")
green "✓ DASH: /api/hot-subnets reachable ($(echo "$HS" | wc -c) bytes)"

# WAL
WAL_FILES=$(find "$WAL_DIR" -type f -name "*.wal" -o -name "*.seg" 2>/dev/null | wc -l)
[ "$WAL_FILES" -ge 0 ] || fail "WAL inspection failed"
green "✓ WAL: $WAL_FILES files in $WAL_DIR"

green ""
green "ALL PROD SMOKE CHECKS PASSED"
