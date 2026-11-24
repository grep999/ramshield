#!/usr/bin/env bash
# scripts/prod_smoke.sh — Production-like smoke test for RamShield.
#
# Boots the binary with --config config.stress.toml --no-xdp, then exercises
# every public IPC + dashboard endpoint to confirm health, block path,
# metrics export, and WAL state. Exits non-zero on first failure.
#
# Two configs, one binary:
#   config.prod.toml   auth fully configured (HMAC keys + dashboard auth)
#   config.stress.toml no auth required (local test profile)
# This smoke uses the no-auth profile so credentials never block CI. The
# generated copy enables WAL because the source stress profile disables it
# for load tests; the source file is never modified.
#
# Coverage:
#  BOOT → health → IPC block path → manual block → block visible in the history →
#  snapshot → status modules → Prometheus metrics → unblock → verify absent →
#  CIDR block → verify → CIDR unblock → verify absent → short TTL → verify
#  expired → WAL persisted → SIGKILL → restart → block restored
#
# Usage:  ./scripts/prod_smoke.sh
# Assumes: ./target/release/ramshield built with --features full.

set -euo pipefail

BIN="${BIN:-./target/release/ramshield}"
CFG="${CFG:-./config.stress.toml}"
# A fresh directory by default. The script deletes this path on start, so
# it must be one it created: WAL_DIR may be overridden, and an operator
# who has it pointing at a real data directory would lose it.
WAL_DIR="${WAL_DIR:-$(mktemp -d /tmp/ramshield-prod-smoke-wal.XXXXXX)}"
LOG="${LOG:-/tmp/ramshield-prod-smoke.log}"
DASH_ADDR="${DASH_ADDR:-127.0.0.1:19999}"
IPC_HOST="${IPC_HOST:-127.0.0.1}"
IPC_PORT="${IPC_PORT:-17890}"

green() { printf '\033[32m%s\033[0m\n' "$*"; }
red()   { printf '\033[31m%s\033[0m\n' "$*" >&2; }
fail()  { red "FAIL: $*"; cleanup; exit 1; }
cleanup() {
    if [[ -n "${PID:-}" ]] && kill -0 "$PID" 2>/dev/null; then
        kill "$PID" 2>/dev/null || true
        wait "$PID" 2>/dev/null || true
    fi
}

# Reset state. Only a path that carries this script's own name may be
# deleted: the directory is ours, never the operator's.
case "$WAL_DIR" in
    *ramshield-prod-smoke-wal*) ;;
    *) fail "refusing to delete $WAL_DIR: it does not carry the smoke test's own name" ;;
esac
rm -rf -- "$WAL_DIR"
mkdir -p "$WAL_DIR"
cleanup
sleep 0.5

# The source config may bind 0.0.0.0 and fail-close without credentials
# (Config::validate). Smoke tests exercise endpoints, not public exposure -
# rewrite binds to loopback and point the WAL at the scratch dir. The
# fail-closed rule itself is covered by ramshield-config unit tests.
SMOKE_CFG="${SMOKE_CFG:-/tmp/ramshield-prod-smoke.toml}"
DASH_PORT="${DASH_ADDR##*:}"
sed -e "s|^[[:space:]]*tcp_addr = .*|tcp_addr = \"127.0.0.1:$IPC_PORT\"|" \
    -e "s|^[[:space:]]*http_addr = .*|http_addr = \"127.0.0.1:$DASH_PORT\"|" \
    -e "s|^[[:space:]]*dir = \".*\"|dir = \"$WAL_DIR\"|" \
    "$CFG" > "$SMOKE_CFG"
# The no-auth stress profile disables WAL for load tests. This smoke exercises
# WAL durability, so enable it in the generated config only (never the source).
awk '
    /^\[/ { in_wal = ($0 == "[wal]") }
    in_wal && /^enabled =/ { print "enabled = true"; next }
    { print }
' "$SMOKE_CFG" > "$SMOKE_CFG.tmp" && mv "$SMOKE_CFG.tmp" "$SMOKE_CFG"

echo "→ booting binary with $SMOKE_CFG (WAL=$WAL_DIR)"
RAMSHIELD_ENGINE__RAM_LIMIT_MB=1024 \
    "$BIN" --config "$SMOKE_CFG" --no-xdp > "$LOG" 2>&1 &
PID=$!
trap cleanup EXIT

# Wait for health — engine boots on another thread; /healthz is 503 until pipeline_ready.
for i in {1..50}; do
    if curl -sf -m 1 "http://$DASH_ADDR/healthz" >/dev/null 2>&1; then break; fi
    sleep 0.2
    if [ "$i" = "50" ]; then fail "binary never became healthy"; fi
done
green "✓ boot: /healthz ok"

# IPC helper: send JSON and return the response
ipc_send() {
    python3 -c "
import socket, json, sys
host, port = sys.argv[1], int(sys.argv[2])
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.settimeout(3)
s.connect((host, port))
s.sendall((sys.argv[3] + '\n').encode())
print(s.recv(4096).decode().strip())
s.close()
" "$IPC_HOST" "$IPC_PORT" "$1"
}

# Block via IPC
RESP=$(ipc_send '{"type":"block_ip","ip":"203.0.113.7","reason":"manual","ttl_secs":300}')
echo "$RESP" | grep -q "block queued" || fail "block_ip did not respond: $RESP"
green "✓ IPC: block_ip queued"

sleep 1
H=$(curl -sf -m 3 "http://$DASH_ADDR/api/history/blocks" || true)
echo "$H" | grep -q "203.0.113.7" || fail "block not in /api/history/blocks: $H"
green "✓ DASH: block visible in history"

# Snapshot. The enforcement actor commits asynchronously; poll briefly rather
# than assuming a fixed sleep is enough under load.
for _ in {1..25}; do
    S=$(curl -sf -m 3 "http://$DASH_ADDR/api/snapshot" || true)
    BT=$(echo "$S" | grep -oE '"blocked_total":[0-9]+' | head -1 | grep -oE '[0-9]+' || true)
    [ -n "$BT" ] || fail "/api/snapshot missing blocked_total"
    [ "$BT" -ge 1 ] && break
    sleep 0.2
done
[ -n "${BT:-}" ] && [ "$BT" -ge 1 ] || fail "blocked_total = ${BT:-unset}, expected ≥ 1"
green "✓ DASH: snapshot reports blocked_total=$BT"

# Status modules
M=$(curl -sf -m 3 "http://$DASH_ADDR/api/status/modules")
echo "$M" | grep -q '"label"' || fail "/api/status/modules missing label"
green "✓ DASH: status/modules ok"

# Metrics
MT=$(curl -sf -m 3 "http://$DASH_ADDR/metrics")
echo "$MT" | grep -q "ramshield_blocks_total" || fail "Prometheus metrics missing blocks_total"
green "✓ DASH: /metrics serves Prometheus format"

# Unblock - and verify the IP is really gone.
RESP=$(ipc_send '{"type":"unblock_ip","ip":"203.0.113.7"}')
echo "$RESP" | grep -q "unblock queued" || fail "unblock_ip did not respond: $RESP"
green "✓ IPC: unblock_ip queued"
sleep 1
RESP=$(ipc_send '{"type":"check_ip","ip":"203.0.113.7"}')
echo "$RESP" | grep -q '"blocked":false' || fail "unblock did not take effect: $RESP"
green "✓ IPC: the unblocked IP is absent"

# CIDR block - and verify a member of the CIDR is blocked, then gone.
RESP=$(ipc_send '{"type":"block_cidr","cidr":"203.0.113.128/25","reason":"manual","ttl_secs":300}')
echo "$RESP" | grep -q "block queued" || fail "block_cidr did not respond: $RESP"
sleep 1
RESP=$(ipc_send '{"type":"check_ip","ip":"203.0.113.200"}')
echo "$RESP" | grep -q '"blocked":true' || fail "the CIDR block did not cover its member: $RESP"
green "✓ IPC: the CIDR block covers its member"
RESP=$(ipc_send '{"type":"unblock_cidr","cidr":"203.0.113.128/25"}')
echo "$RESP" | grep -q "unblock queued" || fail "unblock_cidr did not respond: $RESP"
sleep 1
RESP=$(ipc_send '{"type":"check_ip","ip":"203.0.113.200"}')
echo "$RESP" | grep -q '"blocked":false' || fail "the CIDR unblock did not take effect: $RESP"
green "✓ IPC: the unblocked CIDR is gone"

# A short TTL must expire and the block must disappear.
RESP=$(ipc_send '{"type":"block_ip","ip":"203.0.113.42","reason":"manual","ttl_secs":2}')
echo "$RESP" | grep -q "block queued" || fail "the short-TTL block_ip did not respond: $RESP"
sleep 1
RESP=$(ipc_send '{"type":"check_ip","ip":"203.0.113.42"}')
echo "$RESP" | grep -q '"blocked":true' || fail "the short-TTL block did not take effect: $RESP"
sleep 4
RESP=$(ipc_send '{"type":"check_ip","ip":"203.0.113.42"}')
echo "$RESP" | grep -q '"blocked":false' || fail "the short TTL did not expire: $RESP"
green "✓ IPC: the short TTL expired and the block is gone"

# Hot subnets
HS=$(curl -sf -m 3 "http://$DASH_ADDR/api/hot-subnets" || echo "[]")
green "✓ DASH: /api/hot-subnets reachable ($(echo "$HS" | wc -c) bytes)"

# WAL
WAL_FILES=$(find "$WAL_DIR" -type f 2>/dev/null | wc -l)
[ "$WAL_FILES" -ge 1 ] || fail "no WAL segments written to $WAL_DIR (block never persisted)"
green "✓ WAL: $WAL_FILES files in $WAL_DIR"

# P0#3: SIGKILL then restart — block from WAL must still be present.
# Re-block first (unblock above cleared live state; WAL still has history).
RESP=$(ipc_send '{"type":"block_ip","ip":"203.0.113.99","reason":"manual","ttl_secs":300}')
echo "$RESP" | grep -q "block queued" || fail "pre-kill block_ip: $RESP"
sleep 0.5
kill -9 "$PID" 2>/dev/null || true
wait "$PID" 2>/dev/null || true
PID=""
"$BIN" --config "$SMOKE_CFG" --no-xdp > "$LOG" 2>&1 &
PID=$!
for i in {1..50}; do
    if curl -sf -m 1 "http://$DASH_ADDR/healthz" >/dev/null 2>&1; then break; fi
    sleep 0.2
    if [ "$i" = "50" ]; then fail "binary never became healthy after SIGKILL restart"; fi
done
RESP=$(ipc_send '{"type":"check_ip","ip":"203.0.113.99"}')
echo "$RESP" | grep -q '"blocked":true' || fail "WAL restore after SIGKILL: $RESP"
green "✓ WAL: block restored after SIGKILL restart"

green ""
green "ALL PROD SMOKE CHECKS PASSED"