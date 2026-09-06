//! Real XDP dataplane via aya. Loads the clang-built BPF ELF, attaches to an
//! interface, and applies block/unblock decisions to the kernel BLOCKLIST map.
//!
//! Semantics (see .hermes/plans/2026-08-22_enforcement-production.md):
//! - fail-open at kernel: map update failure => Err, caller keeps in-band state
//! - reconcile(): drain map, delete stale keys, insert missing
//! - key contract: C stores `key[0] = saddr` (LE round-trip of BE octets =
//!   original octet order at memory[0..4]); Rust must serialize the same
//!   bytes — see BlocklistKey::from_ip (P0: u32::from_ne_bytes, NOT
//!   u32::from, which byte-reverses and makes every lookup miss).

use crate::{EnforcementError, ReconciliationState, XdpApplier};
use aya::Ebpf;
use aya::maps::{HashMap, MapError};
use aya::programs::Xdp;
use aya::programs::xdp::XdpMode;
use std::net::IpAddr;
use uuid::Uuid;

/// XDP blocklist key — must stay byte-compatible with the C program's
/// `__u64[2]` map key. IPv4 occupies the low 32 bits of the first u64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct BlocklistKey(pub u128);

// Required by aya for eBPF map key types. #[repr(C)] POD only.
#[allow(unsafe_code)]
unsafe impl aya::Pod for BlocklistKey {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct BlocklistValue(pub u8);

#[allow(unsafe_code)]
unsafe impl aya::Pod for BlocklistValue {}

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
            // ponytail: v6 keys are inserted but the C program never matches
            // them (ETH_P_IP branch only) — v6 stays enforced in-band until the
            // BPF program grows an ETH_P_IPV6 path.
            IpAddr::V6(v6) => BlocklistKey(u128::from_be_bytes(v6.octets())),
        }
    }
}

fn map_err(e: impl std::fmt::Display) -> EnforcementError {
    EnforcementError::Xdp(e.to_string())
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
        f: impl FnOnce(
            &mut HashMap<&mut aya::maps::MapData, BlocklistKey, BlocklistValue>,
        ) -> Result<R, MapError>,
    ) -> Result<R, EnforcementError> {
        let bpf = self
            .bpf
            .as_mut()
            .ok_or_else(|| EnforcementError::Xdp("not loaded".into()))?;
        let map = bpf
            .map_mut("BLOCKLIST")
            .ok_or_else(|| EnforcementError::Xdp("BLOCKLIST map missing".into()))?;
        let mut m: HashMap<_, BlocklistKey, BlocklistValue> =
            HashMap::try_from(map).map_err(map_err)?;
        f(&mut m).map_err(map_err)
    }
}

#[async_trait::async_trait]
impl XdpApplier for AyaXdpApplier {
    fn apply_block(&mut self, ip: IpAddr, _decision_id: Uuid) -> Result<(), EnforcementError> {
        self.with_map(|m| m.insert(BlocklistKey::from_ip(ip), BlocklistValue(1), 0))
    }

    fn apply_unblock(&mut self, ip: IpAddr, _decision_id: Uuid) -> Result<(), EnforcementError> {
        self.with_map(|m| m.remove(&BlocklistKey::from_ip(ip)))
    }

    fn reconcile(
        &mut self,
        expected_blocks: &[IpAddr],
    ) -> Result<ReconciliationState, EnforcementError> {
        let expected: std::collections::HashSet<BlocklistKey> = expected_blocks
            .iter()
            .map(|ip| BlocklistKey::from_ip(*ip))
            .collect();
        let mut stale_count = 0usize;
        self.with_map(|m| {
            let stale: Vec<BlocklistKey> = m
                .keys()
                .filter_map(|k| k.ok())
                .filter(|k| !expected.contains(k))
                .collect();
            for k in stale {
                m.remove(&k)?;
                stale_count += 1;
            }
            for k in &expected {
                m.insert(*k, BlocklistValue(1), 0)?;
            }
            Ok(())
        })?;
        if stale_count > 0 {
            tracing::info!(stale = stale_count, "XDP reconcile removed stale keys");
        }
        Ok(ReconciliationState::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

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
}
