#!/usr/bin/env bash
# Runtime proof that the XDP program drops IPv6 at the kernel edge (IPv6
# plan Task 4 verification — the one thing unit tests cannot check without
# root). Also proves the G4 byte contract end-to-end: the key inserted here
# is built from the wire octets in order; a wrong endianness (the
# from_be_bytes bug class) makes the lookup miss and the blocked ping
# ANSWER — the script then fails. No root daemon, no HMAC socket: it
# attaches the same clang-built object RamShield embeds, to `lo` by default.
#
# Usage: sudo ./scripts/verify_v6_drop.sh [iface]   (iface=lo for a
# self-contained run; name a real NIC to verify the driver path too)
set -euo pipefail
IFACE="${1:-lo}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ELF=/tmp/ramshield-xdp-verify.o
BLOCKED=2001:db8::7
CONTROL=2001:db8::9
DST6=::1
fails=0

# --- build the exact object the release binary embeds -------------------
clang -O2 -g -target bpf -c "$ROOT/crates/ramshield-xdp/ramshield-xdp-bpf/src/main.c" -o "$ELF"
llvm-readelf --syms "$ELF" 2>/dev/null | grep -q BLOCKLIST6 || {
  echo "FAIL: BLOCKLIST6 map missing from object"; exit 1; }

cleanup() {
  ip -6 addr del "$BLOCKED/128" dev "$IFACE" 2>/dev/null || true
  ip -6 addr del "$CONTROL/128" dev "$IFACE" 2>/dev/null || true
  ip link set dev "$IFACE" xdp off 2>/dev/null || true
  ip link set dev "$IFACE" xdpgeneric off 2>/dev/null || true
}
trap cleanup EXIT

# --- attach (driver mode if the NIC takes it, generic otherwise) --------
if ! ip link set dev "$IFACE" xdp obj "$ELF" sec xdp 2>/dev/null; then
  ip link set dev "$IFACE" xdpgeneric obj "$ELF" sec xdp
  echo "attached: xdpgeneric on $IFACE"
else
  echo "attached: native xdp on $IFACE"
fi
bpftool map list name BLOCKLIST6 >/dev/null || { echo "FAIL: no BLOCKLIST6 in kernel"; exit 1; }

ip -6 addr add "$BLOCKED/128" dev "$IFACE" nodad
ip -6 addr add "$CONTROL/128" dev "$IFACE" nodad

ping_ok() { ping -6 -I "$1" "$DST6" -c1 -W2 >/dev/null 2>&1; }

# --- baseline: before any key, both sources answer -----------------------
ping_ok "$BLOCKED" || { echo "FAIL: baseline ping dead (test harness broken, not RamShield)"; exit 1; }
echo "PASS baseline: $BLOCKED answered before insert"

# --- insert BLOCKED with WIRE-ORDER key bytes (== BlocklistKey::from_ip) --
KEYHEX=$(python3 -c "import ipaddress,sys; print(' '.join(f'{b:02x}' for b in ipaddress.IPv6Address('$BLOCKED').packed))")
bpftool map update name BLOCKLIST6 key hex $KEYHEX value hex 01

# 1. blocked src must now be silently dropped
if ping_ok "$BLOCKED"; then
  echo "FAIL: $BLOCKED answered after BLOCKLIST6 insert — key bytes missed (endianness regression? mode not in datapath?)"; fails=1
else
  echo "PASS v6 DROP: blocked $BLOCKED silent (kernel matched wire-order key)"
fi

# 2. control src must still pass (program did not nuke all v6)
if ping_ok "$CONTROL"; then
  echo "PASS v6 PASS: control $CONTROL answered"
else
  echo "FAIL: control also dropped — program kills all IPv6, not just keys"; fails=1
fi

# 3. v4 leg sanity — same program, other map, endianness the other way.
ip addr add 192.0.2.7/32 dev "$IFACE" 2>/dev/null || true
V4KEY=$(python3 -c "import ipaddress,sys; print(' '.join(f'{b:02x}' for b in ipaddress.IPv4Address('192.0.2.7').packed + b'\0'*12))")
bpftool map update name BLOCKLIST key hex $V4KEY value hex 01
if ping -4 -I 192.0.2.7 127.0.0.1 -c1 -W2 >/dev/null 2>&1; then
  echo "FAIL: v4 blocked src answered — BLOCKLIST key missed"; fails=1
else
  echo "PASS v4 DROP: blocked 192.0.2.7 silent"
fi
bpftool map delete name BLOCKLIST key hex $V4KEY  # C drops on key PRESENCE (value never read) — delete is the only un-block
if ping -4 -I 192.0.2.7 127.0.0.1 -c1 -W2 >/dev/null 2>&1; then
  echo "PASS v4 delete: key removal restores traffic (expiry path works)"
else
  echo "FAIL: v4 still dropped after key delete — stale-key bug (reconcile class)"; fails=1
fi
ip addr del 192.0.2.7/32 dev "$IFACE" 2>/dev/null || true

# 4. drop counter, informational (generic path may not increment it)
ip -s link show dev "$IFACE" | grep -A1 "RX:" | head -2

exit $fails
