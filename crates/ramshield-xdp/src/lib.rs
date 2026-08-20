//! RamShield eBPF / XDP Data Plane Acceleration
//!
//! Kernel-level packet drop for known-bad IPs using XDP (eXpress Data Path).
//! Uses Aya (Rust eBPF library) to load and manage XDP programs.

use aya::{
    maps::HashMap,
    programs::Xdp,
    Bpf,
};
use ramshield_storage::Store;
use std::net::IpAddr;
use std::sync::Arc;
use thiserror::Error;

/// XDP-related errors.
#[derive(Debug, Error)]
pub enum XdpError {
    #[error("bpf error: {0}")]
    Bpf(#[from] aya::BpfError),
    #[error("map error: {0}")]
    Map(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("verifier error: {0}")]
    Verifier(String),
    #[error("program error: {0}")]
    Program(#[from] aya::programs::ProgramError),
    #[error("map error: {0}")]
    MapError(#[from] aya::maps::MapError),
}

/// XDP blocklist key (IPv4 or IPv6 packed as u128 for BPF map).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct BlocklistKey(pub u128);

// Required by aya for eBPF map key types. Safe because BlocklistKey is #[repr(C)] and contains only POD data.
#[allow(unsafe_code)]
unsafe impl aya::Pod for BlocklistKey {}

// XDP map entry value, needs to be Pod
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct BlocklistValue(pub u8);

// Required by aya for eBPF map value types. Safe because BlocklistValue is #[repr(C)] and contains only POD data.
#[allow(unsafe_code)]
unsafe impl aya::Pod for BlocklistValue {}

impl BlocklistKey {
    pub fn from_ip(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(v4) => BlocklistKey(u128::from(u32::from(v4))),
            IpAddr::V6(v6) => BlocklistKey(u128::from_be_bytes(v6.octets())),
        }
    }
}

/// High-level XDP manager for loading/unloading programs and syncing blocklist.
pub struct XdpManager {
    bpf: Option<Bpf>,
    _iface: String,
    store: Arc<Store>,
}

impl XdpManager {
    /// Create new XDP manager for a network interface.
    pub fn new(iface: String, store: Arc<Store>) -> Self {
        Self {
            bpf: None,
            _iface: iface,
            store,
        }
    }

    /// Load and attach XDP program to interface.
    pub fn load(&mut self) -> Result<(), XdpError> {
        // Load the compiled eBPF bytecode
        let mut bpf = Bpf::load(aya::include_bytes_aligned!(concat!(
            env!("OUT_DIR"),
            "/ramshield-xdp"
        )))?;

        // Get XDP program
        let program: &mut Xdp = bpf.program_mut("ramshield_xdp").unwrap().try_into()?;
        program.load()?;

        // Attach to interface
        program.attach(&self._iface, aya::programs::XdpFlags::default())
            .map_err(|e| XdpError::Verifier(format!("attach failed: {}", e)))?;

        // Initialize blocklist map
        let mut blocklist: HashMap<aya::maps::MapData, BlocklistKey, BlocklistValue> =
            HashMap::try_from(bpf.take_map("BLOCKLIST").unwrap())?;
        
        // Pre-populate from store
        self.sync_blocklist(&mut blocklist)?;

        self.bpf = Some(bpf);
        tracing::info!(iface = %self._iface, "XDP program loaded and attached");
        Ok(())
    }

    /// Sync in-memory blocklist with storage layer (run once on startup).
    pub fn sync_blocklist(&self, blocklist: &mut HashMap<aya::maps::MapData, BlocklistKey, BlocklistValue>) -> Result<(), XdpError> {
        let blocked_ips = self.store.get_all_blocked_ips();
        for ip in blocked_ips {
            let key = BlocklistKey::from_ip(ip);
            blocklist.insert(key, BlocklistValue(1), 0)?;
        }
        Ok(())
    }

    /// Apply a single block decision to the XDP map.
    pub fn apply_block_decision(&mut self, ip: IpAddr) -> Result<(), XdpError> {
        if let Some(bpf) = self.bpf.as_mut() {
            let mut blocklist: HashMap<_, BlocklistKey, BlocklistValue> = 
                HashMap::try_from(bpf.map_mut("BLOCKLIST").ok_or_else(|| XdpError::Map("BLOCKLIST not found".to_string()))?)?;
            let key = BlocklistKey::from_ip(ip);
            blocklist.insert(key, BlocklistValue(1), 0)?;
        }
        Ok(())
    }

    /// Remove a single IP from the XDP blocklist.
    pub fn remove_block(&mut self, ip: IpAddr) -> Result<(), XdpError> {
        if let Some(bpf) = self.bpf.as_mut() {
            let mut blocklist: HashMap<_, BlocklistKey, BlocklistValue> = 
                HashMap::try_from(bpf.map_mut("BLOCKLIST").ok_or_else(|| XdpError::Map("BLOCKLIST not found".to_string()))?)?;
            let key = BlocklistKey::from_ip(ip);
            blocklist.remove(&key)?;
        }
        Ok(())
    }

    /// Detach and unload XDP program.
    pub fn unload(&mut self) -> Result<(), XdpError> {
        if let Some(bpf) = self.bpf.take() {
            // Programs auto-detach on drop
            drop(bpf);
            tracing::info!(iface = %self._iface, "XDP program unloaded");
        }
        Ok(())
    }
}
#[cfg(test)]
mod runtime_tests {
    use super::*;
    use std::fs;

    #[test]
    #[ignore = "requires CAP_BPF and CAP_NET_ADMIN capabilities"]
    fn test_bpf_object_loads() {
        let bpf_path = concat!(env!("OUT_DIR"), "/ramshield-xdp");
        let bytes = fs::read(bpf_path).expect("failed to read BPF object");
        println!("Loaded {} bytes from BPF object", bytes.len());
        
        match Bpf::load(&bytes) {
            Ok(bpf) => {
                println!("✓ BPF object loaded successfully");
                let programs: Vec<_> = bpf.programs().map(|(k,_)| k.clone()).collect();
                let maps: Vec<_> = bpf.maps().map(|(k,_)| k.clone()).collect();
                println!("Programs: {:?}", programs);
                println!("Maps: {:?}", maps);
                assert!(programs.iter().any(|p| *p == "ramshield_xdp"), "Missing ramshield_xdp program");
                assert!(maps.iter().any(|m| *m == "BLOCKLIST"), "Missing BLOCKLIST map");
            }
            Err(e) => {
                panic!("✗ Failed to load BPF object: {}", e);
            }
        }
    }
}
