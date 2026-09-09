# ramshield-xdp

eBPF/XDP program for kernel-level IP blocking. This crate compiles and loads an XDP program that drops packets from blocked IPs directly in the kernel's network stack — before they reach userspace. This is the fastest possible blocking mechanism: zero syscall overhead, zero context switches, zero memory copies for dropped packets.

## Architecture

```
User space (Rust)                        Kernel space (eBPF)
    │                                        │
    ├─ aya::Bpf::load()                      │
    ├─ Map updates (blocked IPs)  ────────►  XDP program
    │   BPF_HASHmap: ip → 1                  │
    │                                        ├─ XDP_DROP (130) for blocked IPs
    └─ Program attach (skb/drv)              └─ XDP_PASS (2) for allowlist
```

## XDP program

The eBPF program is defined in `src/xdp_shield.bpf.c` (C) and compiled to ELF by the build script. It's loaded at runtime via the `aya` framework.

### Entry point

```c
SEC("xdp")
int xdp_shield(struct xdp_md *ctx) {
    // Parse Ethernet → IPv4/IPv6 header
    // Extract source IP
    // Lookup in BLOCKED_IPS map
    // If found: return XDP_DROP
    // Otherwise: return XDP_PASS
}
```

The program is minimal — no parsing of transport headers, no connection tracking, no state. It does exactly one thing: check the source IP against a hash map and drop if matched. This keeps the instruction count low (verifier-friendly) and the per-packet cost minimal.

### BPF maps

| Map | Type | Key | Value | Purpose |
|-----|------|-----|-------|---------|
| `BLOCKED_IPS` | Hashmap | u32 (v4) or u128 (v6) | u64 (flags) | IPs to block |
| `BLOCKLIST6` | Hashmap | u128 (v6) | u64 (flags) | IPv6 blocked IPs (dedicated map for 128-bit keys) |

Separate maps for v4 and v6 prevent the verifier from complaining about variable-width key lookups. The v4 map uses packed u32 keys; the v6 map uses full u128 keys.

### Verifier compliance

The XDP program must pass the kernel's BPF verifier. Key constraints:
- No unbounded loops.
- All memory accesses must be provably within packet bounds.
- No calls to unavailable helper functions.
- Instruction count under the limit (typically 1M for modern kernels).

The build script (`build.rs`) compiles with `aya-ebpf` and `bpf-linker`, producing an ELF that the verifier accepts on kernels ≥ 5.15.

## Build system

```rust
// build.rs
fn main() {
    // Compiles src/xdp_shield.bpf.c → target/bpf/xdp_shield.bpf.o
    // Requires bpf-linker in PATH
    // Output: ELF binary loaded at runtime via aya::Bpf::load_file()
}
```

Build dependencies:
- `aya-ebpf` (eBPF program framework)
- `bpf-linker` (LLVM-based BPF linker)
- `aya` (user-space loader, runtime dependency)

The `elf` feature on `ramshield-xdp` enables build-time compilation. Without it, the crate is a no-op (useful for development on machines without BPF toolchain).

## User-space loader

```rust
pub struct XdpProgram { ... }
impl XdpProgram {
    pub fn new(interface: &str, mode: XdpMode) -> Result<Self>
    pub fn block_v4(&self, ip: Ipv4Addr) -> Result<()>
    pub fn unblock_v4(&self, ip: Ipv4Addr) -> Result<()>
    pub fn block_v6(&self, ip: Ipv6Addr) -> Result<()>
    pub fn unblock_v6(&self, ip: Ipv6Addr) -> Result<()>
    pub fn detach(&self) -> Result<()>
}
```

`new()` loads the compiled ELF, attaches to the specified network interface in the given mode, and returns a handle for map updates. `block_v4()` / `unblock_v4()` insert/remove entries from the `BLOCKED_IPS` hashmap. `detach()` unlinks the program from the interface.

## XDP modes

| Mode | Flag | Performance | Compatibility |
|------|------|-------------|---------------|
| `Skb` | `XDP_FLAGS_SKB_MODE` | Good (generic) | All NICs |
| `Drv` | `XDP_FLAGS_DRV_MODE` | Better | NICs with native XDP driver support |
| `Offload` | `XDP_FLAGS_HW_MODE` | Best | SmartNICs with XDP offload |

`Skb` mode works everywhere but processes packets through the generic network stack (slower). `Drv` mode bypasses the stack entirely for supported NICs. `Offload` pushes the program to the NIC hardware — fastest, but requires specific NIC firmware.

The enforcement engine falls back to `Skb` if the preferred mode fails. If all modes fail (e.g., no BPF permissions), XDP is disabled gracefully — Store-only blocking continues without kernel-level drops.

## Capabilities required

```bash
# Grant capabilities to the ramshield binary
sudo setcap 'cap_net_admin,cap_bpf,cap_perfmon+eip' ./target/release/ramshield
```

- `cap_net_admin`: attach XDP program to a network interface.
- `cap_bpf`: load eBPF programs into the kernel.
- `cap_perfmon`: access perf events (required by aya for XDP).

Without these, the XDP program cannot be loaded. The enforcement engine detects this and disables XDP without crashing.

## Packet flow

```
NIC receives packet
    → DMA to ring buffer
    → XDP program runs (before skb allocation)
    → Source IP lookup in BLOCKED_IPS
    → Match: XDP_DROP (packet discarded, ~200 cycles)
    → No match: XDP_PASS (packet enters normal stack)
```

The entire XDP path adds approximately 200 CPU cycles per packet for a hash lookup. At 1M packets/second, this is 0.2ms of CPU time — negligible compared to the userspace detection pipeline.

## Dependencies

`aya` (runtime), `aya-ebpf` (build-time), `bpf-linker` (build-time). The `elf` feature gates compilation; without it, the crate is empty.

## Tests

- Map insert/remove: verify BPF hashmap operations from user space.
- Verifier compliance: the compiled ELF passes `bpftool prog load` verification.
- Mode fallback: Skb → Drv → Offload cascade on unsupported NICs.
- Graceful degradation: XDP disabled when capabilities missing, Store-only blocking continues.
