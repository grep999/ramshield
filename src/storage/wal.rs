use crate::error::{Result, RsError};
use crc32fast::Hasher as Crc32;
use lz4_flex::{compress_prepend_size, decompress_size_prepended};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

const MAGIC: u32 = 0x5253_4857;
const HEADER: usize = 4 + 4 + 4 + 1;

#[derive(Debug, Serialize, Deserialize)]
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

pub struct Wal {
    inner: Arc<Mutex<Inner>>,
    compress: bool,
    sync: bool,
    seg_max: u64,
    base_dir: String,
}

struct Inner {
    writer: BufWriter<File>,
    bytes: u64,
    seg: u64,
}

impl Wal {
    pub fn open(dir: &str, compress: bool, sync_mode: &str, seg_bytes: u64) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = seg_path(dir, 0);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let bytes = file.metadata()?.len();
        info!("WAL opened {:?} ({} bytes)", path, bytes);
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                writer: BufWriter::with_capacity(64 * 1024, file),
                bytes,
                seg: 0,
            })),
            compress,
            sync: sync_mode == "sync",
            seg_max: seg_bytes,
            base_dir: dir.to_string(),
        })
    }

    pub fn append(&self, entry: &WalEntry) -> Result<()> {
        let raw = serde_json::to_vec(entry).map_err(|e| RsError::Serde(e.to_string()))?;
        let (payload, flags): (Vec<u8>, u8) = if self.compress && raw.len() > 64 {
            (compress_prepend_size(&raw), 0x01)
        } else {
            (raw, 0x00)
        };
        let mut h = Crc32::new();
        h.update(&payload);
        let crc = h.finalize();

        let mut g = self.inner.lock().unwrap();
        g.writer.write_all(&MAGIC.to_le_bytes())?;
        g.writer.write_all(&(payload.len() as u32).to_le_bytes())?;
        g.writer.write_all(&crc.to_le_bytes())?;
        g.writer.write_all(&[flags])?;
        g.writer.write_all(&payload)?;
        g.bytes += (HEADER + payload.len()) as u64;

        if self.sync {
            g.writer.flush()?;
            g.writer.get_ref().sync_data()?;
        }

        if g.bytes >= self.seg_max {
            g.writer.flush()?;
            g.seg += 1;
            let path = seg_path(&self.base_dir, g.seg);
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)?;
            g.writer = BufWriter::with_capacity(64 * 1024, file);
            g.bytes = 0;
            info!("WAL rotated → {:?}", path);
        }
        Ok(())
    }

    pub fn replay(dir: &str) -> Result<Vec<WalEntry>> {
        let mut segs: Vec<PathBuf> = match std::fs::read_dir(dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "rshw"))
                .collect(),
            Err(_) => return Ok(Vec::new()),
        };
        segs.sort();
        let mut out = Vec::new();
        for seg in &segs {
            let mut file = File::open(seg)?;
            let mut data = Vec::new();
            file.read_to_end(&mut data)?;
            let mut pos = 0usize;
            while pos + HEADER <= data.len() {
                let magic = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
                if magic != MAGIC {
                    return Err(RsError::CorruptWal { offset: pos as u64 });
                }
                let plen = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
                let crc = u32::from_le_bytes(data[pos + 8..pos + 12].try_into().unwrap());
                let flags = data[pos + 12];
                if pos + HEADER + plen > data.len() {
                    warn!("truncated at {}", pos);
                    break;
                }
                let payload = &data[pos + HEADER..pos + HEADER + plen];
                let mut h = Crc32::new();
                h.update(payload);
                if h.finalize() != crc {
                    return Err(RsError::CorruptWal { offset: pos as u64 });
                }
                let decoded: Vec<u8> = if flags & 0x01 != 0 {
                    decompress_size_prepended(payload).map_err(|e| RsError::Serde(e.to_string()))?
                } else {
                    payload.to_vec()
                };
                let entry: WalEntry =
                    serde_json::from_slice(&decoded).map_err(|e| RsError::Serde(e.to_string()))?;
                out.push(entry);
                pos += HEADER + plen;
            }
        }
        info!("WAL replay: {} entries", out.len());
        Ok(out)
    }
}

fn seg_path(dir: &str, idx: u64) -> PathBuf {
    PathBuf::from(dir).join(format!("wal-{:08}.rshw", idx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wal_roundtrip() {
        let dir = std::env::temp_dir()
            .join("rs_wal_rt")
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        let wal = Wal::open(&dir, true, "none", 64 * 1024 * 1024).unwrap();
        wal.append(&WalEntry::BlockIp {
            ip: "1.2.3.4".into(),
            reason: "test".into(),
            ttl_secs: Some(60),
            ts_ns: 1,
        })
        .unwrap();
        drop(wal);
        let entries = Wal::replay(&dir).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], WalEntry::BlockIp { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_replay_multiple_entries() {
        let dir = std::env::temp_dir()
            .join("rs_wal_multi")
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        let wal = Wal::open(&dir, true, "none", 64 * 1024 * 1024).unwrap();
        wal.append(&WalEntry::BlockIp {
            ip: "10.0.0.1".into(),
            reason: "ddos".into(),
            ttl_secs: Some(3600),
            ts_ns: 1,
        })
        .unwrap();
        wal.append(&WalEntry::BlockIp {
            ip: "10.0.0.2".into(),
            reason: "scan".into(),
            ttl_secs: None,
            ts_ns: 2,
        })
        .unwrap();
        wal.append(&WalEntry::UnblockIp {
            ip: "10.0.0.1".into(),
            ts_ns: 3,
        })
        .unwrap();
        wal.append(&WalEntry::Insert {
            key: "test_key".into(),
            value_json: "{\"a\":1}".into(),
            ttl_secs: None,
            ts_ns: 4,
        })
        .unwrap();
        drop(wal);
        let entries = Wal::replay(&dir).unwrap();
        assert_eq!(entries.len(), 4);
        assert!(matches!(entries[0], WalEntry::BlockIp { .. }));
        assert!(matches!(entries[1], WalEntry::BlockIp { .. }));
        assert!(matches!(entries[2], WalEntry::UnblockIp { .. }));
        assert!(matches!(entries[3], WalEntry::Insert { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_no_corrupt_replay_fails() {
        let dir = std::env::temp_dir()
            .join("rs_wal_corrupt")
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Write garbage to simulate corruption
        std::fs::write(
            format!("{}/wal-00000000.rshw", dir),
            b"NOT A VALID WAL ENTRY",
        )
        .unwrap();
        let result = Wal::replay(&dir);
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_empty_dir_returns_empty() {
        let dir = std::env::temp_dir()
            .join("rs_wal_empty")
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let entries = Wal::replay(&dir).unwrap();
        assert!(entries.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_uncompressed_roundtrip() {
        let dir = std::env::temp_dir()
            .join("rs_wal_uncomp")
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        let wal = Wal::open(&dir, false, "none", 64 * 1024 * 1024).unwrap();
        wal.append(&WalEntry::Delete {
            key: "delete_me".into(),
            ts_ns: 1,
        })
        .unwrap();
        drop(wal);
        let entries = Wal::replay(&dir).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], WalEntry::Delete { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_segment_rotation() {
        let dir = std::env::temp_dir()
            .join("rs_wal_seg")
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        // Small segment size to force rotation
        let wal = Wal::open(&dir, false, "none", 128).unwrap();
        for i in 0..100 {
            wal.append(&WalEntry::BlockIp {
                ip: format!("10.0.0.{}", i),
                reason: "test".into(),
                ttl_secs: None,
                ts_ns: i as u64,
            })
            .unwrap();
        }
        drop(wal);
        let entries = Wal::replay(&dir).unwrap();
        assert_eq!(entries.len(), 100);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
