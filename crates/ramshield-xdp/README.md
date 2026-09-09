# ramshield-xdp

Build artifact crate that compiles and exports an eBPF/XDP program for kernel-level IP blocking. This crate contains zero runtime logic — it exists solely to compile the BPF ELF and make it available as a static byte slice for the enforcement crate to load at runtime.

## What this crate does

1. **Build time (`build.rs`):** Compiles `src/xdp_shield.bpf.c` (C) to a BPF ELF binary using `aya-ebpf` and `bpf-linker`. Output goes to `$OUT_DIR/ramshield-xdp`.
2. **Runtime:** Exports the compiled ELF as a static byte slice:
   ```rust
   pub static BPF_ELF: &[u8] = aya::include_bytes_aligned!(...);
   ```
3. **Consumer:** `ramshield-enforcement::xdp::AyaXdpApplier` loads this byte slice via `aya::Bpf::load()`, attaches to a network interface, and updates the `BLOCKED_IPS` / `BLOCKLIST6` BPF hashmaps.

## BPF program behavior

The compiled XDP program (`xdp_shield`) runs on every packet at the NIC driver level:

```
NIC receives packet
    → XDP program inspects source IP
    → Lookup in BLOCKED_IPS (v4) or BLOCKLIST6 (v6) hashmap
    → Match found: return XDP_DROP (130) — packet discarded
    → No match: return XDP_PASS (2) — packet enters normal stack
```

Two separate BPF maps prevent variable-width key issues in the verifier:
- `BLOCKED_IPS`: `BPF_HASH` with u32 keys (packed IPv4 /24)
- `BLOCKLIST6`: `BPF_HASH` with u128 keys (IPv6 /64)

## Build requirements

- `bpf-linker` in PATH (LLVM-based BPF linker)
- `clang` with BPF target support
- Linux kernel ≥ 5.15 (for BPF verifier compliance)

The `elf` feature on this crate gates compilation. Without it, the crate is empty — useful for development on machines without BPF toolchain.

## Feature flags

- `elf` (default): Enables `build.rs` and the `BPF_ELF` static. Without this, the crate is a no-op.
- `aya`: Pulls in `aya` for the `include_bytes_aligned!` macro.

## Capabilities required at runtime

```bash
sudo setcap 'cap_net_admin,cap_bpf,cap_perfmon+eip' ./target/release/ramshield
```

Without these, the enforcement crate detects the failure and disables XDP gracefully — Store-only blocking continues without kernel-level drops.

## Dependencies

`aya` (for `include_bytes_aligned!`), `aya-ebpf` (build-time), `bpf-linker` (build-time). No runtime dependencies beyond the ELF export.

## Tests

No runtime tests — the crate is a build artifact. The BPF program's correctness is verified by the enforcement crate's integration tests and by the kernel BPF verifier at load time.
