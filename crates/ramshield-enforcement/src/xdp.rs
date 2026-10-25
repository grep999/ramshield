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
use aya::maps::{
    Array as AyaArray, HashMap as AyaHashMap, IterableMap, MapError, PerCpuArray, PerCpuValues,
    RingBuf,
    lpm_trie::{Key as LpmKey, LpmTrie},
};
use aya::programs::Xdp;
use aya::programs::xdp::XdpMode;
use ramshield_types::{BlocklistKey, IpNetwork, cidr_map_for, split_by_family, xdp_map_for};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::{AsFd, AsRawFd};

// SAFETY: BlocklistKey is `#[repr(C, align(8))]` and contains only a
// `[u8; 16]`; it has no uninitialized bytes or invalid bit patterns.
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

/// Map a v6 address must never share with a v4 key: the 16-byte v6 layout
/// can collide numerically with a v4-shaped key (D2), so isolation is by
/// map, not bytes. Single source of truth for routing (apply + reconcile).
///
/// Note: Shared route functions (xdp_map_for, cidr_map_for, split_by_family)
/// live in ramshield-types to prevent drift between enforcement and xdp crates.
/// This module uses them via `use ramshield_types::...`.

fn cidr_key(network: IpNetwork) -> LpmKey<[u64; 2]> {
    let bytes = BlocklistKey::from_ip(network.addr).0;
    // SAFETY: BlocklistKey is #[repr(C, align(8))] with a 16-byte payload.
    // 16 bytes fits exactly in two u64s (data.len() == 16), and both pointers
    // are valid for 16 bytes. Nonoverlapping: data is a fresh local and bytes
    // is an owned field — no overlap.
    let mut data = [0u64; 2];
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data.as_mut_ptr() as *mut u8, 16);
    }
    LpmKey::new(u32::from(network.prefix_len), data)
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
/// `fd` must be a valid BPF map file descriptor whose key size equals
/// `size_of::<K>()`. Every byte pattern the kernel can return for a key
/// must be a valid initialized representation of `K`, because successful
/// syscalls write raw map-key bytes into `MaybeUninit<K>` before
/// `assume_init` is called.
unsafe fn raw_get_next_key<K: Copy>(
    fd: std::os::fd::RawFd,
    prev: Option<&K>,
) -> std::io::Result<Option<K>> {
    let key_ptr = prev.map_or(0, |k| std::ptr::from_ref(k) as u64);
    let mut next = std::mem::MaybeUninit::<K>::uninit();
    let next_ptr = next.as_mut_ptr() as u64;
    let attr = bpf_elem_attr(fd, key_ptr, next_ptr, 0);
    // SAFETY: `attr` is a live, fully initialized 32-byte bpf_attr buffer.
    // `key_ptr` is null or points to a valid `K`, and `next_ptr` points to
    // writable storage for one `K`; both outlive the syscall. The caller
    // guarantees `fd` is a BPF map whose key size matches `K`.
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
    // SAFETY: BPF_MAP_GET_NEXT_KEY succeeded, so the kernel wrote a complete
    // key of `size_of::<K>()` bytes into `next`; `K: Copy` has no drop glue.
    Ok(Some(unsafe { next.assume_init() }))
}

/// What a reconcile must do to the kernel map, derived from a single
/// enumeration of the map and the expected key set.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ReconcilePlan {
    /// Resident keys that are not expected: these must be removed.
    pub stale: Vec<BlocklistKey>,
    /// Expected keys that are not resident: these must be inserted.
    ///
    /// Only the ABSENT expected keys belong here. Re-inserting an already
    /// resident key costs one `bpf_map_update_elem` syscall per blocked IP
    /// per reconcile cycle and takes the very bucket lock the XDP data path
    /// takes on every packet, so rewriting the whole map on every cycle
    /// competes with packet processing for the same locks.
    pub missing: Vec<BlocklistKey>,
}

/// Enumerate the map once and compute the reconcile plan.
///
/// The single walk yields both halves of the plan:
///
/// * every resident key that is not in `expected` is `stale` (delete it);
/// * every key in `expected` that the walk did not see is `missing`
///   (insert it).
///
/// Because the walk already visits every resident key, the absent expected
/// keys are exactly `expected` minus what was seen — so the plan costs one
/// syscall per resident key and nothing more, and it never rewrites a key
/// that is already correct.
fn plan_reconcile<F>(
    mut next_key: F,
    expected: &std::collections::HashSet<BlocklistKey>,
) -> std::io::Result<ReconcilePlan>
where
    F: FnMut(Option<&BlocklistKey>) -> std::io::Result<Option<BlocklistKey>>,
{
    let mut prev_key: Option<BlocklistKey> = None;
    let mut plan = ReconcilePlan::default();
    let mut seen: std::collections::HashSet<BlocklistKey> =
        std::collections::HashSet::with_capacity(expected.len());
    loop {
        let next = next_key(prev_key.as_ref())?;
        match next {
            Some(k) => {
                if expected.contains(&k) {
                    // Resident AND expected: nothing to do. Remember that we
                    // saw it so it is not scheduled for insertion.
                    seen.insert(k);
                } else {
                    // Resident but not expected: stage for deferred deletion.
                    plan.stale.push(k);
                }
                // Advance the cursor past EVERY key we visit, including the
                // staged ones — see the doc comment on `plan_reconcile`.
                prev_key = Some(k);
            }
            None => break,
        }
    }
    plan.missing = expected
        .iter()
        .filter(|k| !seen.contains(*k))
        .copied()
        .collect();
    Ok(plan)
}

/// bpf_map_delete_elem — remove `key` from the map.
///
/// # Safety
/// `fd` must be a valid BPF map file descriptor whose key size equals
/// `size_of::<K>()`.
unsafe fn raw_delete_elem<K: Copy>(fd: std::os::fd::RawFd, key: &K) -> std::io::Result<()> {
    let attr = bpf_elem_attr(fd, std::ptr::from_ref(key) as u64, 0, 0);
    // SAFETY: `attr` is a live, fully initialized 32-byte bpf_attr buffer and
    // its key pointer references `key`, which is valid for the whole syscall.
    // The caller guarantees `fd` is a BPF map whose key size matches `K`.
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
        // `native` is the user-facing alias for the driver's/native XDP mode.
        // Never silently downgrade an explicitly requested native mode to SKB.
        let flags = if mode.eq_ignore_ascii_case("drv") || mode.eq_ignore_ascii_case("native") {
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

    fn with_cidr_map<R>(
        &mut self,
        name: &str,
        f: impl FnOnce(&mut LpmTrie<&mut aya::maps::MapData, [u64; 2], u8>) -> Result<R, MapError>,
    ) -> Result<R, EnforcementError> {
        let bpf = self
            .bpf
            .as_mut()
            .ok_or_else(|| EnforcementError::Xdp("not loaded".into()))?;
        let map = bpf
            .map_mut(name)
            .ok_or_else(|| EnforcementError::Xdp(format!("{name} map missing")))?;
        let mut trie = LpmTrie::try_from(map).map_err(map_err)?;
        f(&mut trie).map_err(map_err)
    }

    pub fn configure_autonomous(
        &mut self,
        enabled: bool,
        syn_pps_per_cpu: u64,
        udp_pps_per_cpu: u64,
        packet_pps_per_cpu: u64,
        window_ms: u64,
    ) -> Result<(), EnforcementError> {
        let bpf = self
            .bpf
            .as_mut()
            .ok_or_else(|| EnforcementError::Xdp("not loaded".into()))?;
        let map = bpf
            .map_mut("AUTONOMOUS_CONFIG")
            .ok_or_else(|| EnforcementError::Xdp("AUTONOMOUS_CONFIG map missing".into()))?;
        let mut cfg: AyaArray<_, [u64; 4]> = AyaArray::try_from(map).map_err(map_err)?;
        let values = if enabled {
            [
                syn_pps_per_cpu,
                udp_pps_per_cpu,
                packet_pps_per_cpu,
                window_ms,
            ]
        } else {
            [0, 0, 0, 0]
        };
        cfg.set(0, values, 0).map_err(map_err)
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
    fn configure_trusted_overlay(&mut self, cidrs: &[IpNetwork]) -> Result<(), EnforcementError> {
        if cidrs.is_empty() {
            return Ok(());
        }
        // B01: Fail closed when the map is unavailable. Returning Ok(()) here
        // would silently accept all traffic as trusted, which is a security
        // control failure. The trait dispatch must not report success for an
        // inactive control.
        Err(EnforcementError::Xdp(
            "trusted overlay map not compiled in; cannot configure CIDR blocklist".into(),
        ))
    }

    fn configure_autonomous(
        &mut self,
        enabled: bool,
        syn_pps_per_cpu: u64,
        udp_pps_per_cpu: u64,
        packet_pps_per_cpu: u64,
        window_ms: u64,
    ) -> Result<(), EnforcementError> {
        // B01: delegate to the inherent method, which programs the real
        // AUTONOMOUS_CONFIG map and surfaces "not loaded"/"map missing" as
        // errors. Previously this logged a warning and returned Ok(()),
        // reporting success for a control the boot path never programmed.
        AyaXdpApplier::configure_autonomous(
            self,
            enabled,
            syn_pps_per_cpu,
            udp_pps_per_cpu,
            packet_pps_per_cpu,
            window_ms,
        )
    }

    fn apply_block(
        &mut self,
        ip: IpAddr,
        _decision_id: Uuid,
        ttl_seconds: u64,
    ) -> Result<(), EnforcementError> {
        // CLOCK_MONOTONIC — same clock as BPF's bpf_ktime_get_ns, so the
        // absolute expiry is comparable in-kernel.
        let mut ts = std::mem::MaybeUninit::<libc::timespec>::uninit();
        // SAFETY: `ts.as_mut_ptr()` points to writable storage for one
        // `timespec`, and CLOCK_MONOTONIC is a valid clock id.
        let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, ts.as_mut_ptr()) };
        if rc != 0 {
            return Err(map_err(std::io::Error::last_os_error()));
        }
        // SAFETY: ts was written by clock_gettime — rc == 0 means initialization succeeded.
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

    fn apply_cidr_block(
        &mut self,
        network: IpNetwork,
        _decision_id: Uuid,
        _ttl_seconds: u64,
    ) -> Result<(), EnforcementError> {
        let key = cidr_key(network);
        self.with_cidr_map(cidr_map_for(network), |m| m.insert(&key, 1u8, 0))
    }

    fn apply_cidr_unblock(
        &mut self,
        network: IpNetwork,
        _decision_id: Uuid,
    ) -> Result<(), EnforcementError> {
        let key = cidr_key(network);
        self.with_cidr_map(cidr_map_for(network), |m| m.remove(&key))
    }

    fn reconcile(
        &mut self,
        expected_blocks: &[IpAddr],
        expected_cidrs: &[IpNetwork],
    ) -> Result<ReconciliationState, EnforcementError> {
        // IPv6 plan Task 3: the two maps are reconciled independently — each
        // drains its stale keys against its family's expected set only. The
        // old single-map sweep would have deleted every live v6 key when the
        // expected set was v4-only, and vice versa.
        let (v4_keys, v6_keys) = split_by_family(expected_blocks);
        let mut evicted_count: u64 = 0;
        for (name, expected) in [("BLOCKLIST", v4_keys), ("BLOCKLIST6", v6_keys)] {
            let expected: std::collections::HashSet<BlocklistKey> = expected.into_iter().collect();
            let stale_count = self.with_map(name, |m| {
                let fd_raw = m.map().fd().as_fd().as_raw_fd();
                // STAGED DELETION BUFFER: decouple enumeration from deletion
                // to avoid race conditions where in-loop deletion interrupts
                // raw_get_next_key iteration and returns ENOENT.
                let plan = plan_reconcile(
                    |prev| {
                        // SAFETY: `fd_raw` comes from the BLOCKLIST/BLOCKLIST6 map
                        // borrowed as `m` for this whole closure, so it stays open;
                        // both maps use 16-byte keys matching `BlocklistKey`.
                        unsafe { raw_get_next_key(fd_raw, prev) }
                    },
                    &expected,
                )
                .map_err(MapError::from)?;
                // Execute deletions after collection to avoid race conditions.
                let stale_count = plan.stale.len();
                for k in plan.stale.iter() {
                    // SAFETY: same live map fd and 16-byte key layout as before.
                    unsafe { raw_delete_elem(fd_raw, k) }.map_err(MapError::from)?;
                }
                // Insert ONLY the expected keys that are not already resident.
                // Re-inserting a resident key costs one `bpf_map_update_elem`
                // syscall per blocked IP per cycle and takes the same bucket
                // lock the XDP data path takes on every packet — at a large
                // blocklist that makes the periodic reconcile a competitor of
                // the data plane for no benefit, since the key is already
                // correct.
                //
                // VALUE SEMANTICS. The map value is the ABSOLUTE EXPIRY
                // (`blocklist_value`: now + ttl, or PERMANENT for ttl == 0),
                // on the same CLOCK_MONOTONIC the BPF program reads with
                // `bpf_ktime_get_ns`, and the program drops a packet only
                // while `now < expiry`. A resident key therefore keeps the
                // expiry `apply_block` gave it, and the reconcile no longer
                // rewrites it to PERMANENT.
                //
                // That is consistent with the rest of the system:
                //
                // * `apply_block` writes the real expiry;
                // * the keep-alive re-runs the whole enforce path at the
                //   cooldown boundary (ttl/2), which re-applies the block
                //   and so refreshes the kernel expiry before it passes;
                // * `expire_due` reaps the block at the same TTL and issues
                //   the unblock that removes the map entry.
                //
                // The old PERMANENT rewrite was therefore redundant on the
                // normal path. The one behaviour difference is the corner
                // case where the keep-alive is missed: the block now lapses
                // at its TTL instead of the reconcile extending it
                // indefinitely, which is what a TTL is for.
                for k in plan.missing.iter() {
                    m.insert(*k, PERMANENT, 0)?;
                }
                Ok(stale_count)
            })?;
            if stale_count > 0 {
                evicted_count += stale_count as u64;
                tracing::info!(
                    map = name,
                    stale = stale_count,
                    "XDP reconcile removed stale keys"
                );
            }
        }
        // CIDR state has its own LPM-trie maps — reconcile both families.
        let (v4_cidrs, v6_cidrs): (Vec<_>, Vec<_>) = expected_cidrs
            .iter()
            .partition(|n| matches!(n.addr, IpAddr::V4(_)));
        for (name, expected) in [("BLOCKCIDR", v4_cidrs), ("BLOCKCIDR6", v6_cidrs)] {
            let expected: Vec<LpmKey<[u64; 2]>> = expected
                .into_iter()
                .map(|network: &IpNetwork| cidr_key(*network))
                .collect();
            // Membership test via a hash set, not a linear scan.
            //
            // The old code ran `expected.iter().any(...)` once per resident
            // key, so the CIDR reconcile was O(resident x expected) — quadratic.
            // At a few thousand configured CIDRs that is millions of
            // comparisons per reconcile cycle, and the reconcile cycle runs
            // every 10 seconds. The key comparison is a prefix-length plus
            // data equality, so it hashes cleanly.
            // `LpmKey` is `#[repr(C, packed)]` and derives neither `Hash` nor
            // `Eq`, so the set is keyed by the plain tuple of the two fields
            // the comparison used: the prefix length and the address data.
            let expected_set: std::collections::HashSet<(u32, [u64; 2])> = expected
                .iter()
                .map(|k| (k.prefix_len(), k.data()))
                .collect();
            let is_expected =
                |k: &LpmKey<[u64; 2]>| expected_set.contains(&(k.prefix_len(), k.data()));
            self.with_cidr_map(name, |trie| {
                let current: Vec<LpmKey<[u64; 2]>> = trie.keys().collect::<Result<Vec<_>, _>>()?;
                for key in current {
                    if !is_expected(&key) {
                        trie.remove(&key)?;
                    }
                }
                for key in &expected {
                    trie.insert(key, 1u8, 0)?;
                }
                Ok(())
            })?;
        }

        Ok(ReconciliationState {
            last_wal_lsn: 0,
            pending_blocks: Vec::new(),
            pending_unblocks: Vec::new(),
            evicted_count,
        })
    }

    fn drain_drop_events(&mut self) -> Vec<XdpDropEvent> {
        AyaXdpApplier::drain_drop_events(self)
    }

    fn counters(&mut self) -> Result<[u64; 4], EnforcementError> {
        AyaXdpApplier::counters(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    /// P0: ttl=0 must encode PERMANENT — a zero expiry would be instantly
    /// expired in-kernel (now < 0 never) and silently unblock everything.
    #[test]
    fn native_xdp_mode_is_not_silently_downgraded() {
        assert_eq!(AyaXdpApplier::new("eth0", "native").flags, XdpMode::Driver);
        assert_eq!(AyaXdpApplier::new("eth0", "drv").flags, XdpMode::Driver);
        assert_eq!(AyaXdpApplier::new("eth0", "skb").flags, XdpMode::Skb);
    }

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
    /// original order at bytes [0..4]. Now uses BlocklistKey(pub [u8; 16]),
    /// so insertion and lookup copy wire bytes verbatim with zero arithmetic.
    #[test]
    fn v4_key_bytes_match_dataplane_layout() {
        // Test IP: 1.2.3.4 -> Hex wire bytes: [1, 2, 3, 4]
        let key = BlocklistKey::from_ip(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)));
        // 1. Assert first 4 bytes match the wire octets exactly
        assert_eq!(
            &key.0[0..4],
            &[1, 2, 3, 4],
            "Octet order must match C wire order"
        );
        // 2. Assert remaining 12 bytes are strictly zeroed padding
        assert_eq!(
            &key.0[4..16],
            &[0u8; 12],
            "Trailing 12 bytes must be zeroed padding for IPv4"
        );
        // 3. Simulate raw packet header copy from main.rs:188
        let simulated_packet_header: [u8; 4] = [1, 2, 3, 4];
        let mut dataplane_key = BlocklistKey::zeroed();
        dataplane_key.0[0..4].copy_from_slice(&simulated_packet_header);
        // PROOF OF INVARIANCE: Both must be bitwise identical in memory
        assert_eq!(
            key.0, dataplane_key.0,
            "Userspace and dataplane keys must be bitwise identical"
        );
    }

    /// IPv6 plan Task 3 (G4): same P0 class as the v4 byte-reversal. The C
    /// program memcpy's the raw 16 saddr octets into the key; userspace must
    /// serialize the same memory.
    #[test]
    fn v6_key_bytes_match_dataplane_layout() {
        let key = BlocklistKey::from_ip("2001:db8::1".parse().unwrap());
        assert_eq!(&key.0[0..2], &[0x20, 0x01]);
        assert_eq!(&key.0[2..4], &[0x0d, 0xb8]);
        assert_eq!(key.0[15], 1, "last octet at key[15] = wire order");
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

    #[test]
    fn cidr_routes_to_dedicated_map() {
        let v4 = IpNetwork::new("10.0.0.0".parse().unwrap(), 8).unwrap();
        let v6 = IpNetwork::new("2001:db8::".parse().unwrap(), 32).unwrap();
        assert_eq!(super::cidr_map_for(v4), "BLOCKCIDR");
        assert_eq!(super::cidr_map_for(v6), "BLOCKCIDR6");
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

    /// A 16-byte blocklist key built from a u32 tag, in the same byte order
    /// the kernel map uses.
    fn bkey(tag: u32) -> BlocklistKey {
        let mut raw = [0u8; 16];
        raw[0..4].copy_from_slice(&tag.to_ne_bytes());
        BlocklistKey(raw)
    }

    /// P0: the reconcile enumeration must terminate when the map holds a key
    /// that is not in the expected set.
    ///
    /// A stale key is staged for deferred deletion, so it is STILL RESIDENT
    /// in the map while the enumeration runs. If the cursor is not advanced
    /// past it, `bpf_map_get_next_key` returns that same key again on every
    /// call and the loop never ends — the enforcement control plane wedges
    /// and the staging buffer grows without bound.
    ///
    /// `next_key` below models `bpf_map_get_next_key` (next resident key
    /// after the cursor, `None` at the end of the map). It is a model of the
    /// KERNEL SYSCALL, not of the code under test: the enumeration logic
    /// being exercised here is the production function.
    /// The reconcile must INSERT only the expected keys that are absent from
    /// the map.
    ///
    /// The old code re-inserted every expected key on every reconcile cycle:
    /// one `bpf_map_update_elem` syscall per blocked IP per cycle, each one
    /// taking the very bucket lock the XDP data path takes on every packet.
    /// At a large blocklist that turns the periodic reconcile into a
    /// data-plane competitor, and it is pure waste — the key is already
    /// correct.
    #[test]
    fn reconcile_plan_inserts_only_absent_expected_keys() {
        let resident = [bkey(0x1000), bkey(0x1001)];
        let expected: std::collections::HashSet<BlocklistKey> =
            [bkey(0x1000), bkey(0x1002)].into_iter().collect();

        let mut calls = 0usize;
        let mut next_key = |prev: Option<&BlocklistKey>| -> std::io::Result<Option<BlocklistKey>> {
            calls += 1;
            if calls > 64 {
                return Err(std::io::Error::other("enumeration did not terminate"));
            }
            Ok(match prev {
                None => Some(resident[0]),
                Some(p) if *p == resident[0] => Some(resident[1]),
                _ => None,
            })
        };

        let plan = plan_reconcile(&mut next_key, &expected).expect("plan");

        assert_eq!(
            plan.stale,
            vec![bkey(0x1001)],
            "only the resident non-expected key is stale"
        );
        assert_eq!(
            plan.missing,
            vec![bkey(0x1002)],
            "only the absent expected key is missing"
        );
        assert!(
            !plan.missing.contains(&bkey(0x1000)),
            "a resident expected key must not be re-inserted"
        );
    }

    /// The cursor MUST advance past every key we visit, including the staged
    /// ones.
    ///
    /// A staged key is still resident in the map — that is the entire point of
    /// the staged-deletion buffer — so leaving the cursor pointing at the
    /// previous key makes `bpf_map_get_next_key` return the same key on every
    /// call and the enumeration never terminates. The old reconcile loop
    /// advanced the cursor only for keys that were still expected, so a single
    /// resident stale key hung the enforcement control plane and grew the
    /// staging buffer without bound.
    #[test]
    fn reconcile_enumeration_terminates_with_a_stale_key() {
        let resident = [bkey(0x1000), bkey(0x1001)];
        // Only the first resident key is expected; the second is stale.
        let expected: std::collections::HashSet<BlocklistKey> =
            [bkey(0x1000)].into_iter().collect();

        let mut calls = 0usize;
        let mut next_key = |prev: Option<&BlocklistKey>| -> std::io::Result<Option<BlocklistKey>> {
            calls += 1;
            if calls > 64 {
                return Err(std::io::Error::other("enumeration did not terminate"));
            }
            Ok(match prev {
                None => Some(resident[0]),
                Some(p) if *p == resident[0] => Some(resident[1]),
                _ => None,
            })
        };

        let plan = plan_reconcile(&mut next_key, &expected).expect("plan");

        assert!(
            calls <= 8,
            "the enumeration must terminate, took {calls} calls"
        );
        assert_eq!(
            plan.stale,
            vec![bkey(0x1001)],
            "the stale resident key must be staged for deletion"
        );
        assert!(
            plan.missing.is_empty(),
            "the expected key is already resident, nothing to insert"
        );
    }
}
