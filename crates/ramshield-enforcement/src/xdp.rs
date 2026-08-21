//! Real XDP dataplane via aya. Loads the clang-built BPF ELF, attaches to an
//! interface, and applies block/unblock decisions to the kernel BLOCKLIST map.
//!
//! Semantics (see .hermes/plans/2026-08-22_enforcement-production.md):
//! - fail-open at kernel: map update failure => Err, caller keeps in-band state
//! - reconcile(): drain map, delete stale keys, insert missing
//! - key contract: C stores `key[0] = saddr` as LE u64; Rust BlocklistKey(u128)
//!   from u32 v4 is byte-identical in the first 8 bytes.

use crate::{EnforcementError, ReconciliationState, XdpApplier};
use aya::maps::{HashMap, MapError};
use aya::programs::xdp::XdpMode;
use aya::programs::Xdp;
use aya::Ebpf;
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
            // u32::from(v4) is host-order integer of the BE octets; identical to
            // the register value the C side reads from ip->saddr.
            IpAddr::V4(v4) => BlocklistKey(u128::from(u32::from(v4))),
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

    pub fn detach(&mut self) {
        // Dropping Bpf detaches the program and pins nothing.
        if self.bpf.take().is_some() {
            tracing::info!(iface = %self.iface, "XDP detached");
        }
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
