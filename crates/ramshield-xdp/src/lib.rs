//! RamShield eBPF / XDP data plane — build artifact crate.
//!
//! Sole export: the compiled BPF ELF from build.rs. All loading, attaching,
//! and map management lives in `ramshield-enforcement::xdp::AyaXdpApplier`
//! (the only consumer). Types (`BlocklistKey`/`BlocklistValue`) are defined
//! there too — one source of truth, no drift.

/// Compiled BPF ELF produced by this crate's build.rs (clang fallback path).
pub static BPF_ELF: &[u8] = aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/ramshield-xdp"));
