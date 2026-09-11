//! Real XDP dataplane via aya. Loads the Rust aya-ebpf BPF ELF, attaches to an
//! interface, and applies block/unblock decisions to the kernel BLOCKLIST map.
//!
//! Semantics (see .hermes/plans/2026-08-22_enforcement-production.md):
//! - fail-open at kernel: map update failure => Err, caller keeps in-band state
//! - reconcile(): drain map, delete stale keys, insert missing
//! - key contract: C stores `key[0] = saddr` (LE round-trip of BE octets =
//!   original octet order at memory[0..4]); Rust must serialize the same
//!   bytes — see BlocklistKey::from_ip (P0: u32::from_ne_bytes, NOT
//!   u32::from, which byte-reverses and makes every lookup miss).

#![allow(unsafe_code)]

use crate::{EnforcementError, ReconciliationState, XdpApplier, XdpDropEvent};
use aya::Ebpf;
use aya::maps::{HashMap as AyaHashMap, IterableMap, MapError, PerCpuArray, PerCpuValues, RingBuf};
use aya::programs::Xdp;
use aya::programs::xdp::XdpMode;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::{AsFd, AsRawFd};
use uuid::Uuid;

/// XDP blocklist key — must stay byte-compatible with the C program's
/// `__u64[2]` map key. IPv4 occupies the low 32 bits of the first u64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct BlocklistKey(pub u128);

// Required by aya for eBPF map key types. #[repr(C)] POD only.
#[allow(unsafe_code)]
unsafe impl aya::Pod for BlocklistKey {}

/// Map value = absolute expiry ns on the monotonic clock (same clock as BPF's
/// bpf_ktime_get_ns; on Linux std::time::Instant is CLOCK_MONOTONIC since
/// boot). u64::MAX = permanent. P0: the old 1-byte value made aya's try_from
/// fail with InvalidValueSize → apply_block errored → XDP silently blocked
/// nothing; and a packed u32 expiry capped any block at 2^32 ns ≈ 4.3 s.
pub const PERMANENT: u64 = u64::MAX;

/// Encode a TTL in seconds into the map value. ttl_seconds = 0 → permanent.
fn blocklist_value(now_ns: u64, ttl_seconds: u64) -> u64 {
    if ttl_seconds == 0 {
        return PERMANENT;
    }
    now_ns.saturating_add(ttl_seconds.saturating_mul(1_000_000_000))
}

impl BlocklistKey {
    pub fn from_ip(ip: IpAddr) -> Self {
        match ip {
            // P0 fix: the kernel compares the RAW BYTES of this u128 against
            // the C program's key. C does `key[0] = ip->saddr` — an LE load
            // of the BE wire octets followed by an LE store back, which
            // round-trips to the ORIGINAL octet order: 1.2.3.4 -> memory
            // `01 02 03 04` + 12 zero bytes. The old value
            // u128::from(u32::from(v4)) serialized to `04 03 02 01` — byte-
            // reversed -> lookup NEVER hit -> XDP silently dropped nothing.
            // u32::from_ne_bytes(octets) puts the octets at memory[0..4],
            // matching C byte-for-byte on LE (all BPF targets we run:
            // x86_64/ARM; bpfel).
            IpAddr::V4(v4) => BlocklistKey(u128::from(u32::from_ne_bytes(v4.octets()))),
            // IPv6 plan Task 3 (G4): same P0 class as the v4 comment above.
            // The C program memcpy's the raw 16 saddr octets into the key, so
            // memory must equal wire order. from_be_bytes produced the value
            // with octets[0] as the MSB — serialized on LE that is REVERSED
            // bytes, every v6 lookup would miss forever. from_le_bytes is
            // the exact inverse of to_ne_bytes on LE (all BPF targets we run:
            // x86_64/ARM; bpfel), pinned by v6_key_bytes_match_dataplane_layout.
            // v6 keys go ONLY to BLOCKLIST6 (xdp_map_for) — never the v4 map.
            IpAddr::V6(v6) => BlocklistKey(u128::from_le_bytes(v6.octets())),
        }
    }
}

/// Map a v6 address must never share with a v4 key: the 16-byte v6 layout
/// can collide numerically with a v4-shaped key (D2), so isolation is by
/// map, not bytes. Single source of truth for routing (apply + reconcile).
fn xdp_map_for(ip: IpAddr) -> &'static str {
    match ip {
        IpAddr::V4(_) => "BLOCKLIST",
        IpAddr::V6(_) => "BLOCKLIST6",
    }
}

/// Task 5: split expected blocks per map, keyed. Pure — reconcile() keeps
/// only the map IO; the family routing (the part that leaked v6 keys into
/// the v4 sweep pre-fix) is unit-testable here.
fn split_by_family(expected_blocks: &[IpAddr]) -> (Vec<BlocklistKey>, Vec<BlocklistKey>) {
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

fn map_err(e: impl std::fmt::Display) -> EnforcementError {
    EnforcementError::Xdp(e.to_string())
}

// ── Raw BPF syscalls for O(1)-memory map reconciliation ──────────────
// aya's HashMap borrow-checker rules prevent iterating keys while
// mutating. We extract the raw fd (Copy integer) via IterableMap::map(),
// then call bpf(2) directly. The kernel's linked-list iteration skips
// deleted entries, so reusing the same prev_key after a delete advances
// correctly without restarting.

// 32-byte bpf_attr layout for BPF_MAP_*_ELEM (cmd 1/2/3/4).
// kernel: { map_fd:u32, pad:u32, key:u64, value_or_next:u64, flags:u64 }
// flags@24 MUST be within the buffer — kernel's bpf_check_uarg_tail_zero
// rejects buffers smaller than the expected union size.
const BPF_ELEM_ATTR_SIZE: usize = 32;

/// Build a zeroed 32-byte bpf_attr buffer for map elem operations.
#[inline(always)]
fn bpf_elem_attr(fd: std::os::fd::RawFd, key: u64, value_or_next: u64, flags: u64) -> [u8; 32] {
    let mut buf = [0u8; BPF_ELEM_ATTR_SIZE];
    buf[0..4].copy_from_slice(&(fd as u32).to_ne_bytes());
    buf[8..16].copy_from_slice(&key.to_ne_bytes());
    buf[16..24].copy_from_slice(&value_or_next.to_ne_bytes());
    buf[24..32].copy_from_slice(&flags.to_ne_bytes());
    buf
}

/// bpf_map_get_next_key — returns next key after `prev`, or None at end.
///
/// # Safety
/// `fd` must be a valid BPF map file descriptor.
#[allow(unsafe_code)]
unsafe fn raw_get_next_key<K: Copy>(
    fd: std::os::fd::RawFd,
    prev: Option<&K>,
) -> std::io::Result<Option<K>> {
    let key_ptr = prev.map_or(0, |k| std::ptr::from_ref(k) as u64);
    let mut next = std::mem::MaybeUninit::<K>::uninit();
    let next_ptr = next.as_mut_ptr() as u64;
    let attr = bpf_elem_attr(fd, key_ptr, next_ptr, 0);
    let ret = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            4i64, // BPF_MAP_GET_NEXT_KEY
            attr.as_ptr(),
            BPF_ELEM_ATTR_SIZE,
        )
    };
    if ret < 0 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::ENOENT) {
            return Ok(None);
        }
        return Err(e);
    }
    Ok(Some(unsafe { next.assume_init() }))
}

/// bpf_map_delete_elem — remove `key` from the map.
///
/// # Safety
/// `fd` must be a valid BPF map file descriptor.
#[allow(unsafe_code)]
unsafe fn raw_delete_elem<K: Copy>(fd: std::os::fd::RawFd, key: &K) -> std::io::Result<()> {
    let attr = bpf_elem_attr(fd, std::ptr::from_ref(key) as u64, 0, 0);
    let ret = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            3i64, // BPF_MAP_DELETE_ELEM
            attr.as_ptr(),
            BPF_ELEM_ATTR_SIZE,
        )
    };
    if ret < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Owns the loaded Bpf object and attached program.
pub struct AyaXdpApplier {
    bpf: Option<Ebpf>,
    iface: String,
    flags: XdpMode,
}

impl AyaXdpApplier {
    /// Build the applier without loading. `load_and_attach` does the syscall work.
    pub fn new(interface: &str, mode: &str) -> Self {
        let flags = if mode.eq_ignore_ascii_case("drv") {
            XdpMode::Driver
        } else {
            XdpMode::Skb
        };
        Self {
            bpf: None,
            iface: interface.to_string(),
            flags,
        }
    }

    /// Load ELF + attach + return. Errors surface verbatim for boot logging.
    pub fn load_and_attach(&mut self) -> Result<(), EnforcementError> {
        let mut bpf = Ebpf::load(ramshield_xdp::BPF_ELF).map_err(map_err)?;
        let program: &mut Xdp = bpf
            .program_mut("ramshield_xdp")
            .ok_or_else(|| EnforcementError::Xdp("program ramshield_xdp missing".into()))?
            .try_into()
            .map_err(|e| EnforcementError::Xdp(format!("program type: {e}")))?;
        program.load().map_err(map_err)?;
        program
            .attach(&self.iface, self.flags)
            .map_err(|e| EnforcementError::Xdp(format!("attach {}: {e}", self.iface)))?;
        self.bpf = Some(bpf);
        Ok(())
    }

    fn with_map<R>(
        &mut self,
        name: &str,
        f: impl FnOnce(
            &mut AyaHashMap<&mut aya::maps::MapData, BlocklistKey, u64>,
        ) -> Result<R, MapError>,
    ) -> Result<R, EnforcementError> {
        let bpf = self
            .bpf
            .as_mut()
            .ok_or_else(|| EnforcementError::Xdp("not loaded".into()))?;
        let map = bpf
            .map_mut(name)
            .ok_or_else(|| EnforcementError::Xdp(format!("{name} map missing")))?;
        let mut m: AyaHashMap<_, BlocklistKey, u64> = AyaHashMap::try_from(map).map_err(map_err)?;
        f(&mut m).map_err(map_err)
    }

    /// Read XDP per-CPU drop counters. Returns [v4_drop, v6_drop, pass, parse_fail]
    /// summed across all CPUs.
    pub fn counters(&mut self) -> Result<[u64; 4], EnforcementError> {
        let bpf = self
            .bpf
            .as_ref()
            .ok_or_else(|| EnforcementError::Xdp("not loaded".into()))?;
        let map = bpf
            .map("COUNTERS")
            .ok_or_else(|| EnforcementError::Xdp("COUNTERS map missing".into()))?;
        let array: PerCpuArray<&aya::maps::MapData, u64> =
            PerCpuArray::try_from(map).map_err(map_err)?;
        let mut totals = [0u64; 4];
        for (i, slot) in totals.iter_mut().enumerate() {
            let per_cpu: PerCpuValues<u64> = array.get(&(i as u32), 0).map_err(map_err)?;
            *slot = per_cpu.iter().sum();
        }
        Ok(totals)
    }

    /// Drain kernel→userspace drop notifications from the EVENTS ringbuf.
    /// Returns parsed drop events; empty when the channel is idle.
    pub fn drain_drop_events(&mut self) -> Vec<XdpDropEvent> {
        let Some(bpf) = self.bpf.as_mut() else {
            return Vec::new();
        };
        let Some(map) = bpf.map_mut("EVENTS") else {
            return Vec::new();
        };
        let mut ring = match RingBuf::try_from(map) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        while let Some(item) = ring.next() {
            if let Some(ev) = parse_drop_event(&item) {
                out.push(ev);
            }
        }
        out
    }
}

/// Parse a 26-byte EVENTS record into a drop event. Family from slot:
/// 0 = v4_drop, 1 = v6_drop (see BPF counter module); std::net interprets
/// the 16-byte key as IPv4 (first 4 bytes; trailing zero pad) or IPv6.
pub fn parse_drop_event(rec: &[u8]) -> Option<XdpDropEvent> {
    if rec.len() < 26 {
        return None;
    }
    let mut ipb = [0u8; 16];
    ipb.copy_from_slice(&rec[0..16]);
    let slot = rec[25];
    let ip = if slot == 0 {
        IpAddr::V4(Ipv4Addr::new(ipb[0], ipb[1], ipb[2], ipb[3]))
    } else {
        let mut o = [0u8; 16];
        o.copy_from_slice(&ipb);
        IpAddr::V6(Ipv6Addr::from(o))
    };
    Some(XdpDropEvent {
        ip,
        ts_ns: u64::from_le_bytes(rec[16..24].try_into().ok()?),
        slot,
    })
}

#[async_trait::async_trait]
impl XdpApplier for AyaXdpApplier {
    fn apply_block(
        &mut self,
        ip: IpAddr,
        _decision_id: Uuid,
        ttl_seconds: u64,
    ) -> Result<(), EnforcementError> {
        // CLOCK_MONOTONIC — same clock as BPF's bpf_ktime_get_ns, so the
        // absolute expiry is comparable in-kernel.
        let mut ts = std::mem::MaybeUninit::<libc::timespec>::uninit();
        let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, ts.as_mut_ptr()) };
        if rc != 0 {
            return Err(map_err(std::io::Error::last_os_error()));
        }
        let ts = unsafe { ts.assume_init() };
        let now_ns = (ts.tv_sec as u64) * 1_000_000_000 + ts.tv_nsec as u64;
        self.with_map(xdp_map_for(ip), |m| {
            m.insert(
                BlocklistKey::from_ip(ip),
                blocklist_value(now_ns, ttl_seconds),
                0,
            )
        })
    }

    fn apply_unblock(&mut self, ip: IpAddr, _decision_id: Uuid) -> Result<(), EnforcementError> {
        self.with_map(xdp_map_for(ip), |m| m.remove(&BlocklistKey::from_ip(ip)))
    }

    fn reconcile(
        &mut self,
        expected_blocks: &[IpAddr],
    ) -> Result<ReconciliationState, EnforcementError> {
        // IPv6 plan Task 3: the two maps are reconciled independently — each
        // drains its stale keys against its family's expected set only. The
        // old single-map sweep would have deleted every live v6 key when the
        // expected set was v4-only, and vice versa.
        let (v4_keys, v6_keys) = split_by_family(expected_blocks);
        for (name, expected) in [("BLOCKLIST", v4_keys), ("BLOCKLIST6", v6_keys)] {
            let expected: std::collections::HashSet<BlocklistKey> = expected.into_iter().collect();
            let mut stale_count = 0usize;
            self.with_map(name, |m| {
                // O(1)-memory reconcile: extract raw fd (Copy i32), then
                // iterate + delete via bpf(2). Kernel's linked-list
                // iteration skips deleted entries, so reusing prev_key
                // after a delete advances correctly.
                let fd_raw = m.map().fd().as_fd().as_raw_fd();
                let mut prev_key: Option<BlocklistKey> = None;
                loop {
                    let next = unsafe { raw_get_next_key(fd_raw, prev_key.as_ref()) };
                    match next {
                        Ok(Some(k)) => {
                            if !expected.contains(&k) {
                                unsafe { raw_delete_elem(fd_raw, &k) }.map_err(MapError::from)?;
                                stale_count += 1;
                                // keep prev_key — kernel skips deleted entry
                            } else {
                                prev_key = Some(k);
                            }
                        }
                        Ok(None) => break,
                        Err(e) => return Err(MapError::from(e)),
                    }
                }
                for k in &expected {
                    // Reconcile can't see per-block TTLs from a bare IP list;
                    // userspace owns expiry (removes on unblock). u64::MAX keeps
                    // the key blocking until explicitly removed — LRU eviction
                    // still bounds map growth. ponytail: pass (IpAddr, ttl) into
                    // reconcile if BPF-side expiry backup is ever needed.
                    m.insert(*k, PERMANENT, 0)?;
                }
                Ok(())
            })?;
            if stale_count > 0 {
                tracing::info!(
                    map = name,
                    stale = stale_count,
                    "XDP reconcile removed stale keys"
                );
            }
        }
        Ok(ReconciliationState::default())
    }

    fn drain_drop_events(&mut self) -> Vec<XdpDropEvent> {
        AyaXdpApplier::drain_drop_events(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    /// P0: ttl=0 must encode PERMANENT — a zero expiry would be instantly
    /// expired in-kernel (now < 0 never) and silently unblock everything.
    #[test]
    fn blocklist_value_ttl_zero_is_permanent() {
        assert_eq!(blocklist_value(1_000, 0), PERMANENT);
    }

    /// Value must be absolute expiry ns (not a duration) — BPF compares
    /// now < value directly against bpf_ktime_get_ns.
    #[test]
    fn blocklist_value_encodes_absolute_expiry() {
        assert_eq!(blocklist_value(5_000_000_000, 10), 15_000_000_000);
    }

    /// P0 regression: the kernel memcmp's the raw memory of BlocklistKey
    /// against the C program's `key[0] = ip->saddr` layout — wire octets in
    /// original order at bytes [0..4]. u32::from(v4) byte-reverses (04 03
    /// 02 01) so every lookup missed and XDP silently dropped nothing.
    #[test]
    fn v4_key_bytes_match_dataplane_layout() {
        let key = BlocklistKey::from_ip(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)));
        let mem = key.0.to_ne_bytes(); // aya Pod sends this exact memory
        assert_eq!(&mem[0..4], &[1, 2, 3, 4], "octet order reversed vs C");
        assert_eq!(&mem[4..], &[0; 12], "padding must be zero");
    }

    /// IPv6 plan Task 3 (G4): same P0 class as the v4 byte-reversal. The C
    /// program memcpy's the raw 16 saddr octets into the key; userspace must
    /// serialize the same memory. from_be_bytes put octets REVERSED on LE —
    /// every v6 lookup would miss forever.
    #[test]
    fn v6_key_bytes_match_dataplane_layout() {
        let key = BlocklistKey::from_ip("2001:db8::1".parse().unwrap());
        let mem = key.0.to_ne_bytes();
        assert_eq!(&mem[0..2], &[0x20, 0x01]);
        assert_eq!(&mem[2..4], &[0x0d, 0xb8]);
        assert_eq!(mem[15], 1, "last octet at memory[15] = wire order");
    }

    /// IPv6 plan D2: v6 keys must NEVER enter the v4 map (a 16-byte v6 key
    /// can collide numerically with a v4-shaped key), so isolation is by
    /// map, not bytes. Pins the routing both apply and reconcile use.
    #[test]
    fn family_routes_to_dedicated_map() {
        assert_eq!(super::xdp_map_for("1.2.3.4".parse().unwrap()), "BLOCKLIST");
        assert_eq!(
            super::xdp_map_for("2001:db8::1".parse().unwrap()),
            "BLOCKLIST6"
        );
    }

    /// Task 5: reconcile must never sweep one family's keys against the
    /// other's expected set — pins the split itself (map IO stays thin).
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

    // ===== EVENTS ringbuf record parsing =====

    fn rec(ip: &[u8; 16], ts_ns: u64, slot: u8) -> Vec<u8> {
        let mut r = Vec::with_capacity(26);
        r.extend_from_slice(ip);
        r.extend_from_slice(&ts_ns.to_ne_bytes());
        r.push(0); // action byte
        r.push(slot);
        r
    }

    /// P0: v4 drop event — slot 0 → parsed as a v4 address, ts + slot kept.
    #[test]
    fn parse_v4_drop_event() {
        let mut ip = [0u8; 16];
        ip[0..4].copy_from_slice(&[1, 2, 3, 4]);
        let ev = parse_drop_event(&rec(&ip, 42, 0)).unwrap();
        assert_eq!(ev.ip, "1.2.3.4".parse::<IpAddr>().unwrap());
        assert_eq!(ev.ts_ns, 42);
        assert_eq!(ev.slot, 0);
    }

    /// P0: v6 drop event — slot 1 → full 16-byte address.
    #[test]
    fn parse_v6_drop_event() {
        let ip = b"\x20\x01\x0d\xb8\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x01";
        let ev = parse_drop_event(&rec(ip, 7, 1)).unwrap();
        assert_eq!(ev.ip, "2001:db8::1".parse::<IpAddr>().unwrap());
        assert_eq!(ev.ts_ns, 7);
        assert_eq!(ev.slot, 1);
    }

    /// Truncated records (bad ringbuf framing) must not panic/crash the drain.
    #[test]
    fn parse_short_record_is_none() {
        assert!(parse_drop_event(&[0u8; 25]).is_none());
        assert!(parse_drop_event(&[]).is_none());
    }
}
