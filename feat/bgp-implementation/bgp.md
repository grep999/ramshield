# RamShield BGP Implementation Design

## Overview
RamShield implements a minimal, safe BGP-4 speaker for inter-domain signaling of attack mitigations. The design follows orthodox BGP primitives (RTBH + Flowspec) that every Tier-1 carrier natively supports in hardware, avoiding router crashes from complex TCAM pushes.

The BGP engine is intentionally minimal — it only sends pre-formatted byte buffers when triggered. It does not import routes, calculate routing tables, or touch the kernel FIB. It maintains a single TCP socket and fires UPDATE messages when the Schmitt trigger state machine permits.

## Two Orthodox BGP Primitives

### Primitive A: Community RTBH (RFC 7999)
- Announce a `/32` (or `/128`) host route with standard BGP communities
- Every carrier handles RTBH in their hardware FIB
- Zero TCAM constraints, zero router software CPU punting, 100% reliable
- Communities:
  - RFC 7999: `65535:666` (universal blackhole)
  - RFC 1997: `65535:65281` (`NO_EXPORT` guarantees blackhole never leaks beyond immediate transit)
  - Carrier-specific fallbacks: Cogent `174:666`, Lumen `3356:9999`, HE `6939:666`

### Primitive B: Surgical L4 Flowspec (RFC 8955)
- Tell upstream: "Drop UDP packets where destination is my IP AND source port is 53 (DNS), 123 (NTP), 389 (CLDAP), 1900 (SSDP)"
- 80% of volumetric pipe-saturating DDoS attacks are UDP reflection floods
- Victim host remains 100% online — only the reflection protocol is dropped at the transit edge

## The Minimalist BGP Engine (`ramshield-bgp`)

### Option 1: Native Micro-BGP Speaker (Recommended)
A minimal eBGP client in Rust connecting to upstream router via TCP port 179. Implements exactly 4 message types:

1. **OPEN** — Negotiates peering with upstream ASN
2. **KEEPALIVE** — Sent every 30 seconds to keep session up
3. **UPDATE** — Sends single RTBH `/32` or Flowspec rule
4. **NOTIFICATION** — Handles clean shutdown or error logs

**Does NOT:**
- Import routes
- Calculate routing tables
- Touch kernel FIB

**Maintains:**
- Single TCP socket
- Pre-formatted byte buffers fired when triggered

### Option 2: BIRD / ExaBGP Pipe (Fastest to ship)
If BIRD or ExaBGP already runs on the host, RamShield writes a single line to a FIFO pipe or Unix Domain Socket:

```bash
# How RamShield triggers ExaBGP via standard pipe:
echo "announce route 198.51.100.42/32 community [65535:666 65535:65281]" > /run/exabgp.cmd
```

## 3-Tier Autonomous Escalation Ladder

| Tier | Trigger Condition | Action |
|------|-------------------|--------|
| Tier 1 (Local) | Interface RX < 85% AND NIC drops == 0 | Pure Local XDP Drop. Kernel drops packets in nanoseconds. Zero BGP announcements. Zero upstream disruption. |
| Tier 2 (Proto) | Interface RX >= 85% AND Matches UDP reflection (DNS, NTP, CLDAP, SSDP) | Fire Upstream BGP Flowspec. Drops only amplification vector. Victim host IP remains 100% online. |
| Tier 3 (RTBH) | Interface RX >= 95% OR NIC drop rate > 25k pps OR Direct volumetric flood | Nuclear Button: Fire Upstream RTBH. Announces victim /32 with blackhole community. Sacrifices single IP to save /24 and entire rack link. |

## Anti-Flap Protection: The 15-Minute Hold Rule

The biggest mistake with BGP automation is withdrawing the route the second attack traffic disappears. When you null-route an IP upstream, link utilization drops to 0%. If you immediately withdraw, attack traffic rushes back, link chokes, you re-announce, and upstream transit router flap dampening (RFC 2439) penalizes your network for an hour.

**Standard carrier hold timer:**
- [Trigger BGP Block] → Hold route for 15 MINUTES minimum (No matter what!)
- Attack still active? → Extend hold by another 15m
- Traffic clear after 15m? → Withdraw BGP announcement cleanly

- Minimum Blackhole Lease: 15 minutes (900 seconds)
- Flap Penalty: Same IP attacked again within 1 hour → automatically increase hold to 30 minutes
- Keeps upstream transit providers happy, avoids route flap penalties, gives human operators time to investigate

## Configuration Interface (`config.toml`)

```toml
[upstream.monitor]
enabled = true
interface = "eth0"
link_capacity_mbps = 1000       # 1 Gbps link
saturation_threshold_pct = 85   # Trigger Tier 2/3 at 850 Mbps
hold_time_seconds = 900         # 15-minute minimum hold

[upstream.bgp]
enabled = true
local_asn = 65001
peer_asn = 174                  # Upstream Transit (e.g. Cogent)
peer_ip = "198.51.100.1"
router_id = "198.51.100.2"

# Standard Community Configurations
[upstream.bgp.communities]
blackhole = "65535:666"        # RFC 7999 universal blackhole
no_export = true                # Always add RFC 1997 NO_EXPORT (65535:65281)
custom = ["174:666"]            # Provider-specific blackhole

# Flowspec Rules to attempt BEFORE pulling the /32 blackhole trigger
[upstream.bgp.flowspec]
enabled = true
auto_filter_udp_reflection = true
reflection_ports = [53, 123, 389, 1900]
```

## Real-World Competitive Advantage

| Feature | Enterprise Appliances ($100k+ Arbor/FastNetMon) | RamShield + Orthodox BGP |
|---------|-----------------------------------------------|--------------------------|
| Telemetry Ingest | 1:1,000 Sampled NetFlow/sFlow | 1:1 Full Line-Rate Kernel (XDP/eBPF) |
| Detection Time | 15–45 seconds (waiting for flow buffers) | < 500 milliseconds (direct counter breach) |
| BGP Mitigation Time | 20–60 seconds | < 1.5 seconds (local socket write) |
| Failure Mode | Complex multi-rule TCAM push can crash router sessions | Simple single-rule RTBH + strict L4 Flowspec |
| Dependency Footprint | Heavy database + external collector instances | Single Rust binary or tiny Unix socket to BIRD |

**Bottom Line:** You don't need complex graph algorithms to beat enterprise DDoS tools. By combining in-kernel XDP telemetry (detecting link saturation in milliseconds) with standard, orthodox BGP Blackholing (RFC 7999) and UDP Flowspec (RFC 8955), RamShield stops link-saturating attacks upstream before enterprise collectors even finish processing their first NetFlow sample.

## Implementation Checklist

- [ ] Implement `ramshield-bgp` crate with native micro-BGP speaker (TCP/179)
- [ ] Flowspec NLRI encoder for RFC 8955 (IPv4) and RFC 8956 (IPv6)
- [ ] RTBH announcement with community attachment (RFC 7999)
- [ ] Schmitt trigger state machine (`ramshield-controller`) governing Flowspec assertion/withdrawal
- [ ] Config `config.toml` with monitor + BGP sections
- [ ] `ramshield-cli bgp status` command showing session state
- [ ] `ramshield-cli bgp test-announce <ip>` for manual trigger
- [ ] Option 2: FIFO pipe integration for ExaBGP/BIRD deployments