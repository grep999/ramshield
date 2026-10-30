#!/usr/bin/env bash
# native_capture_netns.sh — run the live TPACKET_V3 capture integration test.
#
# Usage: ./scripts/native_capture_netns.sh
#
# Builds the test binary unprivileged, then runs it as root (needs CAP_NET_ADMIN
# + CAP_NET_RAW) so it can create a network namespace and veth pair.
set -euo pipefail

cd "$(dirname "$0")/.."

# Build the test binary as the calling user.
cargo test --test native_capture_live --no-run 2>/dev/null | grep -q 'test ' && true

# Find the compiled test binary path.
BIN=$(cargo test --test native_capture_live --no-run --message-format=json 2>/dev/null \
  | python3 -c '
import sys, json
for line in sys.stdin:
    try:
        m = json.loads(line)
    except:
        continue
    if m.get("profile", {}).get("test") and m.get("target", {}).get("name") == "native_capture_live":
        print(m["executable"])
        break
')
if [ -z "$BIN" ]; then
    echo "ERROR: could not locate native_capture_live test binary" >&2
    exit 1
fi

echo "[INFO] running $BIN as root (needs CAP_NET_ADMIN + CAP_NET_RAW)"
sudo "$BIN" --ignored --nocapture