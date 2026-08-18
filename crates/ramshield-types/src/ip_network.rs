use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// IP network prefix with configurable CIDR length.
/// Supports both IPv4 and IPv6 with normalized byte order (network byte order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IpNetwork {
    /// Network address in network byte order (big-endian)
    pub addr: IpAddr,
    /// CIDR prefix length (0-32 for IPv4, 0-128 for IPv6)
    pub prefix_len: u8,
}

impl IpNetwork {
    /// Create new IpNetwork, normalizing to network byte order.
    pub fn new(addr: IpAddr, prefix_len: u8) -> Result<Self, &'static str> {
        let max_prefix = match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        if prefix_len > max_prefix {
            return Err("prefix length exceeds address family maximum");
        }
        Ok(Self { addr, prefix_len })
    }

    /// Create IPv4 /24 network (backward compatible with existing batch logic).
    pub fn ipv4_subnet(ip: Ipv4Addr) -> Self {
        let octets = ip.octets();
        // Normalize: mask last octet to zero (network address)
        let network = Ipv4Addr::new(octets[0], octets[1], octets[2], 0);
        Self {
            addr: IpAddr::V4(network),
            prefix_len: 24,
        }
    }

    /// Create IPv6 /64 network (common subnet size for IPv6).
    pub fn ipv6_subnet(ip: Ipv6Addr) -> Self {
        let octets = ip.octets();
        // Normalize: mask last 64 bits to zero (network address)
        let mut network_octets = [0u8; 16];
        network_octets[..8].copy_from_slice(&octets[..8]);
        let network = Ipv6Addr::from(network_octets);
        Self {
            addr: IpAddr::V6(network),
            prefix_len: 64,
        }
    }

    /// Get the address family (4 or 6).
    pub fn family(&self) -> u8 {
        match self.addr {
            IpAddr::V4(_) => 4,
            IpAddr::V6(_) => 6,
        }
    }

    /// Check if an IP address is within this network.
    pub fn contains(&self, ip: IpAddr) -> bool {
        if self.family()
            != match ip {
                IpAddr::V4(_) => 4,
                IpAddr::V6(_) => 6,
            }
        {
            return false;
        }

        match (self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(host)) => {
                let net_bits = u32::from(net);
                let host_bits = u32::from(host);
                let mask = if self.prefix_len == 0 {
                    0
                } else {
                    !0u32 << (32 - self.prefix_len)
                };
                (net_bits & mask) == (host_bits & mask)
            }
            (IpAddr::V6(net), IpAddr::V6(host)) => {
                let net_bits = u128::from(net);
                let host_bits = u128::from(host);
                let mask = if self.prefix_len == 0 {
                    0
                } else {
                    !0u128 << (128 - self.prefix_len)
                };
                (net_bits & mask) == (host_bits & mask)
            }
            _ => false,
        }
    }

    /// Pack network prefix into u128 for use as hash map key.
    /// IPv4: stored in lower 32 bits, IPv6: full 128 bits.
    pub fn pack(&self) -> u128 {
        match self.addr {
            IpAddr::V4(v4) => u128::from(u32::from(v4)),
            IpAddr::V6(v6) => u128::from_be_bytes(v6.octets()),
        }
    }

    /// Unpack from u128 with address family indicator.
    pub fn unpack(packed: u128, family: u8, prefix_len: u8) -> Result<Self, &'static str> {
        match family {
            4 => {
                let octets = (packed as u32).to_be_bytes();
                let addr = IpAddr::V4(Ipv4Addr::from(octets));
                Self::new(addr, prefix_len)
            }
            6 => {
                let octets = packed.to_be_bytes();
                let addr = IpAddr::V6(Ipv6Addr::from(octets));
                Self::new(addr, prefix_len)
            }
            _ => Err("invalid address family"),
        }
    }

    /// Get network prefix bytes for storage/indexing.
    /// IPv4 /24: returns 3 bytes (like legacy [u8; 3]).
    /// IPv6 /64: returns first 8 bytes.
    pub fn prefix_bytes(&self) -> Vec<u8> {
        match self.addr {
            IpAddr::V4(v4) => {
                let o = v4.octets();
                match self.prefix_len {
                    0..=8 => vec![o[0]],
                    9..=16 => vec![o[0], o[1]],
                    17..=24 => vec![o[0], o[1], o[2]],
                    _ => o.to_vec(),
                }
            }
            IpAddr::V6(v6) => {
                let o = v6.octets();
                // Return bytes up to prefix_len / 8 (rounded up)
                let byte_len = ((self.prefix_len as usize) + 7) / 8;
                o[..byte_len.min(16)].to_vec()
            }
        }
    }
}

impl fmt::Display for IpNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipv4_network_creation() {
        let ip = Ipv4Addr::new(192, 168, 1, 100);
        let net = IpNetwork::ipv4_subnet(ip);
        assert_eq!(net.prefix_len, 24);
        assert!(net.contains(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
        assert!(net.contains(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 254))));
        assert!(!net.contains(IpAddr::V4(Ipv4Addr::new(192, 168, 2, 1))));
    }

    #[test]
    fn test_ipv6_network_creation() {
        let ip = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 1, 2, 3, 4);
        let net = IpNetwork::ipv6_subnet(ip);
        assert_eq!(net.prefix_len, 64);
        assert!(net.contains(IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1))));
        assert!(!net.contains(IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb9, 0, 0, 0, 0, 0, 1))));
    }

    #[test]
    fn test_contains_cross_family() {
        let ipv4_net = IpNetwork::ipv4_subnet(Ipv4Addr::new(10, 0, 0, 1));
        assert!(!ipv4_net.contains(IpAddr::V6(Ipv6Addr::LOCALHOST)));
    }

    #[test]
    fn test_pack_unpack_ipv4() {
        let net = IpNetwork::ipv4_subnet(Ipv4Addr::new(10, 20, 30, 40));
        let packed = net.pack();
        let unpacked = IpNetwork::unpack(packed, 4, 24).unwrap();
        assert_eq!(net, unpacked);
    }

    #[test]
    fn test_prefix_bytes() {
        let ipv4_net = IpNetwork::new(IpAddr::V4(Ipv4Addr::new(10, 20, 30, 40)), 24).unwrap();
        assert_eq!(ipv4_net.prefix_bytes(), vec![10, 20, 30]);

        let ipv6_net = IpNetwork::new(
            IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
            64,
        )
        .unwrap();
        assert_eq!(ipv6_net.prefix_bytes().len(), 8);
    }

    #[test]
    fn test_invalid_prefix_length() {
        assert!(IpNetwork::new(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)), 33).is_err());
        assert!(IpNetwork::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 129).is_err());
    }
}
