# ramshield-xdp

eBPF/XDP program for kernel-level IP blocking.

## Architecture

```
User space (Rust)                    Kernel space (eBPF)
    │                                      │
    ├─ aya::Bpf::load()                    │
    ├─ Map updates (blocked IPs)  ──────►  XDP program
    └─ Program attach (skb/xdp)            │
                                      XDP_DROP on blocked IPs
                                      XDP_PASS on allowlist
```

## Key Components

### XDP Program
- `xdp_shield` entry point: checks src IP against blocked map
- Returns XDP_DROP (130) for blocked IPs, XDP_PASS (2) for others
- Map: `BLOCKED_IPS` — Hashmap for O(1) lookup

### Build
- `build.rs`: compiles eBPF program with `aya-ebpf` and `bpf-linker`
- Requires `bpf-linker` in PATH
- Output: ELF binary loaded at runtime via aya

### Modes
- `XdpMode::Skb` (generic): works on all NICs, uses socket buffer
- `XdpMode::Drv`: native NIC driver support, higher performance
- `XdpMode::Offload`: NIC hardware offload, fastest but limited support

### Capabilities required
- `cap_net_admin`: attach XDP program
- `cap_bpf`: load eBPF programs
- `cap_perfmon`: access perf events (for XDP)
- Set via: `sudo setcap 'cap_net_admin,cap_bpf,cap_perfmon+eip' ./target/release/ramshield`
