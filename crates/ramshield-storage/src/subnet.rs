//! Subnet key helpers: u128 keys (IPv4 low-32, IPv6 full) shared by
//! detection batch aggregation and storage subnet tables.

use ramshield_types::IpNetwork;
use std::net::IpAddr;

/// Pack IPv4 /24 into u128 (low 32 bits).
#[inline]
pub fn subnet_key_v4(octets: [u8; 4]) -> u32 {
    u32::from_be_bytes([octets[0], octets[1], octets[2], 0])
}

/// Pack IPv6 /64 prefix into u128 for subnet-scale counters.
/// Network byte order: first 8 bytes in high bits, host bits zeroed.
#[inline]
pub fn subnet_key_v6(octets: [u8; 16]) -> u128 {
    let full = u128::from_be_bytes(octets);
    // Zero out the lower 64 bits (host part of /64)
    full & 0xFFFF_FFFF_FFFF_FFFF_0000_0000_0000_0000
}

/// Get network key as u128 for both address families.
/// IPv4 keys are in lower 32 bits; IPv6 keys use full 128 bits.
#[inline]
pub fn subnet_key(ip: IpAddr) -> Option<(u128, IpNetwork)> {
    match ip {
        IpAddr::V4(v4) => {
            let net = IpNetwork::ipv4_subnet(v4);
            Some((subnet_key_v4(v4.octets()) as u128, net))
        }
        IpAddr::V6(v6) => {
            let net = IpNetwork::ipv6_subnet(v6);
            Some((subnet_key_v6(v6.octets()), net))
        }
    }
}

/// u128 subnet key without metadata (hot path).
#[inline]
pub fn subnet_key_u128(ip: IpAddr) -> Option<u128> {
    match ip {
        IpAddr::V4(v4) => Some(subnet_key_v4(v4.octets()) as u128),
        IpAddr::V6(v6) => Some(subnet_key_v6(v6.octets())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v4_roundtrip() {
        let ip: IpAddr = "10.20.30.40".parse().unwrap();
        let (key, net) = subnet_key(ip).unwrap();
        assert_eq!(key, (10u32 << 24 | 20 << 16 | 30 << 8) as u128);
        assert_eq!(net.prefix_len, 24);
        assert_eq!(net.prefix_octets(), [10, 20, 30]);
    }

    #[test]
    fn v6_host_bits_zeroed() {
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        let (key, net) = subnet_key(ip).unwrap();
        assert_eq!(key & 0xFFFF, 0); // host bits zero
        assert_eq!(net.prefix_len, 64);
        let k2 = subnet_key_u128(ip).unwrap();
        assert_eq!(key, k2);
    }

    #[test]
    fn families_never_collide() {
        let v4 = subnet_key_u128("1.2.3.4".parse().unwrap()).unwrap();
        let v6 = subnet_key_u128("::102:304".parse().unwrap()).unwrap();
        assert_ne!(v4, v6);
    }
}
