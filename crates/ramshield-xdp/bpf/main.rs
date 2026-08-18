use aya_bpf::macros::xdp;
use aya_bpf::programs::XdpContext;

#[xdp]
pub fn ramshield_xdp(ctx: XdpContext) -> u32 {
    0 // Default to PASS
}
