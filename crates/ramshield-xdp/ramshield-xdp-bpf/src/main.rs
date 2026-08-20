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
    ip::Ipv4Hdr,
};

/// IPv4 source → present means DROP. Value unused.
#[map]
static BLOCKLIST: HashMap<u32, u8> = HashMap::with_max_entries(102_400, 0);

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
    if unsafe { (*eth).ether_type } != EtherType::Ipv4 {
        return Ok(xdp_action::XDP_PASS);
    }
    let ip: *const Ipv4Hdr = ptr_at(&ctx, EthHdr::LEN)?;
    let src = u32::from_be(unsafe { (*ip).src_addr });
    if unsafe { BLOCKLIST.get(&src) }.is_some() {
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
