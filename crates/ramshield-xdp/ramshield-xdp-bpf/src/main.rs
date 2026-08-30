#![no_std]
#![no_main]

use aya_ebpf::{
    bindings::xdp_action,
    macros::{map, xdp},
    maps::HashMap,
    programs::XdpContext,
};
use aya_ebpf::bindings::xdp_md;
use core::mem;
use network_types::{
    eth::{EthHdr, EtherType},
    ip::{Ipv4Hdr, Ipv6Hdr},
};

// IPv4 source → present means DROP. Value unused.
// Capacity passed via BLOCKLIST_CAP env at build time (default 102_400).
// Tune with `BLOCKLIST_CAP=1048576` for production deployments expecting
// millions of concurrent blocks. Set per [xdp] config blocklist_cap userspace
// limit when that ships.
#[map]
static BLOCKLIST: HashMap<u32, u8> = HashMap::with_max_entries(blocklist_cap_env(), 0);

// IPv6 source: 16 raw octets of src_addr (network-order). Byte-for-byte
// identical to userspace BlocklistKey::from_ip(V6) low 128 bits
// (u128::from_be_bytes(v6.octets())). Same capacity budget; tune via env too.
#[map]
static BLOCKLIST_V6: HashMap<[u8; 16], u8> =
    HashMap::with_max_entries(blocklist_cap_env(), 0);

#[inline(always)]
const fn blocklist_cap_env() -> u32 {
    // Default 102_400; override at build: BLOCKLIST_CAP=N cargo build ...
    match option_env!("BLOCKLIST_CAP") {
        Some(s) => {
            // ponytail: const parse — no std::parse at compile time, only literal-ish.
            // Honest: this only fires for non-numeric env vars; numbers in source still win.
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
            if v == 0 { 102_400 } else { v }
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
    let ether_type = unsafe { (*eth).ether_type };
    if ether_type == EtherType::Ipv4 {
        let ip: *const Ipv4Hdr = ptr_at(&ctx, EthHdr::LEN)?;
        let src = u32::from_be(unsafe { (*ip).src_addr });
        if unsafe { BLOCKLIST.get(&src) }.is_some() {
            return Ok(xdp_action::XDP_DROP);
        }
        return Ok(xdp_action::XDP_PASS);
    }
    if ether_type == EtherType::Ipv6 {
        let ip6: *const Ipv6Hdr = ptr_at(&ctx, EthHdr::LEN)?;
        // ponytail: version 4 means malformed. PASS to keep fail-open semantics
        // (see try_ramshield_xdp outer catch-all).
        if unsafe { (*ip6).version() } == 6 {
            let src = unsafe { (*ip6).src_addr };
            if unsafe { BLOCKLIST_V6.get(&src) }.is_some() {
                return Ok(xdp_action::XDP_DROP);
            }
        }
        return Ok(xdp_action::XDP_PASS);
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
