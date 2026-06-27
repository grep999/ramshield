use crate::detection::ConnectionEvent;
use std::collections::HashMap;
use std::net::IpAddr;

/// In-memory aggregation for one flush window — no store access until flush completes.
#[derive(Debug, Default)]
pub struct IpAgg {
    pub count:        u32,
    pub bytes:        u64,
    pub status_dist:  [u32; 5],
    pub proto_fp:     u32,
    pub first_ts_ns:  u64,
    pub last_ts_ns:   u64,
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
#[inline]
pub fn subnet_key_v4(octets: [u8; 4]) -> u32 {
    (octets[0] as u32) << 24 | (octets[1] as u32) << 16 | (octets[2] as u32) << 8
}

#[inline]
pub fn subnet_key(ip: IpAddr) -> Option<u32> {
    match ip {
        IpAddr::V4(v4) => Some(subnet_key_v4(v4.octets())),
        IpAddr::V6(_)  => None,
    }
}

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
    [
        (key >> 24) as u8,
        (key >> 16) as u8,
        (key >> 8) as u8,
    ]
}

/// Aggregate a slice of connection events into IP and /24 maps in one pass.
pub fn aggregate(events: &[ConnectionEvent]) -> (HashMap<IpAddr, IpAgg>, HashMap<u32, u32>) {
    let mut ips: HashMap<IpAddr, IpAgg> = HashMap::with_capacity(events.len().min(4096));
    let mut subnets = HashMap::new();
    for ev in events {
        ips.entry(ev.ip).or_default().absorb(ev);
        if let Some(sk) = subnet_key(ev.ip) {
            *subnets.entry(sk).or_insert(0) += 1;
        }
    }
    (ips, subnets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn subnet_key_roundtrip() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 20, 30, 40));
        let key = subnet_key(ip).unwrap();
        assert_eq!(subnet_prefix(key), [10, 20, 30]);
        assert!(ip_in_subnet(ip, [10, 20, 30]));
        assert!(!ip_in_subnet(ip, [10, 20, 31]));
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
        let (ips, subnets) = aggregate(&[ev(1), ev(2), ev(3)]);
        assert_eq!(ips[&ip].count, 3);
        assert_eq!(subnets[&subnet_key(ip).unwrap()], 3);
    }
}
