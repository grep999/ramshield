# Demo transcript (ASCII)

```text
RamShield — Live Attack Simulation Demo
Detects and promotes abusive sources at the XDP/eBPF layer.

Phase 1: 10 IPv6 x 1 event
  processed: 10
  promoted: 0

Phase 2: 60 x 50 = 3000 events
  processed: 3000
  promoted: 60
  subnets: 1

Phase 3: 1 x 10000 events
  processed: 10000
  promoted: 1
```

This is a userspace simulation. It is not a wire-rate benchmark. See `docs/REPRODUCE.md`.
