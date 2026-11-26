# Boundaries

RamShield is a self-hosted, single-node L3/L4 ingress defense. It is not a cloud scrubbing service and not a WAF.

## In scope

- Volumetric and behavioral detection for TCP/UDP/ICMP floods on one host.
- eBPF/XDP enforcement at the kernel ingress path.
- Local WAL/checkpoint durability and recovery.
- No external feed or cloud account required.

## Out of scope

- HTTP/2, L7 routing, bot management, WAF rules.
- Anycast, BGP FlowSpec/RTBH, upstream scrubbing.
- Attacks that saturate the ISP uplink before the host.
- Multi-node mesh as a requirement for one-host defense.
- A community threat-intelligence feed.

## Evidence boundary

The attack demo is a userspace simulation. Public numbers are historical single-run E1 unless raw data and repeated sessions are published. "Promoted" is the correct demo verb; "blocked" is reserved for the good-IP observation sentence.

## Failure boundary

Failed qualification rows are part of the product claim. Do not publish a success number without the matching failure in `docs/BENCHMARKS.md`.