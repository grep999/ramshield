#!/usr/bin/env bash
# native_capture_netns.sh — run the live TPACKET_V3 capture integration test.
#
# Usage: ./scripts/native_capture_netns.sh
#
# Builds the test binary unprivileged, then runs it as root (needs CAP_NET_ADMIN
# + CAP_NET_RAW) so it can create a network namespace and veth pair.
set -euo pipefail

cd "$(dirname "$0")/.."

# Build once and extract the test executable from Cargo's JSON messages.
# Find the compiled test binary path.
BIN=$(cargo test --test native_capture_live --no-run --message-format=json \
  | python3 -c '
import sys, json
executable = None
for line in sys.stdin:
    try:
        m = json.loads(line)
    except:
        continue
    if m.get("reason") == "compiler-message":
        message = m.get("message", {})
        if message.get("level") == "error":
            sys.stderr.write(message.get("rendered") or (message.get("message", "") + "\\n"))
    if m.get("profile", {}).get("test") and m.get("target", {}).get("name") == "native_capture_live":
        executable = m["executable"]
if executable:
    print(executable)
else:
    sys.exit("Cargo did not report the native_capture_live test executable")
')
if [ -z "$BIN" ]; then
    echo "ERROR: could not locate native_capture_live test binary" >&2
    exit 1
fi

echo "[INFO] running $BIN as root (needs CAP_NET_ADMIN + CAP_NET_RAW)"
sudo env RUST_BACKTRACE=1 "$BIN" --ignored --nocapture