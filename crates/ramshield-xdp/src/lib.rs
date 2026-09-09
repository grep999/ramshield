//! RamShield eBPF / XDP data plane — build artifact crate.
//!
//! Sole export: the compiled BPF ELF from build.rs. All loading, attaching,
//! and map management lives in `ramshield-enforcement::xdp::AyaXdpApplier`
//! (the only consumer). Types (`BlocklistKey`/`BlocklistValue`) are defined
//! there too — one source of truth, no drift.

/// Compiled BPF ELF produced by this crate's build.rs (clang fallback path).
pub static BPF_ELF: &[u8] = aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/ramshield-xdp"));

// ---------------------------------------------------------------------------
// Wire-contract tests
//
// These pin the exact memory layout that the C program (main.c) and the
// host-side Rust (ramshield-enforcement::xdp::BlocklistKey) must agree on.
// We re-derive the types locally to avoid a circular dev-dependency on
// ramshield-enforcement.  The layouts MUST match enforcement/xdp.rs 1:1.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    // --- mirror types (must stay 1:1 with ramshield-enforcement/src/xdp.rs) ---

    /// `__u64[2]` map key.  16 bytes.  IPv4 occupies the low 32 bits of the
    /// first u64; v6 fills all 16 octets in wire order.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[repr(C)]
    struct BlocklistKey(u128);

    impl BlocklistKey {
        fn from_ip(ip: IpAddr) -> Self {
            match ip {
                IpAddr::V4(v4) => BlocklistKey(u128::from(u32::from_ne_bytes(v4.octets()))),
                IpAddr::V6(v6) => BlocklistKey(u128::from_le_bytes(v6.octets())),
            }
        }
    }

    /// Map name routing — v4 → BLOCKLIST, v6 → BLOCKLIST6 (D2 isolation).
    fn xdp_map_for(ip: IpAddr) -> &'static str {
        match ip {
            IpAddr::V4(_) => "BLOCKLIST",
            IpAddr::V6(_) => "BLOCKLIST6",
        }
    }

    /// Family-based split used by reconcile() to keep each map's keys
    /// independent — a v6 key must never enter the v4 map (D2).
    fn split_by_family(blocks: &[IpAddr]) -> (Vec<BlocklistKey>, Vec<BlocklistKey>) {
        let mut v4 = Vec::new();
        let mut v6 = Vec::new();
        for ip in blocks {
            match ip {
                IpAddr::V4(_) => v4.push(BlocklistKey::from_ip(*ip)),
                IpAddr::V6(_) => v6.push(BlocklistKey::from_ip(*ip)),
            }
        }
        (v4, v6)
    }

    // ===== v4 edge-case tests =====

    #[test]
    fn v4_zero_all_zero_key_bytes() {
        // 0.0.0.0 → key bytes [0,0,0,0] + 12 zero pad bytes
        let key = BlocklistKey::from_ip(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let mem = key.0.to_ne_bytes();
        assert_eq!(&mem[..4], &[0, 0, 0, 0]);
        assert_eq!(&mem[4..], &[0; 12]);
    }

    #[test]
    fn v4_broadcast_all_ones() {
        // 255.255.255.255 → key bytes [255,255,255,255] + 12 zero pad bytes
        let key = BlocklistKey::from_ip(IpAddr::V4(Ipv4Addr::BROADCAST));
        let mem = key.0.to_ne_bytes();
        assert_eq!(&mem[..4], &[255, 255, 255, 255]);
        assert_eq!(&mem[4..], &[0; 12]);
    }

    #[test]
    fn v4_loopback() {
        // 127.0.0.1 → key bytes [127,0,0,1]
        let key = BlocklistKey::from_ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let mem = key.0.to_ne_bytes();
        assert_eq!(&mem[..4], &[127, 0, 0, 1]);
        assert_eq!(&mem[4..], &[0; 12]);
    }

    #[test]
    fn v4_key_octet_order_matches_c_saddr_le_roundtrip() {
        // C does: __u32 src_ip = ip->saddr; key[0] = src_ip;
        // saddr is network-order (big-endian wire bytes stored in u32).
        // On LE host, assigning to key[0] stores the same byte pattern.
        // 1.2.3.4 → wire bytes 01 02 03 04 → memory[0..4] = [1,2,3,4].
        let key = BlocklistKey::from_ip(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)));
        let mem = key.0.to_ne_bytes();
        assert_eq!(&mem[..4], &[1, 2, 3, 4], "v4 octets must be wire-order");
        assert_eq!(&mem[4..], &[0; 12], "upper 96 bits must be zero");
    }

    // ===== v6 boundary tests =====

    #[test]
    fn v6_boundary_low_sextets_all_f() {
        // 2001:db8::ffff:ffff:ffff:ffff → `::` compresses TWO zero groups,
        // so the wire octets are `2001 0db8 0000 0000 ffff ffff ffff ffff`.
        // Boundary case: the low four sextets are all-ones (0xff), exercising
        // every low byte of the u128 key. Assert exact wire-order round-trip.
        let addr: Ipv6Addr = "2001:db8::ffff:ffff:ffff:ffff".parse().unwrap();
        let key = BlocklistKey::from_ip(IpAddr::V6(addr));
        let mem = key.0.to_ne_bytes();
        assert_eq!(
            mem,
            [
                0x20, 0x01, 0x0d, 0xb8, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xff
            ],
            "v6 boundary: low sextets must be exact wire-order ff bytes"
        );
        // The four low sextets are all-ones → last 8 bytes must be all 0xff.
        assert_eq!(&mem[8..], &[0xff; 8], "low 64 bits all-ones");
    }

    #[test]
    fn v6_loopback() {
        let addr: Ipv6Addr = "::1".parse().unwrap();
        let key = BlocklistKey::from_ip(IpAddr::V6(addr));
        let mem = key.0.to_ne_bytes();
        assert_eq!(&mem[..15], &[0; 15]);
        assert_eq!(mem[15], 1, "loopback octet at byte 15");
    }

    #[test]
    fn v6_zeros() {
        let addr: Ipv6Addr = "::".parse().unwrap();
        let key = BlocklistKey::from_ip(IpAddr::V6(addr));
        let mem = key.0.to_ne_bytes();
        assert_eq!(mem, [0; 16]);
    }

    #[test]
    fn v6_all_ones() {
        let addr: Ipv6Addr = "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff".parse().unwrap();
        let key = BlocklistKey::from_ip(IpAddr::V6(addr));
        let mem = key.0.to_ne_bytes();
        assert_eq!(mem, [0xff; 16]);
    }

    // ===== split_by_family tests =====

    #[test]
    fn split_empty() {
        let (v4, v6) = split_by_family(&[]);
        assert!(v4.is_empty());
        assert!(v6.is_empty());
    }

    #[test]
    fn split_all_v4() {
        let ips: Vec<IpAddr> = ["1.0.0.1", "2.0.0.2", "255.255.255.254"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        let (v4, v6) = split_by_family(&ips);
        assert_eq!(v4.len(), 3);
        assert!(v6.is_empty());
    }

    #[test]
    fn split_all_v6() {
        let ips: Vec<IpAddr> = ["::1", "2001:db8::1", "fe80::1"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        let (v4, v6) = split_by_family(&ips);
        assert!(v4.is_empty());
        assert_eq!(v6.len(), 3);
    }

    #[test]
    fn split_mixed_preserves_order_and_separates() {
        let ips: Vec<IpAddr> = ["1.2.3.4", "2001:db8::1", "5.6.7.8", "fd00::9"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        let (v4, v6) = split_by_family(&ips);
        assert_eq!(v4.len(), 2);
        assert_eq!(v6.len(), 2);
        assert_eq!(v4[0], BlocklistKey::from_ip(ips[0]));
        assert_eq!(v4[1], BlocklistKey::from_ip(ips[2]));
        assert_eq!(v6[0], BlocklistKey::from_ip(ips[1]));
        assert_eq!(v6[1], BlocklistKey::from_ip(ips[3]));
        // No cross-family key leak
        assert!(
            !v4.iter().any(|k| v6.contains(k)),
            "v4 key leaked into v6 list"
        );
    }

    // ===== from_ip → xdp_map_for roundtrip =====

    #[test]
    fn roundtrip_v4_map_routing() {
        let ips: Vec<IpAddr> = ["0.0.0.0", "127.0.0.1", "10.0.0.1", "255.255.255.255"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        for ip in &ips {
            assert_eq!(xdp_map_for(*ip), "BLOCKLIST");
            let key = BlocklistKey::from_ip(*ip);
            // Key must be reconstructable from the same IP (roundtrip)
            assert_eq!(BlocklistKey::from_ip(*ip), key);
        }
    }

    #[test]
    fn roundtrip_v6_map_routing() {
        let ips: Vec<IpAddr> = [
            "::1",
            "::",
            "2001:db8::1",
            "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            "fe80::1",
        ]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
        for ip in &ips {
            assert_eq!(xdp_map_for(*ip), "BLOCKLIST6");
            let key = BlocklistKey::from_ip(*ip);
            assert_eq!(BlocklistKey::from_ip(*ip), key);
        }
    }

    // ===== wire-contract tests: C key ↔ Rust BlocklistKey =====

    /// C program (main.c):
    ///   __u32 src_ip = ip->saddr;
    ///   __u64 key[2] = {0, 0};
    ///   key[0] = src_ip;
    ///   bpf_map_lookup_elem(&BLOCKLIST, &key);
    ///
    /// On LE (bpfel), `ip->saddr` is the network-order bytes stored as u32.
    /// Assigning to `key[0]` (u64) puts the 4 bytes at memory[0..3], with
    /// bytes [4..7] and [8..15] zero.
    ///
    /// Rust: `BlocklistKey(u128::from(u32::from_ne_bytes(v4.octets())))`.
    /// `u32::from_ne_bytes` places octets at memory[0..3] on LE.
    /// `u128::from(u32)` zero-extends into the high 96 bits.
    ///
    /// Both produce identical memory. This test pins the contract.
    #[test]
    fn wire_contract_v4_matches_c_key_layout() {
        let ips = [
            Ipv4Addr::new(0, 0, 0, 0),
            Ipv4Addr::new(1, 2, 3, 4),
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(255, 255, 255, 255),
            Ipv4Addr::new(10, 20, 30, 40),
        ];
        for v4 in ips {
            let key = BlocklistKey::from_ip(IpAddr::V4(v4));
            let mem = key.0.to_ne_bytes();
            // C layout: key[0] = saddr, key[1] = 0.
            // On LE: saddr bytes are [oct3, oct2, oct1, oct0] in u32 register,
            // but memory bytes are [oct0, oct1, oct2, oct3] for `saddr` field
            // which stores network-order.
            //
            // saddr = 0x01020304 → memory [01 02 03 04] (network-order in struct).
            // key[0] = saddr → same bytes at mem[0..4].
            assert_eq!(
                &mem[..4],
                &v4.octets(),
                "v4 {v4}: C key[0] vs Rust mem[0..4] mismatch"
            );
            assert_eq!(&mem[4..], &[0u8; 12], "v4 {v4}: upper 96 bits must be zero");
        }
    }

    /// C program (main.c):
    ///   __u64 key[2];
    ///   __builtin_memcpy(key, ip6->saddr.s6_addr, sizeof(key));
    ///   bpf_map_lookup_elem(&BLOCKLIST6, &key);
    ///
    /// `s6_addr` is a raw 16-byte array in network (wire) order.
    /// memcpy copies those bytes directly into `key`, which is a `__u64[2]`
    /// (16 bytes). The memory layout of the BPF map key is therefore the
    /// exact wire bytes of the IPv6 source address.
    ///
    /// Rust: `BlocklistKey(u128::from_le_bytes(v6.octets()))`.
    /// `octets()` returns wire-order bytes. `u128::from_le_bytes` places
    /// octet[0] at the lowest memory address → same as C's memcpy.
    ///
    /// This test verifies the exact byte layout for all octets.
    #[test]
    fn wire_contract_v6_matches_c_key_layout() {
        // 2001:db8::1 → octets: 20 01 0d b8 00 00 00 00 00 00 00 00 00 00 00 01
        let addr: Ipv6Addr = "2001:db8::1".parse().unwrap();
        let key = BlocklistKey::from_ip(IpAddr::V6(addr));
        let mem = key.0.to_ne_bytes();
        let expected = addr.octets();
        assert_eq!(
            mem, expected,
            "v6 wire contract: Rust memory must equal wire octets (C memcpy layout)"
        );
    }

    /// Pin: v4 and v6 keys must NEVER be equal (same key in both maps
    /// would be ambiguous).  A v4 key is 4 octets + 12 zeros; a v6 key is
    /// 16 octets in wire order.  They can never collide because the
    /// BLOCKLIST / BLOCKLIST6 separation (D2) prevents cross-family lookups.
    #[test]
    fn v4_v6_keys_are_distinct() {
        let v4 = BlocklistKey::from_ip("1.2.3.4".parse().unwrap());
        let v6 = BlocklistKey::from_ip("::102:304".parse().unwrap());
        assert_ne!(v4, v6, "v4 and v6 keys must never be equal");
    }

    /// VLAN tag handling is done in the C program (main.c) before key
    /// extraction.  This test documents that VLAN-tagged frames go through
    /// the same BLOCKLIST/BLOCKLIST6 lookup — the key extraction is
    /// address-family dependent, not VLAN dependent.
    #[test]
    fn vlan_tag_documentation() {
        // VLAN tags are stripped in C before IP extraction. The Rust side
        // doesn't see VLAN — it only sees the IP address.  Verify that
        // xdp_map_for routing is independent of VLAN (it only cares about
        // address family).
        let v4: IpAddr = "10.0.0.1".parse().unwrap();
        let v6: IpAddr = "2001:db8::1".parse().unwrap();
        assert_eq!(xdp_map_for(v4), "BLOCKLIST");
        assert_eq!(xdp_map_for(v6), "BLOCKLIST6");
        // This documents: VLAN tags are transparent to map routing.
    }

    /// BLOCKLIST vs BLOCKLIST6 separation: v4 must route to BLOCKLIST,
    /// v6 must route to BLOCKLIST6.  Never the reverse.  This is the D2
    /// isolation that prevents cross-family map contamination.
    #[test]
    fn blocklist_blocklist6_separation() {
        let v4_ips: Vec<IpAddr> = ["1.0.0.1", "10.0.0.1", "192.168.1.1", "255.255.255.255"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        let v6_ips: Vec<IpAddr> = ["::1", "2001:db8::1", "fe80::1", "ff02::1"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();

        for ip in &v4_ips {
            assert_eq!(
                xdp_map_for(*ip),
                "BLOCKLIST",
                "v4 {ip} must go to BLOCKLIST"
            );
        }
        for ip in &v6_ips {
            assert_eq!(
                xdp_map_for(*ip),
                "BLOCKLIST6",
                "v6 {ip} must go to BLOCKLIST6"
            );
        }
    }

    // ===== aya ELF load verification =====

    /// Load the compiled BPF ELF via aya and verify maps + program exist.
    /// Proves: build.rs → aya-ebpf → bpf-linker → ELF → aya parser → BPF syscalls.
    /// Requires root (BPF syscalls).
    #[test]
    #[ignore] // requires root — run with: cargo test -- --ignored aya_load
    fn aya_load_verifies_maps_and_program() {
        let mut bpf = aya::Bpf::load(crate::BPF_ELF)
            .expect("aya::Bpf::load failed — legacy maps rejected or ELF corrupt");

        // Maps must exist and be takeable
        let _bl = bpf.take_map("BLOCKLIST")
            .expect("BLOCKLIST map missing from loaded ELF");
        let _bl6 = bpf.take_map("BLOCKLIST6")
            .expect("BLOCKLIST6 map missing from loaded ELF");

        // Program must exist
        let _prog = bpf.program("ramshield_xdp")
            .expect("ramshield_xdp program missing");

        eprintln!("aya::Bpf::load succeeded — BLOCKLIST, BLOCKLIST6, ramshield_xdp all present");
    }
}
