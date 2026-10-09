use ahash::AHashMap as HashMap;
use ramshield_types::IpNetwork;
use ramshield_types::events::{ConnectionEvent, HttpMethod};
use std::net::IpAddr;

/// In-memory aggregation for one flush window — no store access until flush completes.
#[derive(Debug, Default, Clone, Copy)]
pub struct RouteAgg {
    pub route_hash: u64,
    pub method: HttpMethod,
    pub count: u32,
    pub http2_opened: u32,
    pub http2_reset: u32,
    pub latency_sum_us: u64,
    pub latency_max_us: u64,
}

#[derive(Debug, Default, Clone)]
pub struct IpAgg {
    pub count: u32,
    pub bytes: u64,
    pub status_dist: [u32; 5],
    pub proto_fp: u32,
    pub first_ts_ns: u64,
    pub last_ts_ns: u64,
    pub l7_count: u32,
    pub http2_opened: u32,
    pub http2_reset: u32,
    pub http2_completed: u32,
    pub http2_active_max: u32,
    pub routes: [RouteAgg; 4],
}

impl IpAgg {
    pub fn absorb(&mut self, ev: &ConnectionEvent) {
        self.count += 1;
        self.bytes += ev.bytes;
        // 600B L1-resident const table beats the /100 division per event.
        if ev.status_code < 600 {
            let b = super::STATUS_BUCKET[ev.status_code as usize];
            if b != 255 {
                self.status_dist[b as usize] += 1;
            }
        }
        if self.count == 1 {
            self.first_ts_ns = ev.timestamp_ns;
            self.proto_fp = ev.proto_fingerprint;
        }
        if let Some(l7) = ev.l7 {
            self.l7_count = self.l7_count.saturating_add(1);
            if let Some(h2) = l7.http2 {
                self.http2_opened = self.http2_opened.saturating_add(h2.streams_opened);
                self.http2_reset = self.http2_reset.saturating_add(h2.streams_reset);
                self.http2_completed = self.http2_completed.saturating_add(h2.streams_completed);
                self.http2_active_max = self.http2_active_max.max(h2.active_streams);
            }
            if l7.route_hash != 0 {
                if let Some(slot) = self.routes.iter_mut().find(|r| r.route_hash == l7.route_hash) {
                    slot.count = slot.count.saturating_add(1);
                    if let Some(h2) = l7.http2 {
                        slot.http2_opened = slot.http2_opened.saturating_add(h2.streams_opened);
                        slot.http2_reset = slot.http2_reset.saturating_add(h2.streams_reset);
                    }
                    slot.latency_sum_us = slot.latency_sum_us.saturating_add(l7.latency_us);
                    slot.latency_max_us = slot.latency_max_us.max(l7.latency_us);
                    slot.method = l7.method;
                } else if let Some(slot) = self.routes.iter_mut().find(|r| r.route_hash == 0) {
                    *slot = RouteAgg { route_hash: l7.route_hash, method: l7.method, count: 1, latency_sum_us: l7.latency_us, latency_max_us: l7.latency_us, http2_opened: l7.http2.map_or(0, |h| h.streams_opened), http2_reset: l7.http2.map_or(0, |h| h.streams_reset) };
                } else if let Some((idx, _)) = self.routes.iter().enumerate().min_by_key(|(_, r)| r.count)
                    && self.routes[idx].count <= 1
                {
                    self.routes[idx] = RouteAgg { route_hash: l7.route_hash, method: l7.method, count: 1, latency_sum_us: l7.latency_us, latency_max_us: l7.latency_us, http2_opened: l7.http2.map_or(0, |h| h.streams_opened), http2_reset: l7.http2.map_or(0, |h| h.streams_reset) };
                }
            }
        }
        self.last_ts_ns = ev.timestamp_ns;
    }

    /// Consolidate another aggregate for the SAME IP (worker-local buffer
    /// merged into the shared pre_aggs — RAM-for-CPU item 3). Counters add;
    /// timestamps take window min/max; first non-zero proto_fp wins.
    pub fn merge_with(&mut self, other: &Self) {
        self.count = self.count.saturating_add(other.count);
        self.bytes = self.bytes.saturating_add(other.bytes);
        for (d, o) in self.status_dist.iter_mut().zip(other.status_dist.iter()) {
            *d = d.saturating_add(*o);
        }
        self.first_ts_ns = self.first_ts_ns.min(other.first_ts_ns);
        self.last_ts_ns = self.last_ts_ns.max(other.last_ts_ns);
        self.l7_count = self.l7_count.saturating_add(other.l7_count);
        self.http2_opened = self.http2_opened.saturating_add(other.http2_opened);
        self.http2_reset = self.http2_reset.saturating_add(other.http2_reset);
        self.http2_completed = self.http2_completed.saturating_add(other.http2_completed);
        self.http2_active_max = self.http2_active_max.max(other.http2_active_max);
        for other_route in other.routes.iter().filter(|r| r.route_hash != 0) {
            if let Some(slot) = self.routes.iter_mut().find(|r| r.route_hash == other_route.route_hash) {
                slot.count = slot.count.saturating_add(other_route.count);
                slot.http2_opened = slot.http2_opened.saturating_add(other_route.http2_opened);
                slot.http2_reset = slot.http2_reset.saturating_add(other_route.http2_reset);
                slot.latency_sum_us = slot.latency_sum_us.saturating_add(other_route.latency_sum_us);
                slot.latency_max_us = slot.latency_max_us.max(other_route.latency_max_us);
                slot.method = other_route.method;
            } else if let Some(slot) = self.routes.iter_mut().find(|r| r.route_hash == 0) {
                *slot = *other_route;
            }
        }
        if self.proto_fp == 0 {
            self.proto_fp = other.proto_fp;
        }
    }
}

/// Pack IPv4 /24 prefix into u32 for subnet-scale counters (no string keys).
/// Network byte order (big-endian): octet[0] in highest bits.
#[inline]
fn subnet_key_v4(octets: [u8; 4]) -> u32 {
    (octets[0] as u32) << 24 | (octets[1] as u32) << 16 | (octets[2] as u32) << 8
}

/// Pack IPv6 /64 prefix into u128 for subnet-scale counters.
/// Network byte order: first 8 bytes in high bits, host bits zeroed.
#[inline]
fn subnet_key_v6(octets: [u8; 16]) -> u128 {
    let full = u128::from_be_bytes(octets);
    // Zero out the lower 64 bits (host part of /64)
    full & 0xFFFF_FFFF_FFFF_FFFF_0000_0000_0000_0000
}

/// Get network key as u128 for both address families.
/// IPv4 keys are in lower 32 bits; IPv6 keys use full 128 bits.
#[inline]
pub(crate) fn subnet_key(ip: IpAddr) -> Option<(u128, IpNetwork)> {
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

/// Check if an IP is in the canonical subnet of `ref_ip` (v4 /24, v6 /64) —
/// Task 1: was ip_in_subnet([u8;3]) + subnet_prefix(key), which could not
/// express v6. IpNetwork::contains is the family-complete equivalent.
#[cfg(test)]
#[inline]
pub(crate) fn same_subnet(ref_ip: IpAddr, ip: IpAddr) -> bool {
    IpNetwork::of_ip(ref_ip).contains(ip)
}

/// Aggregate a slice of connection events into IP and subnet maps in one pass.
/// Returns:
/// Per-flush aggregates: per-IP stats, per-/24 event + distinct-member counts,
/// and the network (v4 /24, v6 /64) each key maps to.
///
/// (The old tuple return carried the same three fields; struct form keeps the
/// complex type under clippy's complexity threshold.)
pub struct FlushAggs {
    pub ips: HashMap<IpAddr, IpAgg>,
    pub subnets: HashMap<u128, (u32, Vec<IpAddr>)>, // (events, distinct member IPs)
    pub networks: HashMap<u128, IpNetwork>,
}

pub fn aggregate(events: &[ConnectionEvent]) -> FlushAggs {
    let mut ips: HashMap<IpAddr, IpAgg> = HashMap::with_capacity(events.len().min(4096));
    let mut subnets = HashMap::new();
    let mut networks = HashMap::new();
    for ev in events {
        let entry = ips.entry(ev.ip).or_default();
        let first_for_ip = entry.count == 0;
        entry.absorb(ev);
        if let Some((sk, net)) = subnet_key(ev.ip) {
            let e = subnets.entry(sk).or_insert((0, Vec::new()));
            e.0 += 1;
            if first_for_ip {
                e.1.push(ev.ip); // once per distinct IP — bitmap input
            }
            // Only store once per key (all IPs in same subnet → same network)
            networks.entry(sk).or_insert(net);
        }
    }
    FlushAggs {
        ips,
        subnets,
        networks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn subnet_key_roundtrip() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 20, 30, 40));
        let (key, net) = subnet_key(ip).unwrap();
        assert_eq!(net.to_string(), "10.20.30.0/24");
        assert!(same_subnet(ip, ip));
        assert!(!same_subnet(ip, IpAddr::V4(Ipv4Addr::new(10, 20, 31, 1))));
        assert_eq!(net.prefix_len, 24);
        assert_eq!(net.family(), 4);
        assert_eq!(key, net.pack(), "subnet key == packed network address");
    }

    /// Task 1: the old [u8;3] helper could not express v6 membership at all
    /// (always false). same_subnet must — this is the behavior delta pinned.
    #[test]
    fn same_subnet_covers_v6() {
        let a: IpAddr = "2001:db8:abcd::5".parse().unwrap();
        let b: IpAddr = "2001:db8:abcd::ff".parse().unwrap();
        let c: IpAddr = "2001:db8:abce::1".parse().unwrap();
        assert!(same_subnet(a, b), "same /64");
        assert!(!same_subnet(a, c), "different /64");
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
    fn absorb_invalid_status_no_panic() {
        use ramshield_types::events::{ConnectionEvent, HttpMethod};
        use std::net::IpAddr;
        use std::net::Ipv4Addr;

        let mut agg = IpAgg::default();
        let ev = ConnectionEvent {
            ip: IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)),
            timestamp_ns: 1,
            bytes: 10,
            status_code: 0, // invalid — status_bucket returns 255
            proto_fingerprint: 0,
            l7: None,
        };
        agg.absorb(&ev);
        assert_eq!(agg.count, 1);
        assert_eq!(agg.bytes, 10);
        assert_eq!(agg.status_dist, [0, 0, 0, 0, 0]); // untouched
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
            l7: None,
        };
        let a = aggregate(&[ev(1), ev(2), ev(3)]);
        assert_eq!(a.ips[&ip].count, 3);
        let sk = subnet_key(ip).unwrap().0;
        assert_eq!(a.subnets[&sk].0, 3, "3 events");
        assert_eq!(
            a.subnets[&sk].1,
            vec![ip],
            "distinct member captured for bitmap"
        );
        assert!(a.networks.contains_key(&sk));
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
            l7: None,
        };
        let a = aggregate(&[ev(ipv4, 1), ev(ipv4, 2), ev(ipv6, 3)]);
        let (ips, subnets, networks) = (&a.ips, &a.subnets, &a.networks);
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
    #[test]
    fn l7_and_http2_telemetry_is_bounded_and_aggregated() {
        use ramshield_types::events::{Http2Telemetry, HttpMethod, HttpVersion, L7Metadata};
        let ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));
        let ev = ConnectionEvent {
            ip, timestamp_ns: 1, bytes: 100, status_code: 200, proto_fingerprint: 0,
            l7: Some(L7Metadata {
                host_hash: 1, route_hash: 42, method: HttpMethod::Post, version: HttpVersion::Http2,
                latency_us: 500, request_bytes: 32, response_bytes: 128,
                http2: Some(Http2Telemetry { active_streams: 8, streams_opened: 10, streams_reset: 9, streams_completed: 1 }),
            }),
        };
        let a = aggregate(&[ev]);
        let agg = &a.ips[&ip];
        assert_eq!(agg.l7_count, 1);
        assert_eq!(agg.http2_opened, 10);
        assert_eq!(agg.http2_reset, 9);
        assert_eq!(agg.http2_active_max, 8);
        assert_eq!(agg.routes[0].route_hash, 42);
        assert_eq!(agg.routes[0].http2_reset, 9);
    }

}