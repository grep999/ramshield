# ramshield-xdp

## Why it exists

This crate is a build artifact container. It compiles a BPF program (written in C or Rust) into an ELF binary at build time, then exports it as a static byte slice. The actual loading, attaching to a network interface, and map management happens in `ramshield-enforcement::xdp::AyaXdpApplier`.

This separation exists because BPF compilation requires `clang` and `llvm-strip` at build time — tools that aren't always available on the target machine. By isolating the compilation into its own crate, the rest of the workspace compiles cleanly even without BPF toolchain. The `xdp` feature flag controls whether this crate is included.

## What it does

```rust
pub static BPF_ELF: &[u8] = aya::include_bytes_aligned!(
    concat!(env!("OUT_DIR"), "/ramshield-xdp")
);
```

That's the entire public API. One static byte slice containing the compiled BPF ELF. `build.rs` compiles the BPF source, strips it, and places it in `OUT_DIR`. The `aya` crate's `include_bytes_aligned!` macro loads it at compile time with correct alignment for BPF map access.

## How it's consumed

`ramshield-enforcement` is the sole consumer:

```rust
// In ramshield-enforcement::xdp
pub struct AyaXdpApplier { ... }

impl AyaXdpApplier {
    pub fn load_and_attach(&mut self) -> Result<(), EnforcementError> {
        let mut bpf = aya::Bpf::load(ramshield_xdp::BPF_ELF)?;
        // ... attach to interface, get blocklist map
    }
}
```

The enforcement crate also defines the BPF map types (`BlocklistKey`, `BlocklistValue`) that the kernel program reads. This keeps the type definitions next to the code that writes to them — one source of truth, no drift between the BPF program and the userspace writer.

## BPF Program Behavior

The compiled kernel program runs on every incoming packet at the XDP hook point (before the kernel's network stack). It:

1. Extracts the source IP from the packet header.
2. Hashes it into the blocklist map.
3. If the IP is in the map, drops the packet (returns `XDP_DROP`).
4. If not, passes it through (returns `XDP_PASS`).

This happens in kernel space — no context switch to userspace, no memory copies. For blocked IPs, the packet is dropped before any kernel processing (no socket allocation, no TCP state machine, no buffer allocation).

## Uniqueness

**Build artifact, not a runtime component.** This crate does nothing at runtime. It exists solely to isolate the BPF compilation step — a build-time concern — from the rest of the workspace. The 9 lines of Rust are a delivery mechanism for a binary artifact.

**Fail-open design.** If the BPF program fails to load (wrong kernel, missing permissions), `AyaXdpApplier::load_and_attach()` returns an error. The enforcement service catches this and falls back to in-band enforcement (Store-based blocking). The system degrades gracefully — slower but functional.

## Dependencies

**Reads from:** `aya` (BPF loading, `include_bytes_aligned!`).

**Written by:** nothing at runtime.

**Read by:** `ramshield-enforcement::xdp::AyaXdpApplier` (sole consumer).

## Benchmarks

No benchmarks — this crate has no runtime code. Kernel-side packet processing benchmarks would require a live XDP environment and are planned for a future phase.

## Testing

No unit tests — the crate's correctness is verified by `ramshield-enforcement` integration tests that load the BPF program and verify block/unblock behavior through the `AyaXdpApplier`.
