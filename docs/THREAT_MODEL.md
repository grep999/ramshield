# Threat model

## Asset
The host's ingress path and its local enforcement state.

## Attacker
Remote actor able to send L3/L4 traffic. No shell access. No signed packets.

## Trust boundary
Packets arriving at the NIC. Every header field is untrusted. Lengths and offsets are checked before reads.

## Defenses
- XDP verdicts before socket allocation.
- EWMA + CUSUM + pulse + subnet correlation.
- WAL + checkpoint for durable state.
- Fail-open map update errors.

## Non-goals
Uplink saturation, L7 semantics, WAF rules, bot management, anycast, upstream BGP.

## Evidence
See `docs/REPRODUCE.md`. Demo is userspace simulation; numbers are E1 until raw repeated data exists.
