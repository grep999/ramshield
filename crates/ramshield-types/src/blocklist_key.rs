use crate::ip_network::IpNetwork;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// XDP blocklist key — must stay byte-compatible with the C program's
/// `__u64[2]` map key. IPv4 occupies the low 32 bits of the first u64.
///
/// Single source of truth (CQ-P0-1): both ramshield-xdp and
/// ramshield-enforcement import this struct. Any byte-layout drift between
/// the two crates would cause silent blocklist failures (v6 key in v4 map).
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlocklistKey(pub [u8; 16]);

// SAFETY: BlocklistKey is `#[repr(C, align(8))]` and contains only a
// `[u8; 16]`; it has no uninitialized bytes or invalid bit patterns.
// Note: aya::Pod impl lives in ramshield-enforcement/src/xdp.rs where `aya` is available.
// The types crate must not depend on `aya` to avoid circular dependency.

impl BlocklistKey {
    /// Creates a zeroed key.
    #[inline(always)]
    pub const fn zeroed() -> Self {
        Self([0u8; 16])
    }

    /// Access the underlying raw byte slice.
    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    pub fn from_ip(ip: IpAddr) -> Self {
        Self::from(ip)
    }
}

impl From<Ipv4Addr> for BlocklistKey {
    #[inline(always)]
    fn from(v4: Ipv4Addr) -> Self {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&v4.octets());
        Self(bytes)
    }
}

impl From<Ipv6Addr> for BlocklistKey {
    #[inline(always)]
    fn from(v6: Ipv6Addr) -> Self {
        Self(v6.octets())
    }
}

impl From<IpAddr> for BlocklistKey {
    #[inline(always)]
    fn from(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(v4) => Self::from(v4),
            IpAddr::V6(v6) => Self::from(v6),
        }
    }
}

/// Map a v6 address must never share with a v4 key: the 16-byte v6 layout
/// can collide numerically with a v4-shaped key (D2), so isolation is by
/// map, not bytes. Single source of truth for routing (apply + reconcile).
pub fn xdp_map_for(ip: IpAddr) -> &'static str {
    match ip {
        IpAddr::V4(_) => "BLOCKLIST",
        IpAddr::V6(_) => "BLOCKLIST6",
    }
}

pub fn cidr_map_for(network: IpNetwork) -> &'static str {
    match network.addr {
        IpAddr::V4(_) => "BLOCKCIDR",
        IpAddr::V6(_) => "BLOCKCIDR6",
    }
}

/// Split expected blocks by address family. V4 and V6 use dedicated maps
/// to prevent 12-byte-v6-in-16-byte-key collisions.
pub fn split_by_family(expected_blocks: &[IpAddr]) -> (Vec<BlocklistKey>, Vec<BlocklistKey>) {
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for ip in expected_blocks {
        match ip {
            IpAddr::V4(_) => v4.push(BlocklistKey::from_ip(*ip)),
            IpAddr::V6(_) => v6.push(BlocklistKey::from_ip(*ip)),
        }
    }
    (v4, v6)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    /// P0 regression: the kernel memcmp's the raw memory of BlocklistKey
    /// against the C program's `key[0] = ip->saddr` layout — wire octets in
    /// original order at bytes [0..4].
    #[test]
    fn v4_key_bytes_match_dataplane_layout() {
        let key = BlocklistKey::from_ip(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)));
        assert_eq!(
            &key.0[0..4],
            &[1, 2, 3, 4],
            "Octet order must match C wire order"
        );
        assert_eq!(
            &key.0[4..16],
            &[0u8; 12],
            "Trailing 12 bytes must be zeroed padding for IPv4"
        );
    }

    #[test]
    fn v6_key_bytes_match_dataplane_layout() {
        let key = BlocklistKey::from_ip("2001:db8::1".parse().unwrap());
        assert_eq!(&key.0[0..2], &[0x20, 0x01]);
        assert_eq!(&key.0[2..4], &[0x0d, 0xb8]);
        assert_eq!(key.0[15], 1, "last octet at key[15] = wire order");
    }

    #[test]
    fn family_routes_to_dedicated_map() {
        assert_eq!(super::xdp_map_for("1.2.3.4".parse().unwrap()), "BLOCKLIST");
        assert_eq!(
            super::xdp_map_for("2001:db8::1".parse().unwrap()),
            "BLOCKLIST6"
        );
    }

    #[test]
    fn split_by_family_never_crosses_maps() {
        let mixed = ["1.2.3.4", "2001:db8::1", "5.6.7.8", "fd00::9"];
        let ips: Vec<IpAddr> = mixed.iter().map(|s| s.parse().unwrap()).collect();
        let (v4, v6) = split_by_family(&ips);
        assert_eq!(v4.len(), 2);
        assert_eq!(v6.len(), 2);
        assert_eq!(v6[0].0, BlocklistKey::from_ip(ips[1]).0);
        assert_eq!(v6[1].0, BlocklistKey::from_ip(ips[3]).0);
        assert!(
            !v4.iter().any(|k| v6.contains(k)),
            "a key must not appear in both sweeps"
        );
    }
}
