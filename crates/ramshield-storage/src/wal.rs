use crc32fast::Hasher as Crc32;
use lz4_flex::{compress_prepend_size, decompress_size_prepended};
use ramshield_types::{Durability, Result, RsError};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

/// WAL format version — bump when on-disk layout changes.
const FORMAT_VERSION: u16 = 1;
const MAGIC: u32 = 0x5253_4857;
/// Header: magic(4) + version(2) + lsn(8) + payload_len(4) + crc(4) + flags(1) = 23
const HEADER: usize = 4 + 2 + 8 + 4 + 4 + 1;
/// Maximum single-record payload size (64 KiB). Prevents OOM on corrupt length.
const MAX_RECORD_SIZE: usize = 64 * 1024;
/// Quarantine subdirectory for corrupt tail segments.
const QUARANTINE_DIR: &str = "quarantine";

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

/// On-disk record header (written before payload).
#[derive(Debug)]
struct RecordHeader {
    magic: u32,
    version: u16,
    lsn: u64,
    payload_len: u32,
    crc: u32,
    flags: u8,
}

impl RecordHeader {
    fn to_bytes(&self) -> [u8; HEADER] {
        let mut buf = [0u8; HEADER];
        buf[0..4].copy_from_slice(&self.magic.to_le_bytes());
        buf[4..6].copy_from_slice(&self.version.to_le_bytes());
        buf[6..14].copy_from_slice(&self.lsn.to_le_bytes());
        buf[14..18].copy_from_slice(&self.payload_len.to_le_bytes());
        buf[18..22].copy_from_slice(&self.crc.to_le_bytes());
        buf[22] = self.flags;
        buf
    }

    fn from_bytes(buf: &[u8; HEADER]) -> Self {
        Self {
            magic: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            version: u16::from_le_bytes([buf[4], buf[5]]),
            lsn: u64::from_le_bytes([
                buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12], buf[13],
            ]),
            payload_len: u32::from_le_bytes([buf[14], buf[15], buf[16], buf[17]]),
            crc: u32::from_le_bytes([buf[18], buf[19], buf[20], buf[21]]),
            flags: buf[22],
        }
    }
}

pub struct Wal {
    inner: Arc<Mutex<Inner>>,
    compress: bool,
    durability: Durability,
    seg_max: u64,
    /// Total-bytes cap across segments; oldest deleted first. 0 = unlimited.
    retention_max: u64,
    base_dir: String,
    lsn_counter: AtomicU64,
    next_sync_due_ns: AtomicU64,
}

struct Inner {
    // writer holds a BufWriter over the same Arc<File> exposed below.
    // The file handle is kept as Arc<File> so the writer + the sync_data
    // caller can both reference it after the mutex is dropped.
    writer: BufWriter<Arc<File>>,
    file: Arc<File>,
    bytes: u64,
    seg: u64,
}

impl Wal {
    pub fn open(
        dir: &str,
        compress: bool,
        durability: Durability,
        seg_bytes: u64,
        retention_max: u64,
    ) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        fsync_dir(dir)?;
        enforce_retention(dir, retention_max);

        // Discover highest segment to resume from
        let max_seg = discover_max_seg(dir);
        let path = seg_path(dir, max_seg);
        let file = Arc::new(OpenOptions::new().create(true).append(true).open(&path)?);
        let bytes = file.metadata()?.len();

        // Discover highest LSN across all segments
        // LSNs start at 1: 0 is reserved as "no entry" (EnforceResult.wal_lsn: None ↔ 0).
        let start_lsn = match discover_max_lsn(dir)? {
            Some(max) => max + 1,
            None => 1,
        };

        info!(
            "WAL opened {:?} ({} bytes, start_lsn={}, durability={:?})",
            path, bytes, start_lsn, durability
        );

        let writer = BufWriter::with_capacity(64 * 1024, Arc::clone(&file));
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                writer,
                file,
                bytes,
                seg: max_seg,
            })),
            compress,
            durability,
            seg_max: seg_bytes,
            retention_max,
            base_dir: dir.to_string(),
            lsn_counter: AtomicU64::new(start_lsn),
            next_sync_due_ns: AtomicU64::new(0),
        })
    }

    /// Append an entry and return its LSN.
    pub fn append(&self, entry: &WalEntry) -> Result<u64> {
        // Step 1: Serialization, CRC, header — all CPU work OUTSIDE the mutex.
        let raw = serde_json::to_vec(entry).map_err(|e| RsError::Serde(e.to_string()))?;
        if raw.len() > MAX_RECORD_SIZE {
            return Err(RsError::RecordTooLarge {
                size: raw.len(),
                max: MAX_RECORD_SIZE,
            });
        }
        let (payload, flags): (Vec<u8>, u8) = if self.compress && raw.len() > 64 {
            (compress_prepend_size(&raw), 0x01)
        } else {
            (raw, 0x00)
        };
        if payload.len() > MAX_RECORD_SIZE {
            return Err(RsError::RecordTooLarge {
                size: payload.len(),
                max: MAX_RECORD_SIZE,
            });
        }
        let mut h = Crc32::new();
        h.update(&payload);
        let crc = h.finalize();
        let lsn = self.lsn_counter.fetch_add(1, Ordering::SeqCst);

        let rh = RecordHeader {
            magic: MAGIC,
            version: FORMAT_VERSION,
            lsn,
            payload_len: payload.len() as u32,
            crc,
            flags,
        };
        let rh_bytes = rh.to_bytes();

        // Step 2: I/O under mutex — minimal critical section.
        // Writes + flush are fast (buffered). The expensive sync_data() runs
        // OUTSIDE the lock: we snapshot the current file handle as Arc<File>
        // and drop the guard, then call sync_data on the Arc clone.
        // ponytail: a dedicated writer thread via crossbeam channel would
        // remove the Arc clone hop entirely.
        let (old_file_arc, needs_dir_sync) = {
            let mut g = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            g.writer.write_all(&rh_bytes)?;
            g.writer.write_all(&payload)?;
            g.bytes += (HEADER + payload.len()) as u64;

            let want_sync = matches!(self.durability, Durability::Fsync | Durability::GroupCommit);
            // P0 fix (starvation): deadline-based group commit. The previous
            // form (swap prev->now, require gap >= 100ms) reset the clock on
            // EVERY append — steady writers with <100ms inter-arrival (e.g.
            // a block storm at 500 writes/s) never synced, unbounded data
            // loss on crash. Now: an absolute due-deadline; the first append
            // observing `now >= due` performs the sync and arms the next
            // deadline. Worst-case exposure: one 100ms window. A racing
            // double-claimant only costs one extra fsync per window (~10Hz)
            // — benign, and cheaper than a CAS loop here.
            let now_wall = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            let mut must_sync = false;
            if want_sync && now_wall >= self.next_sync_due_ns.load(Ordering::Relaxed) {
                self.next_sync_due_ns
                    .store(now_wall + 100_000_000, Ordering::Relaxed);
                must_sync = true;
            }

            let must_rotate = g.bytes >= self.seg_max;
            if must_sync || matches!(self.durability, Durability::Flush) || must_rotate {
                // P0 fix (rotation durability): the old code never flushed
                // before replacing g.writer. BufWriter::drop does write out
                // its buffer, but (a) it IGNORES errors — a full disk lost
                // up to 64KiB of WAL silently — and (b) with no fsync of the
                // old segment after rotation, that segment's tail could only
                // ever reach disk by luck (no future append touches it).
                // Explicit flush + sync_data on the old fd (via old_file_arc
                // below) closes both holes.
                g.writer.flush()?;
            }
            // Snapshot the file holding the just-flushed bytes BEFORE any
            // rotation reassigns g.file, so step 3 fsyncs the right segment.
            // On rotation the old segment is never written again — syncing
            // it now makes the whole segment durable in one fsync.
            let old_file_arc = if must_sync || must_rotate {
                Some(Arc::clone(&g.file))
            } else {
                None
            };

            if must_rotate {
                let new_seg = g.seg + 1;
                let path = seg_path(&self.base_dir, new_seg);
                let new_file = Arc::new(
                    OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(&path)?,
                );
                g.writer = BufWriter::with_capacity(64 * 1024, Arc::clone(&new_file));
                g.file = new_file;
                g.bytes = 0;
                g.seg = new_seg;
                info!("WAL rotated → {:?}", path);
            }
            (old_file_arc, must_rotate)
        }; // mutex dropped here

        // Step 3: sync_data OUTSIDE the mutex. Other appenders proceed
        // immediately while this fsync runs (100µs SSD, up to ~10ms HDD).
        if let Some(f) = old_file_arc {
            f.sync_data()?;
        }

        // Step 4: Directory sync OUTSIDE the mutex — ~10ms on rotational, ~0.1ms on SSD.
        if needs_dir_sync {
            fsync_dir(&self.base_dir)?;
        }

        // P2 fix (F5): retention was scanned on EVERY append — read_dir +
        // metadata() per segment + sort, hundreds of syscalls during a
        // subnet-burst block storm. Total size only crosses the cap at
        // rotation (appends add ≤seg_max to one file), so scanning only on
        // rotation is both cheaper and sufficient.
        if needs_dir_sync && self.retention_max > 0 {
            enforce_retention(&self.base_dir, self.retention_max);
        }

        Ok(lsn)
    }

    /// Write an atomic checkpoint: flush WAL, write manifest, fsync both + dir.
    pub fn checkpoint(&self, snapshot_path: &str) -> Result<u64> {
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);

        let lsn = self.append(&WalEntry::Checkpoint {
            snapshot_path: snapshot_path.to_string(),
            ts_ns: now_ns,
        })?;

        // Write manifest atomically: tmp + rename
        let manifest_path = PathBuf::from(&self.base_dir).join("MANIFEST");
        let tmp_path = manifest_path.with_extension("tmp");
        {
            let mut f = File::create(&tmp_path)?;
            write!(f, "lsn={}\nsnapshot={}\n", lsn, snapshot_path)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp_path, &manifest_path)?;
        fsync_dir(&self.base_dir)?;

        info!("WAL checkpoint lsn={} snapshot={}", lsn, snapshot_path);
        Ok(lsn)
    }

    /// Streaming replay with bounded reads. Returns entries in LSN order.
    /// Corrupt tail records are quarantined instead of failing the entire replay.
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

        let mut out: Vec<(u64, WalEntry)> = Vec::new();
        let quarantine_dir = PathBuf::from(dir).join(QUARANTINE_DIR);

        for seg in &segs {
            let mut file = File::open(seg)?;
            let mut payload_buf = vec![0u8; MAX_RECORD_SIZE];
            let mut corrupted = false;
            let mut last_valid_offset: u64 = 0;

            loop {
                let mut peek = [0u8; 1];
                match file.read(&mut peek) {
                    Ok(0) => break, // Clean EOF
                    Ok(_) => {
                        let mut hdr_buf = [0u8; HEADER];
                        hdr_buf[0] = peek[0];
                        if let Err(e) = file.read_exact(&mut hdr_buf[1..]) {
                            warn!("WAL partial header in {:?}: {}", seg, e);
                            corrupted = true;
                            break;
                        }

                        let rh = RecordHeader::from_bytes(&hdr_buf);
                        if rh.magic != MAGIC {
                            warn!("WAL bad magic at in {:?}", seg);
                            corrupted = true;
                            break;
                        }
                        if rh.version > FORMAT_VERSION {
                            warn!("WAL future version {} in {:?}", rh.version, seg);
                            corrupted = true;
                            break;
                        }
                        if rh.payload_len as usize > MAX_RECORD_SIZE {
                            warn!(
                                "WAL record too large ({} bytes) in {:?}",
                                rh.payload_len, seg
                            );
                            corrupted = true;
                            break;
                        }

                        let plen = rh.payload_len as usize;
                        if plen > payload_buf.len() {
                            payload_buf.resize(plen, 0);
                        }
                        if let Err(e) = file.read_exact(&mut payload_buf[..plen]) {
                            warn!("WAL truncated payload in {:?}: {}", seg, e);
                            corrupted = true;
                            break;
                        }

                        let payload = &payload_buf[..plen];
                        let mut h = Crc32::new();
                        h.update(payload);
                        if h.finalize() != rh.crc {
                            warn!("WAL crc mismatch in {:?}", seg);
                            corrupted = true;
                            break;
                        }

                        let decoded: Vec<u8> = if rh.flags & 0x01 != 0 {
                            // Decompression-bomb guard: decompress_size_prepended
                            // trusts the u32 LE size prefix and allocates it
                            // eagerly (probe: 8-byte payload -> 4GB VmPeak ->
                            // OOM-abort). Every legitimately written record was
                            // <= MAX_RECORD_SIZE raw (append enforces it pre-
                            // and post-compression), so a larger declared
                            // decompressed size is corruption or an attack.
                            let declared = payload
                                .get(..4)
                                // ponytail: invariant — get(..4) guarantees exactly 4 bytes,
                                // so TryInto<[u8; 4]> is provably infallible. Unwrap is safe.
                                .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
                                .unwrap_or(usize::MAX);
                            if declared > MAX_RECORD_SIZE {
                                warn!(
                                    "WAL record claims {declared} decompressed bytes \
                                     (max {MAX_RECORD_SIZE}) in {:?} — treating as corrupt",
                                    seg
                                );
                                corrupted = true;
                                break;
                            }
                            match decompress_size_prepended(payload) {
                                Ok(d) => d,
                                Err(e) => {
                                    warn!("WAL decompress error in {:?}: {}", seg, e);
                                    corrupted = true;
                                    break;
                                }
                            }
                        } else {
                            payload.to_vec()
                        };

                        match serde_json::from_slice::<WalEntry>(&decoded) {
                            Ok(entry) => {
                                last_valid_offset = file.stream_position()?;
                                out.push((rh.lsn, entry));
                            }
                            Err(e) => {
                                warn!("WAL deser error in {:?}: {}", seg, e);
                                corrupted = true;
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("WAL read error in {:?}: {}", seg, e);
                        corrupted = true;
                        break;
                    }
                }
            }

            if corrupted {
                drop(file); // close read-only handle before reopening for write
                if last_valid_offset > 0 {
                    // P2 fix: truncate at last valid byte offset instead of
                    // quarantining the entire segment. This preserves all
                    // valid records before the corrupt tail. Idempotent:
                    // if corruption is re-detected on next replay, the same
                    // truncation point is applied (no data loss, no double count).
                    let f = OpenOptions::new().write(true).open(seg)?;
                    f.set_len(last_valid_offset)?;
                    info!(
                        "WAL truncated {:?} at {} bytes (corrupt tail removed)",
                        seg, last_valid_offset
                    );
                } else {
                    // No valid records were recovered — quarantine the entire
                    // segment (corruption from the start). This is the original
                    // behavior for an all-corrupt segment.
                    let _ = std::fs::create_dir_all(&quarantine_dir);
                    let dest = quarantine_dir.join(seg.file_name().unwrap_or_default());
                    if let Err(e) = std::fs::rename(seg, &dest) {
                        warn!("WAL quarantine rename failed: {}", e);
                    } else {
                        info!("WAL quarantined {:?} → {:?}", seg, dest);
                    }
                }
            }
        }

        // Idempotent replay: sort by LSN, dedup by LSN
        out.sort_by_key(|(lsn, _)| *lsn);
        out.dedup_by_key(|(lsn, _)| *lsn);

        info!("WAL replay: {} entries", out.len());
        Ok(out.into_iter().map(|(_, e)| e).collect())
    }

    /// Directory this WAL was opened under (for replay).
    pub fn base_dir(&self) -> &str {
        &self.base_dir
    }
}

/// fsync the directory to ensure directory entries (creates, renames) are durable.
fn fsync_dir(dir: &str) -> Result<()> {
    let d = File::open(dir)?;
    d.sync_all()?;
    Ok(())
}

/// Find the highest segment index in the directory.
fn discover_max_seg(dir: &str) -> u64 {
    match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name();
                let s = name.to_string_lossy();
                s.strip_prefix("wal-")
                    .and_then(|s| s.strip_suffix(".rshw"))
                    .and_then(|s| s.parse::<u64>().ok())
            })
            .max()
            .unwrap_or(0),
        Err(_) => 0,
    }
}

/// Scan all segments to find the highest LSN (for crash recovery).
/// Returns None if no segments exist.
fn discover_max_lsn(dir: &str) -> Result<Option<u64>> {
    let segs: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "rshw"))
            .collect(),
        Err(_) => return Ok(None),
    };

    if segs.is_empty() {
        return Ok(None);
    }

    let mut max_lsn: u64 = 0;
    let mut found_any = false;
    for seg in &segs {
        let file = File::open(seg)?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);
        let mut hdr_buf = [0u8; HEADER];
        let mut skip_buf = vec![0u8; MAX_RECORD_SIZE];

        loop {
            if reader.read_exact(&mut hdr_buf).is_err() {
                break;
            }
            let rh = RecordHeader::from_bytes(&hdr_buf);
            if rh.magic != MAGIC || rh.payload_len as usize > MAX_RECORD_SIZE {
                break;
            }
            if rh.lsn > max_lsn {
                max_lsn = rh.lsn;
                found_any = true;
            }
            let plen = rh.payload_len as usize;
            if plen > skip_buf.len() {
                skip_buf.resize(plen, 0);
            }
            if reader.read_exact(&mut skip_buf[..plen]).is_err() {
                break;
            }
        }
    }
    Ok(if found_any { Some(max_lsn) } else { None })
}

fn seg_path(dir: &str, idx: u64) -> PathBuf {
    PathBuf::from(dir).join(format!("wal-{:08}.rshw", idx))
}

/// Delete oldest segments until total .rshw bytes fit the cap. Never touches
/// the newest segment. Best-effort: delete failures are logged, not fatal.
fn enforce_retention(dir: &str, max_bytes: u64) {
    if max_bytes == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut segs: Vec<(u64, u64)> = rd // (seg_idx, size)
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            let idx = s
                .strip_prefix("wal-")?
                .strip_suffix(".rshw")?
                .parse::<u64>()
                .ok()?;
            Some((idx, e.metadata().ok()?.len()))
        })
        .collect();
    segs.sort_unstable_by_key(|&(idx, _)| idx);

    let total: u64 = segs.iter().map(|&(_, sz)| sz).sum();
    if total <= max_bytes {
        return;
    }
    let mut over = total - max_bytes;
    for &(idx, sz) in &segs[..segs.len().saturating_sub(1)] {
        if over == 0 {
            break;
        }
        let path = seg_path(dir, idx);
        match std::fs::remove_file(&path) {
            Ok(()) => {
                warn!("WAL retention: deleted {:?} ({} bytes)", path, sz);
                over = over.saturating_sub(sz);
            }
            Err(e) => warn!("WAL retention delete {:?} failed: {}", path, e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> String {
        let dir = std::env::temp_dir()
            .join(name)
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// P0 regression: rotation must not lose buffered records. With
    /// GroupCommit armed for the future (no sync due) and a tiny segment
    /// limit forcing rotation, records buffered in the old 64KiB BufWriter
    /// used to vanish when the writer was replaced. All appends must replay.
    #[test]
    fn wal_rotation_flushes_buffer() {
        let dir = tmp("rs_wal_rotation");
        {
            let wal = Wal::open(&dir, false, Durability::GroupCommit, 256, 0).unwrap();
            // First append syncs (deadline 0 => due immediately) and arms
            // the 100ms window; following appends land in the no-sync region.
            // seg_max=256B forces several rotations inside that window.
            for i in 1..=20u64 {
                wal.append(&WalEntry::BlockIp {
                    ip: format!("10.0.0.{i}"),
                    reason: "r".into(),
                    ttl_secs: None,
                    ts_ns: i,
                })
                .unwrap();
            }
            // Do NOT drop cleanly via checkpoint — simulate process holding
            // only userspace buffers when rotation swaps writers.
        }
        let entries = Wal::replay(&dir).unwrap();
        assert_eq!(
            entries.len(),
            20,
            "rotation lost buffered WAL records: {} of 20 survived",
            entries.len()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_roundtrip() {
        let dir = tmp("rs_wal_rt2");
        let wal = Wal::open(&dir, true, Durability::None, 64 * 1024 * 1024, 0).unwrap();
        let lsn = wal
            .append(&WalEntry::BlockIp {
                ip: "1.2.3.4".into(),
                reason: "test".into(),
                ttl_secs: Some(60),
                ts_ns: 1,
            })
            .unwrap();
        assert_eq!(lsn, 1);
        drop(wal);
        let entries = Wal::replay(&dir).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], WalEntry::BlockIp { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_lsn_monotonic() {
        let dir = tmp("rs_wal_lsn");
        let wal = Wal::open(&dir, false, Durability::None, 64 * 1024 * 1024, 0).unwrap();
        let a = wal
            .append(&WalEntry::BlockIp {
                ip: "1.1.1.1".into(),
                reason: "a".into(),
                ttl_secs: None,
                ts_ns: 1,
            })
            .unwrap();
        let b = wal
            .append(&WalEntry::BlockIp {
                ip: "2.2.2.2".into(),
                reason: "b".into(),
                ttl_secs: None,
                ts_ns: 2,
            })
            .unwrap();
        let c = wal
            .append(&WalEntry::UnblockIp {
                ip: "1.1.1.1".into(),
                ts_ns: 3,
            })
            .unwrap();
        assert!(a < b);
        assert!(b < c);
        drop(wal);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_replay_after_restart() {
        let dir = tmp("rs_wal_restart");
        let wal = Wal::open(&dir, true, Durability::None, 64 * 1024 * 1024, 0).unwrap();
        wal.append(&WalEntry::BlockIp {
            ip: "10.0.0.1".into(),
            reason: "ddos".into(),
            ttl_secs: Some(3600),
            ts_ns: 1,
        })
        .unwrap();
        wal.append(&WalEntry::UnblockIp {
            ip: "10.0.0.1".into(),
            ts_ns: 2,
        })
        .unwrap();
        drop(wal);

        // Reopen — should discover LSN and continue
        let wal2 = Wal::open(&dir, true, Durability::None, 64 * 1024 * 1024, 0).unwrap();
        let lsn = wal2
            .append(&WalEntry::Insert {
                key: "k".into(),
                value_json: "{}".into(),
                ttl_secs: None,
                ts_ns: 3,
            })
            .unwrap();
        assert_eq!(lsn, 3); // 1,2 from first open, 3 is next
        drop(wal2);

        let entries = Wal::replay(&dir).unwrap();
        assert_eq!(entries.len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_corrupt_tail_quarantine() {
        let dir = tmp("rs_wal_quar");
        let wal = Wal::open(&dir, false, Durability::None, 64 * 1024 * 1024, 0).unwrap();
        wal.append(&WalEntry::BlockIp {
            ip: "10.0.0.1".into(),
            reason: "ok".into(),
            ttl_secs: None,
            ts_ns: 1,
        })
        .unwrap();
        drop(wal);

        // Append garbage to the segment file (corrupt tail)
        let seg = std::fs::read_dir(&dir)
            .unwrap()
            .find_map(|e| {
                let p = e.ok()?.path();
                if p.extension().is_some_and(|x| x == "rshw") {
                    Some(p)
                } else {
                    None
                }
            })
            .unwrap();
        {
            use std::fs::OpenOptions;
            let mut f = OpenOptions::new().append(true).open(&seg).unwrap();
            f.write_all(b"GARBAGE_DATA_HERE").unwrap();
        }

        // P2 fix: when the segment has 1 valid record before a corrupt tail,
        // the new code truncates the segment (preserving the valid record)
        // instead of quarantining the whole file. The valid entry must survive.
        let entries = Wal::replay(&dir).unwrap();
        assert_eq!(
            entries.len(),
            1,
            "valid record before corrupt tail must survive truncation"
        );
        // No quarantine needed — the segment was truncated in place.
        let quarantine = PathBuf::from(&dir).join(QUARANTINE_DIR);
        assert!(
            !quarantine.exists(),
            "no quarantine dir needed when truncation is sufficient"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_empty_dir_returns_empty() {
        let dir = tmp("rs_wal_empty2");
        std::fs::create_dir_all(&dir).unwrap();
        let entries = Wal::replay(&dir).unwrap();
        assert!(entries.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_uncompressed_roundtrip() {
        let dir = tmp("rs_wal_uncomp2");
        let wal = Wal::open(&dir, false, Durability::None, 64 * 1024 * 1024, 0).unwrap();
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
        let dir = tmp("rs_wal_seg2");
        let wal = Wal::open(&dir, false, Durability::None, 128, 0).unwrap();
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

    /// Retention deletes oldest segments first; the newest (live) segment and
    /// its records always survive.
    #[test]
    fn wal_retention_deletes_oldest_segments() {
        let dir = tmp("rs_wal_ret");
        // Tiny segments (~1 record each), 600-byte total cap.
        let wal = Wal::open(&dir, false, Durability::None, 128, 600).unwrap();
        for i in 0..40 {
            wal.append(&WalEntry::BlockIp {
                ip: format!("10.9.{}.{}.{}", i >> 8 & 255, i >> 4 & 15, i & 15),
                reason: "retention_test".into(),
                ttl_secs: None,
                ts_ns: i as u64,
            })
            .unwrap();
        }
        drop(wal);

        let segs: Vec<(std::path::PathBuf, u64)> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "rshw"))
            .map(|e| {
                let p = e.path();
                let sz = p.metadata().unwrap().len();
                (p, sz)
            })
            .collect();
        let total: u64 = segs.iter().map(|&(_, sz)| sz).sum();
        assert!(
            total <= 600 + 300, // cap + newest-segment slack
            "retention should prune: {} bytes across {} segments",
            total,
            segs.len()
        );
        // Newest segment's records must still replay.
        let entries = Wal::replay(&dir).unwrap();
        assert!(!entries.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_record_too_large() {
        let dir = tmp("rs_wal_big");
        let wal = Wal::open(&dir, false, Durability::None, 64 * 1024 * 1024, 0).unwrap();
        let big_val = "x".repeat(MAX_RECORD_SIZE + 1);
        let result = wal.append(&WalEntry::Insert {
            key: "k".into(),
            value_json: big_val,
            ttl_secs: None,
            ts_ns: 1,
        });
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_durability_fsync() {
        let dir = tmp("rs_wal_fsync");
        let wal = Wal::open(&dir, false, Durability::Fsync, 64 * 1024 * 1024, 0).unwrap();
        wal.append(&WalEntry::BlockIp {
            ip: "10.0.0.1".into(),
            reason: "test".into(),
            ttl_secs: None,
            ts_ns: 1,
        })
        .unwrap();
        drop(wal);
        let entries = Wal::replay(&dir).unwrap();
        assert_eq!(entries.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_checkpoint_atomic() {
        let dir = tmp("rs_wal_ckpt");
        let wal = Wal::open(&dir, false, Durability::Fsync, 64 * 1024 * 1024, 0).unwrap();
        let lsn = wal.checkpoint("/tmp/snap.bin").unwrap();
        assert!(lsn > 0);
        // Manifest should exist
        let manifest = PathBuf::from(&dir).join("MANIFEST");
        assert!(manifest.exists());
        let content = std::fs::read_to_string(&manifest).unwrap();
        assert!(content.contains(&format!("lsn={}", lsn)));
        assert!(content.contains("/tmp/snap.bin"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_replay_idempotent() {
        let dir = tmp("rs_wal_idem");
        let wal = Wal::open(&dir, false, Durability::None, 64 * 1024 * 1024, 0).unwrap();
        wal.append(&WalEntry::BlockIp {
            ip: "10.0.0.1".into(),
            reason: "a".into(),
            ttl_secs: None,
            ts_ns: 1,
        })
        .unwrap();
        drop(wal);

        let e1 = Wal::replay(&dir).unwrap();
        let e2 = Wal::replay(&dir).unwrap();
        assert_eq!(e1.len(), e2.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// P2 regression: corrupt records in a segment must NOT cause loss of
    /// valid records that came before them. Old code quarantined the whole
    /// segment on any corruption, throwing away every valid record. New code
    /// truncates the segment at the last valid byte offset, preserving the
    /// pre-corruption valid records.
    #[test]
    fn wal_replay_preserves_valid_records_before_corruption() {
        let dir = tmp("rs_wal_partial_corrupt");
        let wal = Wal::open(&dir, true, Durability::None, 64 * 1024 * 1024, 0).unwrap();
        // Append 3 valid records
        for i in 1..=3u64 {
            wal.append(&WalEntry::BlockIp {
                ip: format!("10.0.0.{i}"),
                reason: "test".into(),
                ttl_secs: None,
                ts_ns: i,
            })
            .unwrap();
        }
        drop(wal);

        // Find the segment file and append a corrupt record (wrong magic).
        let seg_path = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "rshw"))
            .expect("at least one .rshw segment must exist");
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&seg_path)
            .unwrap();
        // 23 bytes of garbage: 0xFF as a bad magic.
        f.write_all(&[0xFF; HEADER]).unwrap();
        f.sync_all().unwrap();
        drop(f);

        // Replay must recover all 3 valid records, not 0.
        let entries = Wal::replay(&dir).unwrap();
        assert_eq!(
            entries.len(),
            3,
            "replay must preserve valid records before the corrupt tail"
        );

        // Re-replay (after truncation) must also recover all 3 — idempotency.
        let entries2 = Wal::replay(&dir).unwrap();
        assert_eq!(
            entries2.len(),
            3,
            "replay must be idempotent after truncation"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// P1 regression: a compressed record whose LZ4 size prefix claims ~4GB
    /// must be treated as corrupt, NOT decompressed. lz4_flex's
    /// decompress_size_prepended allocates the declared size eagerly (probe:
    /// 8-byte payload -> 4GB VmPeak). Header magic/CRC can be valid — only
    /// the claimed decompressed length is hostile.
    #[test]
    fn wal_decompression_bomb_rejected() {
        let dir = tmp("rs_wal_bomb");
        let wal = Wal::open(&dir, true, Durability::None, 64 * 1024 * 1024, 0).unwrap();
        wal.append(&WalEntry::BlockIp {
            ip: "10.0.0.1".into(),
            reason: "test".into(),
            ttl_secs: None,
            ts_ns: 1,
        })
        .unwrap();
        drop(wal);

        let seg_path = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "rshw"))
            .expect("segment");
        // payload: u32 LE claimed-size = 0xFFFFFFFE, plus a few filler bytes.
        let mut payload = vec![0u8; 8];
        payload[0..4].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        let mut h = crc32fast::Hasher::new();
        h.update(&payload);
        let hdr = RecordHeader {
            magic: MAGIC,
            version: 1,
            lsn: 2,
            payload_len: payload.len() as u32,
            crc: h.finalize(),
            flags: 0x01, // compressed
        }
        .to_bytes();
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&seg_path)
            .unwrap();
        f.write_all(&hdr).unwrap();
        f.write_all(&payload).unwrap();
        f.sync_all().unwrap();
        drop(f);

        // Replay must return the 1 good record and flag the bomb as corrupt
        // (pre-fix this panicked/OOM'd inside decompress_size_prepended).
        let entries = Wal::replay(&dir).unwrap();
        assert_eq!(
            entries.len(),
            1,
            "bomb record must be truncated, not expanded"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
