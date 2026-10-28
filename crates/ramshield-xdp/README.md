# ramshield-xdp

## Why it exists

This crate is a build artifact container. It compiles the BPF program (Rust, in `ramshield-xdp-bpf/src/main.rs`, built with `aya-ebpf`) into an ELF binary at build time, then exports it as a static byte slice. The actual loading, attaching to a network interface, and map management happens in `ramshield-enforcement::xdp::AyaXdpApplier`.

This separation exists because BPF compilation needs `bpf-linker` and the `bpfel-unknown-none` target at build time — tools that aren't always available on the target machine. `build.rs` looks for `bpf-linker` on `PATH` and then in `~/.local/bin`; if it finds neither, the build **fails**: `build.rs` panics with the install step, and no placeholder ELF is written. The `xdp` feature flag controls whether this crate is included.

## What it does

```rust
pub static BPF_ELF: &[u8] = aya::include_bytes_aligned!(
    concat!(env!("OUT_DIR"), "/ramshield-xdp")
);
```

That's the entire public API. One static byte slice containing the compiled BPF ELF. `build.rs` runs a nested `cargo build --target=bpfel-unknown-none` on the BPF crate through `bpf-linker` and places the result in `OUT_DIR`. The `aya` crate's `include_bytes_aligned!` macro loads it at compile time with correct alignment for BPF map access.

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

The enforcement crate defines the BPF map key type (`BlocklistKey`) and writes the map values. There is no `BlocklistValue` type: the map value is a plain `u64` — the absolute expiry timestamp on the monotonic clock, with `u64::MAX` meaning permanent. This keeps the type definitions next to the code that writes to them — one source of truth, no drift between the BPF program and the userspace writer.

## BPF Program Behavior

The compiled kernel program runs on every incoming packet at the XDP hook point (before the kernel's network stack). It:

1. Extracts the source IP from the packet header.
2. Looks the source IP up in the flat blocklist map. The key is the raw
   wire bytes of the address copied into a 16-byte key — there is no
   hash, so the userspace writer and the kernel program agree on the
   key by construction.
3. If the map holds the address **and its expiry is still in the
   future**, drops the packet (returns `XDP_DROP`). An entry whose
   expiry has passed does not drop the packet, so an expired block stops
   blocking on its own.
4. Otherwise checks the CIDR maps (`BLOCKCIDR` / `BLOCKCIDR6`, LPM
   tries holding /24 and /64 entries). A match there drops the packet
   even when the address is not in the flat map.
5. If neither matched, passes it through (returns `XDP_PASS`).

This happens in kernel space — no context switch to userspace, no memory copies. For blocked IPs, the packet is dropped before any kernel processing (no socket allocation, no TCP state machine, no buffer allocation).

## Uniqueness

**Build artifact, not a runtime component.** This crate does nothing at runtime. It exists solely to isolate the BPF compilation step — a build-time concern — from the rest of the workspace. The 9 lines of Rust are a delivery mechanism for a binary artifact.

**Fail-open design.** If the BPF program fails to load (wrong kernel, missing permissions), `AyaXdpApplier::load_and_attach()` returns an error. The enforcement service catches this and falls back to in-band enforcement (Store-based blocking). The system degrades gracefully — slower but functional.

## Dependencies

**Reads from:** `aya` (BPF loading, `include_bytes_aligned!`).

**Written by:** nothing at runtime.

**Read by:** `ramshield-enforcement::xdp::AyaXdpApplier` (sole consumer).

## Benchmarks

XDP kernel-side packet processing benchmarks require a live XDP environment (root, eBPF-capable kernel) and are not run in CI. The userspace enforcement pipeline benchmarks below are measured on the host:

```text
  Detection Flush (Warm)     222400 ns/op  (4496 ops/s)
  Detection Flush (Cold Skip) 2606343 ns/op  (384 ops/s)
  Enforcement Commit (WAL)   3955 ns/op    (252873 ops/s)
  Idempotent Decision Cache   130 ns/op     (7702531 ops/s)
  Reverse-Proxy SHM Lookup    1.3 ns        (761 M ops/s)
  Ingested Event Throughput   95653 eps     (5 s, 1000 IPs)
```

Full benchmark suite (detection, storage, protocol, CGNAT, mesh) runs via `cargo bench` in the root workspace. See `benches/hot_paths.rs` and `benches/field_day.rs` for raw output.

## Testing

19 unit tests: the v4 and v6 key layouts (zero, broadcast, loopback and
all-ones addresses), the key octet order against the C `saddr` layout,
the split between v4 and v6 keys, the map routing round-trip, the wire
contract against the C key layout, the VLAN tag documentation, and the
separation of the BLOCKLIST and BLOCKLIST6 maps. Three more need root
and are `#[ignore]`d by default: loading the BPF program with `aya`, and
the insert/lookup round-trips against a live map. The enforcement crate's
integration tests then load the BPF program and verify block/unblock
behavior through the `AyaXdpApplier`.
