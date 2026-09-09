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
    maps::HashMap,
    programs::XdpContext,
};
use core::mem;
use network_types::{
    eth::{EthHdr, EtherType},
    ip::{Ipv4Hdr, Ipv6Hdr},
};

// 16-byte key matching C's __u64[2] layout.
// Capacity tuned via BLOCKLIST_CAP env var at build time (default 102_400).
#[map]
static BLOCKLIST: HashMap<[u64; 2], u8> = HashMap::with_max_entries(blocklist_cap_env(), 0);

#[map]
static BLOCKLIST6: HashMap<[u64; 2], u8> = HashMap::with_max_entries(blocklist_cap_env(), 0);

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
        Err(()) => xdp_action::XDP_PASS,
    }
}

fn try_ramshield_xdp(ctx: XdpContext) -> Result<u32, ()> {
    let eth: *const EthHdr = ptr_at(&ctx, 0)?;
    let mut proto = unsafe { (*eth).ether_type } as u16;
    let mut l3_off = EthHdr::LEN;

    // Strip up to 4 VLAN tags (802.1Q / 802.1ad), matching C's loop.
    for _ in 0..4 {
        if proto != ETH_P_8021Q && proto != ETH_P_8021AD {
            break;
        }
        if l3_off + 4 > ctx.data_end() {
            return Ok(xdp_action::XDP_PASS);
        }
        proto = unsafe { core::ptr::read_unaligned((ctx.data() + l3_off + 2) as *const u16) };
        l3_off += 4;
    }

    if proto == EtherType::Ipv6 as u16 {
        let ip6: *const Ipv6Hdr = ptr_at(&ctx, l3_off)?;
        let mut key = [0u64; 2];
        unsafe {
            core::ptr::copy_nonoverlapping(
                (*ip6).src_addr.as_ptr(),
                key.as_mut_ptr() as *mut u8,
                16,
            );
        }
        if unsafe { BLOCKLIST6.get(&key) }.is_some() {
            return Ok(xdp_action::XDP_DROP);
        }
        return Ok(xdp_action::XDP_PASS);
    }

    if proto != EtherType::Ipv4 as u16 {
        return Ok(xdp_action::XDP_PASS);
    }

    let ip: *const Ipv4Hdr = ptr_at(&ctx, l3_off)?;
    let src = u32::from_be_bytes(unsafe { (*ip).src_addr });
    let key = [src as u64, 0u64];
    if unsafe { BLOCKLIST.get(&key) }.is_some() {
        return Ok(xdp_action::XDP_DROP);
    }
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
