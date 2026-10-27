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

[xdp]
enabled = true
interface = "$HOST_VETH"

[wal]
dir = "$WALD"

[ipc]
tcp_addr = "127.0.0.1:17899"

[dashboard]
http_addr = "127.0.0.1:17999"
EOF

log "starting ramshield (safe userspace + XDP attach)"
"$BIN" --config "$WORK/config.toml" >"$WORK/ramshield.log" 2>&1 &
PID=$!
sleep 3
kill -0 "$PID" 2>/dev/null || { cat "$WORK/ramshield.log"; die "ramshield did not stay up"; }

bpftool prog show 2>/dev/null | grep -qi ramshield || { cat "$WORK/ramshield.log"; die "no XDP program attached"; }
log "XDP program attached"

log "flooding from namespace (400 connection attempts)"
# No listener on purpose: refused connects still cross the veth (SYN each),
# which is what the XDP hook counts.
ip netns exec "$NS" sh -c 'for i in $(seq 1 400); do (echo > /dev/tcp/10.200.0.1/9) 2>/dev/null & done; wait' || true
sleep 2

BLOCKS=$(curl -fsS http://127.0.0.1:17999/metrics 2>/dev/null | awk '/^ramshield_blocks_total/ {print $2}')
[ -n "$BLOCKS" ] || { cat "$WORK/ramshield.log"; die "metrics endpoint unreachable"; }
log "ramshield_blocks_total=$BLOCKS"
awk -v b="$BLOCKS" 'BEGIN { exit (b+0 > 0) ? 0 : 1 }' || die "expected blocks_total > 0 after flood"

kill "$PID" 2>/dev/null || true
log "netns XDP smoke: PASS"
