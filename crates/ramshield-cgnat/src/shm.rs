#![allow(unsafe_code)]

//! High-Performance Shared Memory Rule Table with OS Fallback
//!
//! Provides a memory-mapped rule table for sub-20ns reverse-proxy
//! lookups and Shannon-entropy analysis to prevent blackholing
//! shared infrastructure (CGNAT / corporate proxy).
//!
//! Designed for integration with the ramshield-analytics crate
//! (SubnetHll for IPv6 cardinality, host_bitmap for IPv4).

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, AtomicU64, AtomicU8, Ordering};
use memmap2::{MmapMut, MmapOptions};

pub const SHM_TABLE_CAPACITY: usize = 65_536; // 64K rule slots
pub const SHM_SET_COUNT: usize = SHM_TABLE_CAPACITY / 2;
pub const FLAG_SHARED_INFRA: u8 = 0x01;

#[repr(C, align(64))]
pub struct ShmRuleEntry {
    pub client_hash: AtomicU64,   // 0 = Empty
    pub expires_at_ms: AtomicU64, // Absolute Unix epoch (ms)
    pub max_rps: AtomicU16,       // 0 = Block, >0 = Rate Limit
    pub tier: AtomicU8,           // 0: Allow, 1: 429, 2: Challenge, 3: XDP Drop
    pub flags: AtomicU8,          // Bit 0: Shared Infrastructure / CGNAT
    pub challenge_seed: [u8; 16],
    pub _padding: [u8; 30],       // Exact 64-byte alignment
}

pub struct ShmTableManager {
    _file: std::fs::File,
    mmap: MmapMut,
    pub path: PathBuf,
}

impl ShmTableManager {
    /// Selects /dev/shm if available (Linux), falling back to temp_dir (macOS/Windows/CI)
    pub fn default_path() -> PathBuf {
        let dev_shm = Path::new("/dev/shm");
        if dev_shm.exists() && dev_shm.is_dir() {
            dev_shm.join("ramshield_rules")
        } else {
            std::env::temp_dir().join("ramshield_rules")
        }
    }

    pub fn open_or_create(path: &Path) -> std::io::Result<Self> {
        let total_size = SHM_TABLE_CAPACITY * std::mem::size_of::<ShmRuleEntry>();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;

        file.set_len(total_size as u64)?;

        // SAFETY: File size is strictly enforced above. The mapping is shared with read-only proxies.
        let mmap = unsafe { MmapOptions::new().map_mut(&file)? };
        let manager = Self {
            _file: file,
            mmap,
            path: path.to_path_buf(),
        };
        manager.clear();
        Ok(manager)
    }

    #[inline(always)]
    pub fn get_slot(&self, index: usize) -> &ShmRuleEntry {
        let offset = (index & (SHM_TABLE_CAPACITY - 1)) * std::mem::size_of::<ShmRuleEntry>();
        // SAFETY: Offset is masked by (SHM_TABLE_CAPACITY - 1), guaranteeing bounds within mmap.
        // Alignment is guaranteed by repr(C, align(64)).
        unsafe { &*(self.mmap.as_ptr().add(offset) as *const ShmRuleEntry) }
    }

    /// SHM is an ephemeral acceleration layer, not durable state. Clear it
    /// when the daemon opens the file so rules from a previous process cannot
    /// override fresh WAL/store state after restart.
    pub fn clear(&self) {
        for index in 0..SHM_TABLE_CAPACITY {
            let slot = self.get_slot(index);
            slot.client_hash.store(0, Ordering::Release);
            slot.expires_at_ms.store(0, Ordering::Relaxed);
            slot.max_rps.store(0, Ordering::Relaxed);
            slot.tier.store(0, Ordering::Relaxed);
            slot.flags.store(0, Ordering::Relaxed);
        }
    }

    #[inline(always)]
    pub fn get_set_slot(&self, client_hash: u64, way: usize) -> &ShmRuleEntry {
        let set = (client_hash as usize) & (SHM_SET_COUNT - 1);
        self.get_slot(set * 2 + (way & 1))
    }

    /// Find a rule in either way of the set-associative table.
    pub fn find_rule(&self, client_hash: u64) -> Option<&ShmRuleEntry> {
        (0..2).map(|way| self.get_set_slot(client_hash, way)).find(|slot| {
            slot.client_hash.load(Ordering::Acquire) == client_hash
                && slot.expires_at_ms.load(Ordering::Acquire)
                    > epoch_ms()
        })
    }

    pub fn publish_rule(&self, client_hash: u64, ttl_ms: u64, tier: u8, max_rps: u16, is_shared: bool) {
        let now_ms = epoch_ms();
        let slots = [self.get_set_slot(client_hash, 0), self.get_set_slot(client_hash, 1)];
        let slot = slots
            .iter()
            .copied()
            .find(|slot| slot.client_hash.load(Ordering::Acquire) == client_hash)
            .or_else(|| {
                slots
                    .iter()
                    .copied()
                    .find(|slot| slot.client_hash.load(Ordering::Acquire) == 0)
            })
            .unwrap_or_else(|| {
                if slots[0].expires_at_ms.load(Ordering::Acquire)
                    <= slots[1].expires_at_ms.load(Ordering::Acquire)
                {
                    slots[0]
                } else {
                    slots[1]
                }
            });
        let flags = if is_shared { FLAG_SHARED_INFRA } else { 0 };

        slot.tier.store(tier, Ordering::Relaxed);
        slot.max_rps.store(max_rps, Ordering::Relaxed);
        slot.flags.store(flags, Ordering::Relaxed);
        slot.expires_at_ms
            .store(now_ms.saturating_add(ttl_ms), Ordering::Release);
        slot.client_hash.store(client_hash, Ordering::Release);
    }
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}