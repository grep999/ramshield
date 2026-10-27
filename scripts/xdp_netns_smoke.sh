#!/usr/bin/env bash
# Month 5/6 gate: netns XDP smoke. Requires CAP_NET_ADMIN/CAP_BPF (sudo).
# 1. veth pair + namespace, 2. ramshield with XDP on the host veth,
# 3. flood from the namespace, 4. assert the XDP program is attached and
# ramshield_blocks_total advanced.
# Usage: sudo scripts/xdp_netns_smoke.sh <path-to-ramshield-bin>
set -euo pipefail
log()  { printf '[INFO] %s\n' "$*"; }
die()  { printf '[ERROR] %s\n' "$*" >&2; exit 1; }

BIN="${1:-/usr/local/bin/ramshield}"
[ -x "$BIN" ] || die "usage: $0 <ramshield-binary>"
[ "$(id -u)" = 0 ] || die "must run as root (netns + XDP)"

NS=rs-smoke
HOST_VETH=rs-smoke-h
NS_VETH=rs-smoke-n
HOST_IP=10.200.0.1/30
NS_IP=10.200.0.2
WORK=$(mktemp -d)
trap 'ip netns del "$NS" 2>/dev/null || true; ip link del "$HOST_VETH" 2>/dev/null || true; kill %1 2>/dev/null || true; rm -rf "$WORK"' EXIT

log "creating netns $NS + veth pair"
ip netns add "$NS"
ip link add "$HOST_VETH" type veth peer name "$NS_VETH"
ip link set "$NS_VETH" netns "$NS"
ip addr add "$HOST_IP" dev "$HOST_VETH"
ip link set "$HOST_VETH" up
ip netns exec "$NS" ip addr add "${NS_IP}/30" dev "$NS_VETH"
ip netns exec "$NS" ip link set "$NS_VETH" up
ip netns exec "$NS" ip link set lo up

log "writing smoke config (xdp on $HOST_VETH, low thresholds)"
WALD="$WORK/wal"; mkdir -p "$WALD"
cat > "$WORK/config.toml" <<EOF
[engine]
worker_threads = 2
ram_limit_mb = 256
shard_count = 16

[detection]
rps_threshold = 50
rate_window_secs = 10
subnet_batch_threshold = 64
batch_block_enabled = true
block_ttl_secs = 60
bloom_bits = 100000
emergency_burst_threshold = 50

[xdp]
enabled = true
interface = "$HOST_VETH"
mode = "skb"
allow_inband_fallback = false

[wal]
enabled = true
dir = "$WALD"
durability = "None"
compress = false
seg_max_bytes = 67108864
retention_max_bytes = 1073741824
allow_volatile_fallback = true

[native_ingest]
enabled = true
interface = "$HOST_VETH"

[ipc]
tcp_addr = "127.0.0.1:17899"
max_connections = 64

[dashboard]
enabled = true
http_addr = "127.0.0.1:17999"
EOF

log "starting ramshield (safe userspace + XDP attach)"
"$BIN" --config "$WORK/config.toml" >"$WORK/ramshield.log" 2>&1 &
PID=$!
sleep 3
kill -0 "$PID" 2>/dev/null || { cat "$WORK/ramshield.log"; die "ramshield did not stay up"; }

if ! ip -d link show "$HOST_VETH" 2>&1 | grep -qiE "prog/xdp"; then
  echo "=== ip -d link ==="; ip -d link show "$HOST_VETH" 2>&1
  echo "=== engine log (xdp/attach lines) ==="; grep -iE "xdp|attach|active" "$WORK/ramshield.log" || true
  echo "=== engine log (tail) ==="; tail -20 "$WORK/ramshield.log"
  die "no XDP program attached"
fi
log "XDP program attached"

log "flooding from namespace (10 rounds x 300 serial connects, bash /dev/tcp)"
FAILS=0
for round in $(seq 1 10); do
  F=$(ip netns exec "$NS" bash -c 'f=0; for i in $(seq 1 300); do echo > /dev/tcp/10.200.0.1/9 2>/dev/null || f=$((f+1)); done; echo $f')
  FAILS=$((FAILS + F))
done
echo "flood connect-failures (ECONNREFUSED expected): $FAILS"
ip -s link show "$HOST_VETH" | sed -n '3,6p'
sleep 2

BLOCKS=$(curl -fsS http://127.0.0.1:17999/metrics 2>/dev/null)
[ -n "$BLOCKS" ] || { cat "$WORK/ramshield.log"; die "metrics endpoint unreachable"; }
DET=$(printf '%s\n' "$BLOCKS" | awk '/^ramshield_blocks_detection/ {print $2}')
log "ramshield_blocks_detection=$DET (blocks_total=$(printf '%s\n' "$BLOCKS" | awk '/^ramshield_blocks_total/ {print $2}'))"
printf '%s\n' "$BLOCKS" | grep -E "ingested|shed|native_ingest_drops|emergency|blocks" | head -12
awk -v b="$DET" 'BEGIN { exit (b+0 > 0) ? 0 : 1 }' || { tail -30 "$WORK/ramshield.log"; die "expected blocks_detection > 0 after flood"; }
# Kernel leg: the decision must reach the XDP blocklist map (reconcile logs it).
grep -q "XDP reconciled with [1-9]" "$WORK/ramshield.log" || { tail -20 "$WORK/ramshield.log"; die "block never reached the XDP map (no reconcile)"; }

kill "$PID" 2>/dev/null || true
log "netns XDP smoke: PASS"
