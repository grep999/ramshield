use ramshield_types::events::ConnectionEvent;
use ramshield_types::IpNetwork;
use std::collections::HashMap;
use std::net::IpAddr;

/// In-memory aggregation for one flush window — no store access until flush completes.
#[derive(Debug, Default, Clone)]
pub struct IpAgg {
    pub count: u32,
    pub bytes: u64,
    pub status_dist: [u32; 5],
    pub proto_fp: u32,
    pub first_ts_ns: u64,
    pub last_ts_ns: u64,
}

impl IpAgg {
    pub fn absorb(&mut self, ev: &ConnectionEvent) {
        self.count += 1;
        self.bytes += ev.bytes;
        let bucket = ((ev.status_code / 100).saturating_sub(1)).min(4) as usize;
        self.status_dist[bucket] += 1;
        if self.count == 1 {
            self.first_ts_ns = ev.timestamp_ns;
            self.proto_fp = ev.proto_fingerprint;
        }
        self.last_ts_ns = ev.timestamp_ns;
    }
}

/// Pack IPv4 /24 prefix into u32 for subnet-scale counters (no string keys).
/// Network byte order (big-endian): octet[0] in highest bits.
#[inline]
pub fn subnet_key_v4(octets: [u8; 4]) -> u32 {
    (octets[0] as u32) << 24 | (octets[1] as u32) << 16 | (octets[2] as u32) << 8
}

/// Pack IPv6 /64 prefix into u128 for subnet-scale counters.
/// Network byte order: first 8 bytes in high bits.
#[inline]
pub fn subnet_key_v6(octets: [u8; 16]) -> u128 {
    u128::from_be_bytes(octets)
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

/// Check if IP is within a given IPv4 subnet prefix (legacy /24 compat).
#[inline]
pub fn ip_in_subnet(ip: IpAddr, prefix: [u8; 3]) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == prefix[0] && o[1] == prefix[1] && o[2] == prefix[2]
        }
        IpAddr::V6(_) => false,
    }
}

#[inline]
pub fn subnet_prefix(key: u32) -> [u8; 3] {
    [(key >> 24) as u8, (key >> 16) as u8, (key >> 8) as u8]
}

/// Aggregate a slice of connection events into IP and subnet maps in one pass.
/// Returns:
/// - Per-IP aggregation map
/// - Subnet counters keyed by packed u128 (IPv4 in low 32, IPv6 full 128)
/// - Network metadata for each subnet key (address family, prefix, CIDR)
pub fn aggregate(
    events: &[ConnectionEvent],
) -> (
    HashMap<IpAddr, IpAgg>,
    HashMap<u128, u32>,
    HashMap<u128, IpNetwork>,
) {
    let mut ips: HashMap<IpAddr, IpAgg> = HashMap::with_capacity(events.len().min(4096));
    let mut subnets = HashMap::new();
    let mut networks = HashMap::new();
    for ev in events {
        ips.entry(ev.ip).or_default().absorb(ev);
        if let Some((sk, net)) = subnet_key(ev.ip) {
            *subnets.entry(sk).or_insert(0) += 1;
            // Only store once per key (all IPs in same subnet → same network)
            networks.entry(sk).or_insert(net);
        }
    }
    (ips, subnets, networks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn subnet_key_roundtrip() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 20, 30, 40));
        let (key, net) = subnet_key(ip).unwrap();
        assert_eq!(subnet_prefix(key as u32), [10, 20, 30]);
        assert!(ip_in_subnet(ip, [10, 20, 30]));
        assert!(!ip_in_subnet(ip, [10, 20, 31]));
        assert_eq!(net.prefix_len, 24);
        assert_eq!(net.family(), 4);
    }

    #[test]
    fn subnet_key_v6_roundtrip() {
        let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 1, 2, 3, 4));
        let (key, net) = subnet_key(ip).unwrap();
        assert_eq!(net.prefix_len, 64);
        assert_eq!(net.family(), 6);
        // Verify network address has host bits zeroed
        match net.addr {
            IpAddr::V6(n) => assert_eq!(n.octets()[8..], [0u8; 8]),
            _ => panic!("expected IPv6"),
        }
        // Verify key packs to same value as the network address
        assert_eq!(key, net.pack());
    }

    #[test]
    fn aggregate_counts() {
        let ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        let ev = |n| ConnectionEvent {
            ip,
            timestamp_ns: n,
            bytes: 100,
            status_code: 200,
            proto_fingerprint: 0,
        };
        let (ips, subnets, networks) = aggregate(&[ev(1), ev(2), ev(3)]);
        assert_eq!(ips[&ip].count, 3);
        let sk = subnet_key(ip).unwrap().0;
        assert_eq!(subnets[&sk], 3);
        assert!(networks.contains_key(&sk));
    }

    #[test]
    fn aggregate_dual_stack() {
        let ipv4 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let ipv6 = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        let ev = |ip, n| ConnectionEvent {
            ip,
            timestamp_ns: n,
            bytes: 100,
            status_code: 200,
            proto_fingerprint: 0,
        };
        let (ips, subnets, networks) = aggregate(&[ev(ipv4, 1), ev(ipv4, 2), ev(ipv6, 3)]);
        // Per-IP counts
        assert_eq!(ips[&ipv4].count, 2);
        assert_eq!(ips[&ipv6].count, 1);
        // Subnet counts: two different subnets
        assert_eq!(subnets.len(), 2);
        // Network metadata: one IPv4, one IPv6
        assert_eq!(networks.len(), 2);
        let ipv4_families: Vec<_> = networks.values().filter(|n| n.family() == 4).collect();
        let ipv6_families: Vec<_> = networks.values().filter(|n| n.family() == 6).collect();
        assert_eq!(ipv4_families.len(), 1);
        assert_eq!(ipv6_families.len(), 1);
    }

    #[test]
    fn byte_order_normalization() {
        // Verify IPv4 subnet key is in network byte order
        let ip = Ipv4Addr::new(192, 168, 1, 100);
        let key = subnet_key_v4(ip.octets());
        // Big-endian: 192 in highest byte
        assert_eq!((key >> 24) as u8, 192);
        assert_eq!((key >> 16) as u8, 168);
        assert_eq!((key >> 8) as u8, 1);
        // Last octet masked out (host portion)
        assert_eq!(key & 0xFF, 0);

        // Verify IPv6 subnet key is in network byte order
        let ipv6 = Ipv6Addr::new(0x2001, 0xdb8, 0, 1, 0, 0, 0, 1);
        let key = subnet_key_v6(ipv6.octets());
        let bytes = key.to_be_bytes();
        assert_eq!(&bytes[..2], &[0x20, 0x01]);
        assert_eq!(&bytes[2..4], &[0x0d, 0xb8]);
    }
}
