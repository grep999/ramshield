#![no_std]
#![no_main]

// aya-ebpf path for RamShield XDP. Wire-compatible with shipping C main.c.
// Key types: [u64; 2] for both BLOCKLIST and BLOCKLIST6 (matching C's __u64[2]).
// VLAN stripping: up to 4 tags (802.1Q + 802.1ad), fail-open.
// Build: bpf-linker (prebuilt musl at ~/.local/bin/bpf-linker) + cargo build --target bpfel-unknown-none.

use aya_ebpf::bindings::xdp_md;
use aya_ebpf::{
    bindings::xdp_action,
    macros::{map, xdp},
    maps::{Array, lpm_trie::Key, LpmTrie, LruHashMap, PerCpuArray, RingBuf},
    programs::XdpContext,
};
use core::mem;
use network_types::{
    eth::{EthHdr, EtherType},
    ip::{Ipv4Hdr, Ipv6Hdr},
};

// 16-byte key matching C's __u64[2] layout.
// LRU_HASH: kernel auto-evicts coldest entry when full — no E2BIG under botnet flood.
// Key for LPM_CIDR is { prefixlen: u32, addr: [u64; 2] } — one /24 or /128 entry
// replaces 256 flat entries in BLOCKLIST.
#[map]
static BLOCKLIST: LruHashMap<[u64; 2], u64> = LruHashMap::with_max_entries(blocklist_cap_env(), 0);

#[map]
static BLOCKLIST6: LruHashMap<[u64; 2], u64> = LruHashMap::with_max_entries(blocklist_cap_env(), 0);

// TTL value format: absolute expiry ns (CLOCK_MONOTONIC, same clock as
// bpf_ktime_get_ns). u64::MAX = permanent. P0: full u64 — a packed u32
// expiry capped any block at 4.3s, and a 1-byte userspace value made the
// map type-check fail at open (InvalidValueSize) → XDP never blocked.
#[map]
static BLOCKCIDR: LpmTrie<[u64; 2], u8> = LpmTrie::with_max_entries(102_400, 0);

#[map]
static BLOCKCIDR6: LpmTrie<[u64; 2], u8> = LpmTrie::with_max_entries(102_400, 0);

// RingBuf for kernel→userspace drop events. 64KB default.
// Event: { ip: [u8; 16], ts_ns: u64, action: u8, slot: u8 }
#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(65536, 0);

// Per-CPU drop counters. One u64 slot per CPU; zero cache-line contention
// under peak flood. Slot index encodes the outcome (see CounterSlot below).
#[map]
static COUNTERS: PerCpuArray<u64> = PerCpuArray::with_max_entries(4, 0);

// Autonomous cold-start PPS guard config. Single-element Array of [u64; 4]:
// [syn_pps_per_cpu, udp_pps_per_cpu, packet_pps_per_cpu, window_ms].
// Map is present so userspace configure_autonomous succeeds when enabled.
// Kernel-side enforcement logic for the guard is not yet active.
#[map]
static AUTONOMOUS_CONFIG: Array<[u64; 4]> = Array::with_max_entries(1, 0);

// COUNTERS slot indices. Protocol-aware split so the "did XDP actually drop?"
// question is answerable without any userspace map scanning.
mod counter {
    pub const V4_DROP: u32 = 0;
    pub const V6_DROP: u32 = 1;
    pub const PASS: u32 = 2;
    pub const PARSE_FAIL: u32 = 3;
}

/// Bump a per-CPU counter. PerCpuArray slots are isolated per CPU so a plain
/// read-modify-write is safe; no atomics, no lock.
#[inline(always)]
fn inc_counter(slot: u32) {
    let ptr = COUNTERS.get_ptr_mut(slot);
    if let Some(p) = ptr {
        // SAFETY: `get_ptr_mut` returned a valid pointer to this CPU's slot,
        // and XDP runs non-preemptibly per CPU, so no other writer aliases it.
        unsafe { *p += 1 };
    }
}

fn emit_drop_event(slot: u32, ip: &[u8; 16]) {
    // SAFETY: bpf_ktime_get_ns takes no arguments and has no preconditions.
    let ts = unsafe { aya_ebpf::helpers::bpf_ktime_get_ns() };
    let mut buf = [0u8; 26];
    buf[0..16].copy_from_slice(ip);
    buf[16..24].copy_from_slice(&ts.to_ne_bytes());
    buf[24] = 0;
    buf[25] = (slot & 0xFF) as u8;
    let _ = EVENTS.output(&buf, 0);
}

// Wire-format VLAN ethertypes (native/LE representation of the on-wire values).
const ETH_P_8021Q: u16 = 0x8100_u16.to_be();
const ETH_P_8021AD: u16 = 0x88a8_u16.to_be();

#[inline(always)]
const fn blocklist_cap_env() -> u32 {
    match option_env!("BLOCKLIST_CAP") {
        Some(s) => {
            let bytes = s.as_bytes();
            let mut v: u32 = 0;
            let mut i = 0;
            while i < bytes.len() {
                let c = bytes[i];
                if c < b'0' || c > b'9' {
                    return 102_400;
                }
                v = v.saturating_mul(10).saturating_add((c - b'0') as u32);
                i += 1;
            }
            if v == 0 {
                102_400
            } else {
                v
            }
        }
        None => 102_400,
    }
}

#[inline(always)]
fn ptr_at<T>(ctx: &XdpContext, offset: usize) -> Result<*const T, ()> {
    let start = ctx.data();
    let end = ctx.data_end();
    let len = mem::size_of::<T>();
    if start + offset + len > end {
        return Err(());
    }
    Ok((start + offset) as *const T)
}

#[xdp]
pub fn ramshield_xdp(ctx: XdpContext) -> u32 {
    match try_ramshield_xdp(ctx) {
        Ok(action) => action,
        Err(()) => {
            inc_counter(counter::PARSE_FAIL);
            xdp_action::XDP_PASS
        }
    }
}

fn try_ramshield_xdp(ctx: XdpContext) -> Result<u32, ()> {
    let eth: *const EthHdr = ptr_at(&ctx, 0)?;
    // SAFETY: `ptr_at` verified the full EthHdr lies within packet bounds.
    let mut proto = unsafe { (*eth).ether_type } as u16;
    let mut l3_off = EthHdr::LEN;

    // Strip up to 4 VLAN tags (802.1Q / 802.1ad), matching C's loop.
    for _ in 0..4 {
        if proto != ETH_P_8021Q && proto != ETH_P_8021AD {
            break;
        }
        // ponytail: validate the VLAN header through packet-relative bounds;
        // comparing an integer offset directly with data_end lets the BPF
        // verifier lose the packet range and rejects the subsequent read.
        let vlan: *const [u8; 4] = ptr_at(&ctx, l3_off)?;
        // SAFETY: `ptr_at` verified all 4 VLAN header bytes are in bounds;
        // bytes 2..4 (the inner ethertype) are read unaligned.
        proto = unsafe { core::ptr::read_unaligned((vlan as *const u8).add(2) as *const u16) };
        l3_off += 4;
    }

    if proto == EtherType::Ipv6 as u16 {
        let ip6: *const Ipv6Hdr = ptr_at(&ctx, l3_off)?;
        let mut key = [0u64; 2];
        let mut key_bytes = [0u8; 16];
        // SAFETY: `ptr_at` verified the full Ipv6Hdr is in bounds, so its
        // 16-byte src_addr is readable; `key` and `key_bytes` are distinct
        // 16-byte locals, so the copies cannot overlap or overflow.
        unsafe {
            core::ptr::copy_nonoverlapping(
                (*ip6).src_addr.as_ptr(),
                key.as_mut_ptr() as *mut u8,
                16,
            );
            key_bytes.copy_from_slice(core::slice::from_raw_parts(
                (*ip6).src_addr.as_ptr(),
                16,
            ));
        }
        // SAFETY: the returned value is a plain u64 read once, immediately;
        // a concurrent userspace update can only yield a stale expiry.
        if let Some(v) = unsafe { BLOCKLIST6.get(&key) } {
            // SAFETY: bpf_ktime_get_ns takes no arguments and has no preconditions.
            let now = unsafe { aya_ebpf::helpers::bpf_ktime_get_ns() };
            if now < *v {
                inc_counter(counter::V6_DROP);
                emit_drop_event(counter::V6_DROP, &key_bytes);
                return Ok(xdp_action::XDP_DROP);
            }
        }
        // CIDR fallback: LPM_TRIE — one /64 entry replaces 256 flat entries in BLOCKLIST6
        if BLOCKCIDR6.get(&Key::new(128, key)).is_some() {
            inc_counter(counter::V6_DROP);
            emit_drop_event(counter::V6_DROP, &key_bytes);
            return Ok(xdp_action::XDP_DROP);
        }
        inc_counter(counter::PASS);
        return Ok(xdp_action::XDP_PASS);
    }

    if proto != EtherType::Ipv4 as u16 {
        inc_counter(counter::PASS);
        return Ok(xdp_action::XDP_PASS);
    }

    let ip: *const Ipv4Hdr = ptr_at(&ctx, l3_off)?;
    // Zero-conversion raw byte copy: wire octets → key memory directly.
    // No from_be_bytes / from_ne_bytes / integer casts — eliminates
    // the endianness inversion that made every v4 lookup miss on LE.
    // SAFETY: `ptr_at` verified the full Ipv4Hdr is in bounds.
    let src_bytes = unsafe { (*ip).src_addr }; // [u8; 4] raw wire order
    let mut key = [0u64; 2];
    // SAFETY: copies 4 bytes from a local array into the 16-byte local
    // `key`; both are valid and non-overlapping.
    unsafe {
        core::ptr::copy_nonoverlapping(
            src_bytes.as_ptr(),
            key.as_mut_ptr() as *mut u8,
            4,
        );
    }
    let mut ipb = [0u8; 16];
    ipb[0..4].copy_from_slice(&src_bytes);
    // SAFETY: the returned value is a plain u64 read once, immediately;
    // a concurrent userspace update can only yield a stale expiry.
    if let Some(v) = unsafe { BLOCKLIST.get(&key) } {
        // SAFETY: bpf_ktime_get_ns takes no arguments and has no preconditions.
        let now = unsafe { aya_ebpf::helpers::bpf_ktime_get_ns() };
        if now < *v {
            inc_counter(counter::V4_DROP);
            emit_drop_event(counter::V4_DROP, &ipb);
            return Ok(xdp_action::XDP_DROP);
        }
    }
    // CIDR fallback: LPM_TRIE — one /24 entry replaces 256 flat entries in BLOCKLIST
    if BLOCKCIDR.get(&Key::new(32, key)).is_some() {
        inc_counter(counter::V4_DROP);
        emit_drop_event(counter::V4_DROP, &ipb);
        return Ok(xdp_action::XDP_DROP);
    }
    inc_counter(counter::PASS);
    Ok(xdp_action::XDP_PASS)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
static LICENSE: [u8; 4] = *b"GPL\0";

// silence unused import of xdp_md on some aya versions
const _: Option<&xdp_md> = None;
