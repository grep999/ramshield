# ramshield-xdp

## Problem

Userspace IP blocking (writing to a store, checking on every connection) has overhead: syscall per check, context switches, memory copies. For high-traffic servers handling millions of packets per second, this overhead adds up. XDP (eXpress Data Path) runs a BPF program directly in the NIC driver, before packets enter the kernel's network stack. A blocked IP's packets are dropped at the driver level — zero syscall overhead, zero context switches, zero memory copies.

## How it works

This crate is a **build artifact** — it compiles a C eBPF program and exports it as a static byte slice. It contains zero runtime logic.

### Build time

`build.rs` compiles `src/xdp_shield.bpf.c` using `aya-ebpf` and `bpf-linker`:

```rust
// build.rs output:
pub static BPF_ELF: &[u8] = aya::include_bytes_aligned!("ramshield-xdp");
```

### Runtime (consumed by ramshield-enforcement)

The `ramshield-enforcement` crate's `AyaXdpApplier` loads this byte slice:

```rust
let mut bpf = aya::Bpf::load(ramshield_xdp::BPF_ELF)?;
// Attach to NIC, update BLOCKED_IPS map
```

### BPF program behavior

```
NIC receives packet
    → XDP program inspects source IP
    → Lookup in BLOCKED_IPS (v4) or BLOCKLIST6 (v6) hashmap
    → Match: XDP_DROP (130) — packet discarded, ~200 cycles
    → No match: XDP_PASS (2) — packet enters normal stack
```

Two separate BPF maps for v4/v6 prevent variable-width key issues in the kernel verifier.

## Dependencies

```
ramshield-xdp
  ← aya (include_bytes_aligned! macro)
  ← aya-ebpf, bpf-linker (build-time only)

Used by:
  → ramshield-enforcement::AyaXdpApplier (loads BPF_ELF)
```

The `elf` feature gates compilation. Without it, the crate is empty — useful for development on machines without BPF toolchain.

## Requirements

- Linux kernel ≥ 5.15
- `bpf-linker` in PATH
- Capabilities: `cap_net_admin`, `cap_bpf`, `cap_perfmon`

Without capabilities, enforcement falls back to Store-only blocking.
