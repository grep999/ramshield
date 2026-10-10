//! RamShield WAL adapter over RAMWAL.
//!
//! RamShield owns the semantic record (`WalEntry`) and the snapshot; RAMWAL
//! owns durability, recovery, segment layout and retention. This module is a
//! thin bridge: encode/decode + error mapping + checkpoint protocol. It must
//! NOT contain a second segment writer, CRC, fsync, rotation, recovery
//! scanner, retention algorithm, LSN allocator or MANIFEST implementation.

use ramshield_types::{Durability, IpNetwork, Result, RsError};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use ramwal::{
    Compression, Config, Durability as RamWalDurability, Error as RamWalError, Lsn, Wal as RamWal,
};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum WalEntry {
    BlockIp {
        ip: String,
        reason: String,
        ttl_secs: Option<u64>,
        ts_ns: u64,
    },
    UnblockIp {
        ip: String,
        ts_ns: u64,
    },
    BlockCidr {
        cidr: IpNetwork,
        reason: String,
        ttl_secs: Option<u64>,
        ts_ns: u64,
    },
    UnblockCidr {
        cidr: IpNetwork,
        ts_ns: u64,
    },
    Insert {
        key: String,
        value_json: String,
        ttl_secs: Option<u64>,
        ts_ns: u64,
    },
    Delete {
        key: String,
        ts_ns: u64,
    },
    Checkpoint {
        snapshot_path: String,
        ts_ns: u64,
    },
}

pub struct CheckpointBoundary {
    /// The LSN this boundary represents (entries <= this are in the snapshot).
    pub lsn: u64,
}

impl CheckpointBoundary {
    pub fn boundary_lsn(&self) -> u64 {
        self.lsn
    }
}

/// RAMWAL adapter — interior-mutable via `Mutex<Option<Arc<RamWal>>>`.
/// The guard is poisoned on rotation failure; `reopen()` replaces it.
pub struct Wal {
    guard: Guard,
}

struct Guard {
    inner: Mutex<Option<Arc<RamWal>>>,
    cfg: Config,
}

impl Guard {
    fn new(cfg: Config) -> Result<Self> {
        let inner = Some(Arc::new(RamWal::open(cfg.clone()).map_err(map_error)?));
        Ok(Self {
            inner: Mutex::new(inner),
            cfg,
        })
    }
}

fn map_durability(durability: Durability) -> RamWalDurability {
    match durability {
        Durability::None => RamWalDurability::Buffered,
        Durability::Flush => RamWalDurability::Explicit,
        Durability::Fsync => RamWalDurability::SyncEach,
        Durability::GroupCommit => RamWalDurability::GroupCommit,
    }
}

fn map_error(error: RamWalError) -> RsError {
    match error {
        RamWalError::Io(e) => RsError::Io(e),
        RamWalError::Poisoned => RsError::Io(std::io::Error::other("WAL poisoned")),
        RamWalError::DiskFull => RsError::Io(std::io::Error::other("disk full")),
        RamWalError::Closed => RsError::Io(std::io::Error::other("WAL closed")),
        RamWalError::InvalidConfiguration(msg) => RsError::Io(std::io::Error::other(msg)),
        RamWalError::LsnExhausted => RsError::Io(std::io::Error::other("LSN sequence exhausted")),
        RamWalError::Corruption {
            segment: _,
            offset,
            reason: _,
        } => RsError::CorruptWal { offset },
        RamWalError::Tail { segment, state } => RsError::Io(std::io::Error::other(format!(
            "WAL tail state in segment {segment}: {state:?}"
        ))),
        RamWalError::Manifest(msg) => {
            RsError::Io(std::io::Error::other(format!("manifest: {msg}")))
        }
        RamWalError::CheckpointRequired {
            requested,
            checkpoint,
        } => RsError::Io(std::io::Error::other(format!(
            "checkpoint required: requested={requested:?} checkpoint={checkpoint:?}"
        ))),
        RamWalError::CheckpointNotDurable { requested, durable } => {
            RsError::Io(std::io::Error::other(format!(
                "checkpoint not durable: requested={requested:?} durable={durable:?}"
            )))
        }
        RamWalError::CheckpointRegression { current, requested } => {
            RsError::Io(std::io::Error::other(format!(
                "checkpoint regression: current={current:?} requested={requested:?}"
            )))
        }
    }
}

impl Wal {
    pub fn open(
        dir: &str,
        compress: bool,
        durability: Durability,
        seg_bytes: u64,
        retention_max: u64,
    ) -> Result<Self> {
        let mut cfg = Config::new(dir);
        cfg.durability = map_durability(durability);
        cfg.compression = if compress {
            Compression::Lz4
        } else {
            Compression::None
        };
        cfg.seg_max_bytes = seg_bytes;
        cfg.retention_max_bytes = retention_max;
        Ok(Self {
            guard: Guard::new(cfg)?,
        })
    }

    /// Drop the poisoned handle and re-open on the same directory. Records
    /// already on disk survive: RAMWAL's recovery scanner re-reads all
    /// segments on open; the LSN counter resumes from disk state. Callers
    /// must clear the rotation-failure cause first (e.g. remove a stray
    /// directory blocking `create`).
    pub fn reopen(&self) -> Result<()> {
        let new_inner = Arc::new(RamWal::open(self.guard.cfg.clone()).map_err(map_error)?);
        let mut guard = self.guard.inner.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(new_inner);
        Ok(())
    }

    /// True if the underlying handle is poisoned (rotation failure etc.).
    pub fn is_poisoned(&self) -> bool {
        self.guard
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none()
    }

    /// Append a record. Returns its LSN (1-based).
    pub fn append(&self, entry: &WalEntry) -> Result<u64> {
        let payload = encode_entry(entry)?;
        // RAMWAL enforces its own record cap and rejects larger payloads;
        // map that error into RamShield's typed result.
        let guard = self.guard.inner.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(inner) => inner
                .append(&payload)
                .map(|lsn| lsn.get())
                .map_err(|e| match e {
                    RamWalError::InvalidConfiguration(msg) if msg.contains("exceeds max") => {
                        RsError::Io(std::io::Error::other(msg))
                    }
                    other => map_error(other),
                }),
            None => Err(RsError::Io(std::io::Error::other("WAL poisoned"))),
        }
    }

    /// Flush userspace buffers (no fsync).
    pub fn flush(&self) -> Result<()> {
        let guard = self.guard.inner.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(inner) => inner.flush().map_err(map_error),
            None => Err(RsError::Io(std::io::Error::other("WAL poisoned"))),
        }
    }

    /// Flush + fsync the active segment. On Ok, all prior appends are durable.
    pub fn sync(&self) -> Result<()> {
        let guard = self.guard.inner.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(inner) => inner.sync().map_err(map_error),
            None => Err(RsError::Io(std::io::Error::other("WAL poisoned"))),
        }
    }

    /// Highest LSN confirmed durable (0 = none yet).
    pub fn durable_lsn(&self) -> u64 {
        let guard = self.guard.inner.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(inner) => inner.durable_lsn().get(),
            None => 0,
        }
    }

    /// Full replay of every segment in the directory this WAL was opened on.
    pub fn replay(&self) -> Result<Vec<WalEntry>> {
        Self::replay_dir(&self.guard.cfg.dir)
    }

    /// Full replay of every segment in an arbitrary directory (independent
    /// of any open handle).
    pub fn replay_dir(dir: &str) -> Result<Vec<WalEntry>> {
        let report = ramwal::recovery::recover_dir(dir).map_err(map_error)?;
        decode_all(&report)
    }

    /// Replay entries with LSN > min_lsn. The directory is rescanned so
    /// records appended after the WAL handle was opened are visible.
    pub fn replay_from(&self, min_lsn: u64) -> Result<Vec<WalEntry>> {
        if min_lsn == 0 {
            return self.replay();
        }
        let dir = self.guard.cfg.dir.clone();
        let report = ramwal::recovery::recover_dir(&dir).map_err(map_error)?;
        decode_records(report.records.iter().filter(|r| r.lsn.get() > min_lsn))
    }

    /// Begin a checkpoint by reading the current WAL boundary. The caller is
    /// responsible for taking the application snapshot and then publishing
    /// this LSN with `finish_checkpoint`.
    pub fn begin_checkpoint(&self) -> CheckpointBoundary {
        let lsn = {
            let guard = self.guard.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|i| i.current_lsn().get()).unwrap_or(0)
        };
        CheckpointBoundary { lsn }
    }

    /// Complete a checkpoint. The caller must have durably written the
    /// application snapshot through `boundary_lsn` first. RAMWAL owns the
    /// manifest; RamShield owns the snapshot path and contents.
    pub fn finish_checkpoint(&self, boundary_lsn: u64) -> Result<u64> {
        let lsn = Lsn::new(boundary_lsn);
        let guard = self.guard.inner.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(inner) => {
                // RAMWAL checkpoints require the boundary LSN to be durable.
                // RamShield's default Flush mode is not an fsync barrier, so
                // checkpoint completion must establish durability here rather
                // than relying on every caller to remember an extra sync().
                inner.sync().map_err(map_error)?;
                inner.checkpoint(lsn).map_err(map_error)?;
                // Retention happens only after the checkpoint manifest is
                // durable. A retention failure must be observable but must not
                // turn an already-durable checkpoint into a failed checkpoint.
                if let Err(err) = inner.truncate_before(lsn) {
                    tracing::warn!(
                        error = %err,
                        checkpoint_lsn = boundary_lsn,
                        "WAL retention failed after checkpoint commit"
                    );
                }
                Ok(boundary_lsn)
            }
            None => Err(RsError::Io(std::io::Error::other("WAL poisoned"))),
        }
    }

    /// Checkpoint LSN recorded by RAMWAL (0 = none).
    pub fn ckpt_lsn(&self) -> u64 {
        let guard = self.guard.inner.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(inner) => inner.ckpt_lsn().get(),
            None => 0,
        }
    }

    /// Snapshot boundary LSN — replay starts after this. 0 = no checkpoint.
    /// RAMWAL has a single manifest LSN; RamShield treats it as both.
    pub fn snapshot_lsn(&self) -> u64 {
        self.ckpt_lsn()
    }

    /// Oldest LSN still on disk, or None when no segments exist.
    /// Used by the hard-WAL pruned-history check.
    pub fn oldest_lsn(&self) -> Option<u64> {
        let guard = self.guard.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .as_ref()
            .and_then(|i| i.recovery_report().first_lsn.map(|l| l.get()))
    }

    /// Segments pruned since last call. ponytail: RAMWAL owns retention
    /// metadata and does not expose a pruned counter; RamShield only ever
    /// triggers deletion via checkpoint + truncate_before, which is a no-op
    /// between checkpoints — so this reports 0. Add adapter-local accounting
    /// when a live metric needs it.
    pub fn take_segments_pruned(&self) -> u64 {
        0
    }
}

fn encode_entry(entry: &WalEntry) -> Result<Vec<u8>> {
    let raw = serde_json::to_vec(entry).map_err(|e| RsError::Serde(e.to_string()))?;
    Ok(raw)
}

fn decode_all(report: &ramwal::recovery::RecoveryReport) -> Result<Vec<WalEntry>> {
    decode_records(report.records.iter())
}

fn decode_records<'a>(
    iter: impl Iterator<Item = &'a ramwal::recovery::RecoveredRecord>,
) -> Result<Vec<WalEntry>> {
    iter.map(|r| {
        serde_json::from_slice::<WalEntry>(&r.payload).map_err(|e| RsError::Serde(e.to_string()))
    })
    .collect()
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn wal_roundtrip_block_ip() {
        let dir = format!(
            "/tmp/rs_test_rt_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let _ = std::fs::remove_dir_all(&dir);

        let wal = Wal::open(&dir, false, Durability::None, 64 * 1024 * 1024, 0).unwrap();
        wal.append(&WalEntry::BlockIp {
            ip: "10.0.0.1".into(),
            reason: "ddos".into(),
            ttl_secs: Some(3600),
            ts_ns: 1,
        })
        .unwrap();
        drop(wal);

        let entries = Wal::replay_dir(&dir).unwrap();
        assert_eq!(entries.len(), 1, "one record must survive round-trip");
        assert!(matches!(entries[0], WalEntry::BlockIp { .. }));

        // Verify append returns monotonically increasing LSNs.
        let wal2 = Wal::open(&dir, false, Durability::None, 64 * 1024 * 1024, 0).unwrap();
        let lsn1 = wal2
            .append(&WalEntry::UnblockIp {
                ip: "10.0.0.1".into(),
                ts_ns: 2,
            })
            .unwrap();
        let lsn2 = wal2
            .append(&WalEntry::BlockCidr {
                cidr: "192.168.0.0/24".parse().unwrap(),
                reason: "scan".into(),
                ttl_secs: None,
                ts_ns: 3,
            })
            .unwrap();
        assert!(lsn2 > lsn1);
        drop(wal2);

        let entries = Wal::replay_dir(&dir).unwrap();
        assert_eq!(entries.len(), 3, "original + reopened appends");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_recovery_keeps_exact_synced_prefix_after_power_loss() {
        let dir = tempfile::tempdir().unwrap();
        let wal = Wal::open(
            dir.path().to_str().unwrap(),
            false,
            Durability::Flush,
            64 * 1024 * 1024,
            0,
        )
        .unwrap();

        for i in 1..=2u64 {
            wal.append(&WalEntry::BlockIp {
                ip: format!("10.0.0.{i}"),
                reason: "durable-prefix".into(),
                ttl_secs: None,
                ts_ns: i,
            })
            .unwrap();
        }
        wal.sync().unwrap();

        let segment = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("segment-") && name.ends_with(".rwl"))
            })
            .expect("WAL active segment must exist");
        let synced_len = std::fs::metadata(&segment).unwrap().len();

        // This append is deliberately not synced. Truncating the segment to
        // the previously synced byte length models loss of the volatile tail
        // after power loss, independent of the host page cache.
        wal.append(&WalEntry::BlockIp {
            ip: "10.0.0.3".into(),
            reason: "unsynced-tail".into(),
            ttl_secs: None,
            ts_ns: 3,
        })
        .unwrap();
        // Dropping a buffered writer may flush its userspace buffer. That
        // is deliberately not treated as durability: the simulated crash
        // below removes every byte beyond the last explicit sync boundary.
        drop(wal);

        let segment_file = std::fs::OpenOptions::new()
            .write(true)
            .open(&segment)
            .unwrap();
        segment_file.set_len(synced_len).unwrap();
        segment_file.sync_all().unwrap();

        let recovered = Wal::open(
            dir.path().to_str().unwrap(),
            false,
            Durability::Flush,
            64 * 1024 * 1024,
            0,
        )
        .unwrap();
        let entries = recovered.replay_from(0).unwrap();
        let blocks: Vec<_> = entries
            .iter()
            .filter_map(|entry| match entry {
                WalEntry::BlockIp { ip, reason, .. } => Some((ip.as_str(), reason.as_str())),
                _ => None,
            })
            .collect();

        assert_eq!(
            blocks,
            vec![
                ("10.0.0.1", "durable-prefix"),
                ("10.0.0.2", "durable-prefix"),
            ],
            "recovery must retain the synced prefix and exclude the unsynced tail"
        );
        drop(recovered);
    }

    #[test]
    fn wal_replay_from_min_lsn_sees_records_appended_after_open() {
        let dir = format!(
            "/tmp/rs_test_rfl_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let _ = std::fs::remove_dir_all(&dir);

        let wal = Wal::open(&dir, false, Durability::None, 64 * 1024 * 1024, 0).unwrap();
        for i in 1..=2u64 {
            wal.append(&WalEntry::BlockIp {
                ip: format!("10.0.0.{i}"),
                reason: "test".into(),
                ttl_secs: None,
                ts_ns: i,
            })
            .unwrap();
        }
        wal.sync().unwrap();

        let first = wal.replay_from(0).unwrap();
        assert_eq!(first.len(), 2, "current WAL contents must be visible");

        for i in 3..=4u64 {
            wal.append(&WalEntry::BlockIp {
                ip: format!("10.0.0.{i}"),
                reason: "test".into(),
                ttl_secs: None,
                ts_ns: i,
            })
            .unwrap();
        }
        wal.sync().unwrap();

        let tail = wal.replay_from(2).unwrap();
        assert_eq!(
            tail.len(),
            2,
            "records appended after open must be replayed"
        );
        assert!(matches!(&tail[0], WalEntry::BlockIp { ip, .. } if ip == "10.0.0.3"));
        assert!(matches!(&tail[1], WalEntry::BlockIp { ip, .. } if ip == "10.0.0.4"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_checkpoint_flow() {
        let dir = format!(
            "/tmp/rs_test_ckpt_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let _ = std::fs::remove_dir_all(&dir);

        let wal = Wal::open(&dir, false, Durability::Flush, 64 * 1024 * 1024, 0).unwrap();
        // Write pre-checkpoint records.
        for i in 1..=3u64 {
            wal.append(&WalEntry::BlockIp {
                ip: format!("10.0.0.{i}"),
                reason: "pre".into(),
                ttl_secs: None,
                ts_ns: i,
            })
            .unwrap();
        }
        // finish_checkpoint establishes the WAL durability barrier itself.
        let boundary = wal.begin_checkpoint();
        // Simulate snapshot write (caller's job).
        let snap_path = std::path::PathBuf::from(&dir)
            .join(format!("snapshot.{:020}.ckpt", boundary.lsn))
            .to_string_lossy()
            .to_string();
        let snap_content = serde_json::json!({ "lsn": boundary.lsn });
        let tmp = format!("{snap_path}.tmp");
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            use std::io::Write;
            f.write_all(&serde_json::to_vec(&snap_content).unwrap())
                .unwrap();
            f.sync_all().unwrap();
        }
        std::fs::rename(&tmp, &snap_path).unwrap();
        std::fs::File::open(&dir).unwrap().sync_all().unwrap();

        // Complete checkpoint at RAMWAL.
        let ckpt_lsn = wal.finish_checkpoint(boundary.lsn).unwrap();
        assert!(ckpt_lsn > 0);

        // Verify manifest exists.
        let manifest = std::path::PathBuf::from(&dir).join("MANIFEST");
        assert!(manifest.exists(), "MANIFEST should exist after checkpoint");
        let content = std::fs::read_to_string(&manifest).unwrap();
        assert!(content.contains("lsn="));

        // Reopen: RAMWAL recovers up to checkpoint.
        let wal2 = Wal::open(&dir, false, Durability::Fsync, 64 * 1024 * 1024, 0).unwrap();
        assert!(
            wal2.snapshot_lsn() > 0 || wal2.ckpt_lsn() > 0,
            "checkpoint should persist across reopen"
        );

        // Post-checkpoint records.
        for i in 4..=5u64 {
            wal2.append(&WalEntry::BlockIp {
                ip: format!("10.0.0.{i}"),
                reason: "post".into(),
                ttl_secs: None,
                ts_ns: i,
            })
            .unwrap();
        }
        drop(wal2);

        // Full replay should get all 5 + possibly checkpoint marker.
        let all = Wal::replay_dir(&dir).unwrap();
        // At least the 5 BlockIp records must be present.
        let block_ips = all
            .iter()
            .filter(|e| matches!(e, WalEntry::BlockIp { .. }))
            .count();
        assert_eq!(block_ips, 5, "all 5 block records must survive full replay");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_entry_roundtrip_all_variants() {
        let net = IpNetwork::new("10.0.0.0".parse().unwrap(), 8).unwrap();
        let variants: Vec<WalEntry> = vec![
            WalEntry::BlockIp {
                ip: "1.2.3.4".into(),
                reason: "attack".into(),
                ttl_secs: Some(100),
                ts_ns: 1,
            },
            WalEntry::UnblockIp {
                ip: "1.2.3.4".into(),
                ts_ns: 2,
            },
            WalEntry::BlockCidr {
                cidr: net,
                reason: "subnet".into(),
                ttl_secs: None,
                ts_ns: 3,
            },
            WalEntry::UnblockCidr {
                cidr: net,
                ts_ns: 4,
            },
            WalEntry::Insert {
                key: "k".into(),
                value_json: "{}".into(),
                ttl_secs: Some(60),
                ts_ns: 5,
            },
            WalEntry::Delete {
                key: "k".into(),
                ts_ns: 6,
            },
        ];
        let _ = net;
        for entry in &variants {
            let encoded = encode_entry(entry).unwrap();
            let decoded = serde_json::from_slice::<WalEntry>(&encoded).unwrap();
            assert_eq!(entry, &decoded, "round-trip failed for {:?}", entry);
        }
    }
}
