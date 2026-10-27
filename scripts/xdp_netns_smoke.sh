#!/usr/bin/env bash
# Month 5/6 gate: netns XDP smoke. Requires CAP_NET_ADMIN/CAP_BPF (sudo).
# End-to-end kernel enforcement:
#   1. veth pair + namespace; ramshield with XDP (skb) on the host veth.
#   2. Telemetry over the real IPC wire blocks the namespace IP.
#   3. Assert the block reconciled into the XDP blocklist map.
#   4. Assert the KERNEL actually drops the blocked IP's packets: before the
#      block a connect from the ns fails fast (ECONNREFUSED); after the block
#      the same connect times out because XDP discards the SYN.
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
NS_IP=10.200.0.2
WORK=$(mktemp -d)
trap 'ip netns del "$NS" 2>/dev/null || true; ip link del "$HOST_VETH" 2>/dev/null || true; kill %1 2>/dev/null || true; rm -rf "$WORK"' EXIT

log "creating netns $NS + veth pair"
ip netns add "$NS"
ip link add "$HOST_VETH" type veth peer name "$NS_VETH"
ip link set "$NS_VETH" netns "$NS"
ip addr add 10.200.0.1/30 dev "$HOST_VETH"
ip link set "$HOST_VETH" up
ip netns exec "$NS" ip addr add "${NS_IP}/30" dev "$NS_VETH"
ip netns exec "$NS" ip link set "$NS_VETH" up
ip netns exec "$NS" ip link set lo up

log "writing smoke config (xdp skb on $HOST_VETH, IPC telemetry feed)"
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

log "starting ramshield"
"$BIN" --config "$WORK/config.toml" >"$WORK/ramshield.log" 2>&1 &
PID=$!
sleep 3
kill -0 "$PID" 2>/dev/null || { cat "$WORK/ramshield.log"; die "ramshield did not stay up"; }

if ! ip -d link show "$HOST_VETH" 2>&1 | grep -qiE "prog/xdp"; then
  echo "=== ip -d link ==="; ip -d link show "$HOST_VETH" 2>&1
  echo "=== engine log (tail) ==="; tail -30 "$WORK/ramshield.log"
  die "no XDP program attached"
fi
log "XDP program attached"

# Baseline kernel behavior: connect must fail FAST (host RST, no listener).
base_ms=$(ip netns exec "$NS" bash -c '
  s=$(date +%s%N)
  timeout 3 bash -c "echo > /dev/tcp/10.200.0.1/9" 2>/dev/null
  e=$(date +%s%N); echo $(( (e - s) / 1000000 ))')
echo "baseline connect: ${base_ms}ms (expect fast refuse)"
[ "$base_ms" -lt 1000 ] || die "baseline connect took ${base_ms}ms — expected a fast RST"

# #90 under-capture probe: serial flood; kernel-side counters tell whether
# the AF_PACKET taps even received the SYNs.
FAILS=0
for round in $(seq 1 10); do
  F=$(ip netns exec "$NS" bash -c 'f=0; for i in $(seq 1 300); do echo > /dev/tcp/10.200.0.1/9 2>/dev/null || f=$((f+1)); done; echo $f')
  FAILS=$((FAILS + F))
done
echo "flood connect-failures (ECONNREFUSED expected): $FAILS"
echo "=== ip -s link $HOST_VETH ==="
ip -s link show "$HOST_VETH" | sed -n '1,8p'
echo "=== /proc/net/packet (per-socket: sk, received, dropped) ==="
cat /proc/net/packet

# #90 discriminator: an independent AF_PACKET tap (plain recv, no ring) on
# the same veth during a second flood. ~3000 => kernel tap delivery is fine
# and the loss is in the RX ring or the walk; ~60 => the tap itself is
# starved (fanout/kernel config).
echo "=== independent AF_PACKET recv tap (2s during flood) ==="
python3 - > "$WORK/tap_count.txt" 2>&1 <<'PY' &
import socket, struct, sys, time
s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(3))
s.bind(("rs-smoke-h", 0))
s.settimeout(0.5)
end = time.time() + 2.0
n = 0
while time.time() < end:
    try:
        s.recvfrom(65535)
        n += 1
    except socket.timeout:
        pass
    except OSError:
        break
print(n)
PY
TAP_PID=$!
ip netns exec "$NS" bash -c 'for i in $(seq 1 300); do echo > /dev/tcp/10.200.0.1/9 2>/dev/null || true; done' || true
wait $TAP_PID || true
echo "independent tap packets in 2s: $(cat "$WORK/tap_count.txt" | tail -1)"

# Cross-check with tcpdump (battle-tested AF_PACKET tap). If tcpdump sees the
# flood but our sockets don't, the defect is in our socket configuration
# (fanout/ignore_outgoing/ring). If tcpdump also sees ~0, it is kernel-side.
command -v tcpdump >/dev/null 2>&1 || (apt-get update -qq && apt-get install -y -qq tcpdump) || true
timeout 4 tcpdump -i "$HOST_VETH" -c 100 -nn 2>&1 | tail -3 || true

# Block the namespace IP via the real IPC wire (proxy/sidecar deployment path).
python3 - "$NS_IP" > "$WORK/frame.jsonl" <<'PY'
import json, sys
ip = sys.argv[1]
print(json.dumps({"type": "report_connections",
                  "events": [{"ip": ip, "bytes": 512, "status_code": 404,
                              "proto_fp": 42} for _ in range(200)]}))
PY
for i in 1 2 3; do
  timeout 5 bash -c 'exec 3<>/dev/tcp/127.0.0.1/17899 && cat '"$WORK"'/frame.jsonl >&3 && head -1 <&3' || true
done
sleep 2

METRICS=$(curl -fsS http://127.0.0.1:17999/metrics 2>/dev/null)
[ -n "$METRICS" ] || { cat "$WORK/ramshield.log"; die "metrics endpoint unreachable"; }
DET=$(printf '%s\n' "$METRICS" | awk '/^ramshield_blocks_detection/ {print $2}')
log "ramshield_blocks_detection=$DET"
printf '%s\n' "$METRICS" | grep -E "native_frames_walked|native_socket_packets|native_socket_drops|native_parse_rejects|native_ingest_emitted|native_ingest_drops_total|events_ingested_total"
awk -v b="$DET" 'BEGIN { exit (b+0 > 0) ? 0 : 1 }' || { tail -30 "$WORK/ramshield.log"; die "expected blocks_detection > 0"; }

# Kernel drop: the same connect must now time out (XDP discards the SYN)
# instead of failing fast with RST.
drop_ms=$(ip netns exec "$NS" bash -c '
  s=$(date +%s%N)
  timeout 3 bash -c "echo > /dev/tcp/10.200.0.1/9" 2>/dev/null
  e=$(date +%s%N); echo $(( (e - s) / 1000000 ))')
echo "post-block connect: ${drop_ms}ms (expect >=1000ms: SYN silently dropped)"
[ "$drop_ms" -ge 1000 ] || die "connect still failed in ${drop_ms}ms — XDP map is not dropping the blocked IP"

kill "$PID" 2>/dev/null || true
log "netns XDP smoke: PASS (attach, telemetry block, XDP map reconcile, kernel drop)"
