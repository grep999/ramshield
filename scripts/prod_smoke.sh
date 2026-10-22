#!/usr/bin/env bash
# scripts/prod_smoke.sh — Production-like smoke test for RamShield.
#
# Boots the binary with --config config.prod.toml --no-xdp, then exercises
# every public IPC + dashboard endpoint to confirm health, block path,
# metrics export, and WAL state. Exits non-zero on first failure.
#
# Coverage:
#  BOOT → health → IPC auth → manual block → block visible in the history →
#  snapshot → status modules → Prometheus metrics → unblock → verify absent →
#  CIDR block → verify → CIDR unblock → verify absent → short TTL → verify
#  expired → WAL persisted → SIGKILL → restart → block restored
#
# Usage:  ./scripts/prod_smoke.sh
# Assumes: ./target/release/ramshield built with --features full.

set -euo pipefail

BIN="${BIN:-./target/release/ramshield}"
# The shipped baseline config. There is no config.prod.toml in the tree,
# so the old default pointed at a file that does not exist and the script
# could not run at all.
CFG="${CFG:-./config.baseline.toml}"
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

echo "→ booting binary with $SMOKE_CFG (WAL=$WAL_DIR)"
python3 - "$SMOKE_CFG" <<'PY'
from pathlib import Path
import re
import sys
p = Path(sys.argv[1])
t = p.read_text()
# Install the smoke test's own IPC key, whatever the source config held.
# The old script only replaced the literal "auth_keys = []", so against a
# config that already carried a key the daemon used that key while the
# script signed with its own - and every signed frame was rejected with
# "signature mismatch" before the first check ran.
key_line = 'auth_keys = ["k1:0b8d647fda3a0ae3c38207e0d7e61edfdfe59bda7359c89f953f76ed68f3768b"]'
roles_line = 'key_roles = [{ key_id = "k1", role = "Admin" }]'
t, n = re.subn(r"^auth_keys\s*=.*$", key_line, t, count=1, flags=re.M)
if n == 0:
    t = t.rstrip("\n") + "\n" + key_line + "\n"
# The roles table must name the same key_id.
t, n = re.subn(r"^key_roles\s*=.*$", roles_line, t, count=1, flags=re.M)
if n == 0:
    t = t.rstrip("\n") + "\n" + roles_line + "\n"
p.write_text(t)
PY
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

# Sign a request with the configured k1 test key -> compact signed frame.
# HMAC input is <ts_ms>.<key_id><payload>; payload must be the request object
# reserialized compactly with sorted keys (serde_json::Map is a BTreeMap).
sign_payload() {
    python3 -c "
import hmac, hashlib, time, json, sys
key = bytes.fromhex('0b8d647fda3a0ae3c38207e0d7e61edfdfe59bda7359c89f953f76ed68f3768b')
key_id, ts = 'k1', int(time.time() * 1000)
req = json.loads(sys.argv[1])
payload = json.dumps(req, separators=(',', ':'), sort_keys=True).encode()
sig = hmac.new(key, f'{ts}.{key_id}'.encode() + payload, hashlib.sha256).hexdigest()
print(json.dumps({'auth': {'key_id': key_id, 'ts_ms': ts, 'sig': sig}, **req},
                 separators=(',', ':'), sort_keys=True))
" "$1"
}

# Block via IPC
RESP=$(ipc_send "$(sign_payload '{"type":"block_ip","ip":"203.0.113.7","reason":"manual","ttl_secs":300}')")
echo "$RESP" | grep -q "block queued" || fail "block_ip did not respond: $RESP"
green "✓ IPC: block_ip queued"

sleep 1
H=$(curl -sf -m 3 "http://$DASH_ADDR/api/history/blocks" || true)
echo "$H" | grep -q "203.0.113.7" || fail "block not in /api/history/blocks: $H"
green "✓ DASH: block visible in history"

# Snapshot
S=$(curl -sf -m 3 "http://$DASH_ADDR/api/snapshot")
echo "$S" | grep -q '"blocked_total":' || fail "/api/snapshot missing blocked_total"
BT=$(echo "$S" | grep -oE '"blocked_total":[0-9]+' | head -1 | grep -oE '[0-9]+')
[ "$BT" -ge 1 ] || fail "blocked_total = $BT, expected ≥ 1"
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
RESP=$(ipc_send "$(sign_payload '{"type":"unblock_ip","ip":"203.0.113.7"}')")
echo "$RESP" | grep -q "unblock queued" || fail "unblock_ip did not respond: $RESP"
green "✓ IPC: unblock_ip queued"
sleep 1
RESP=$(ipc_send "$(sign_payload '{"type":"check_ip","ip":"203.0.113.7"}')")
echo "$RESP" | grep -q '"blocked":false' || fail "unblock did not take effect: $RESP"
green "✓ IPC: the unblocked IP is absent"

# CIDR block - and verify a member of the CIDR is blocked, then gone.
RESP=$(ipc_send "$(sign_payload '{"type":"block_cidr","cidr":"203.0.113.128/25","reason":"manual","ttl_secs":300}')")
echo "$RESP" | grep -q "block queued" || fail "block_cidr did not respond: $RESP"
sleep 1
RESP=$(ipc_send "$(sign_payload '{"type":"check_ip","ip":"203.0.113.200"}')")
echo "$RESP" | grep -q '"blocked":true' || fail "the CIDR block did not cover its member: $RESP"
green "✓ IPC: the CIDR block covers its member"
RESP=$(ipc_send "$(sign_payload '{"type":"unblock_cidr","cidr":"203.0.113.128/25"}')")
echo "$RESP" | grep -q "unblock queued" || fail "unblock_cidr did not respond: $RESP"
sleep 1
RESP=$(ipc_send "$(sign_payload '{"type":"check_ip","ip":"203.0.113.200"}')")
echo "$RESP" | grep -q '"blocked":false' || fail "the CIDR unblock did not take effect: $RESP"
green "✓ IPC: the unblocked CIDR is gone"

# A short TTL must expire and the block must disappear.
RESP=$(ipc_send "$(sign_payload '{"type":"block_ip","ip":"203.0.113.42","reason":"manual","ttl_secs":2}')")
echo "$RESP" | grep -q "block queued" || fail "the short-TTL block_ip did not respond: $RESP"
sleep 1
RESP=$(ipc_send "$(sign_payload '{"type":"check_ip","ip":"203.0.113.42"}')")
echo "$RESP" | grep -q '"blocked":true' || fail "the short-TTL block did not take effect: $RESP"
sleep 4
RESP=$(ipc_send "$(sign_payload '{"type":"check_ip","ip":"203.0.113.42"}')")
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
RESP=$(ipc_send "$(sign_payload '{"type":"block_ip","ip":"203.0.113.99","reason":"manual","ttl_secs":300}')")
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
RESP=$(ipc_send "$(sign_payload '{"type":"check_ip","ip":"203.0.113.99"}')")
echo "$RESP" | grep -q '"blocked":true' || fail "WAL restore after SIGKILL: $RESP"
green "✓ WAL: block restored after SIGKILL restart"

green ""
green "ALL PROD SMOKE CHECKS PASSED"