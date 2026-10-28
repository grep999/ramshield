use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::config::{Compression, Config, Durability};
use crate::error::Error;
use crate::lsn::Lsn;
use crate::record::{HEADER_SIZE, MAX_RECORD_SIZE, RecordHeader, encode_payload};
use crate::recovery::{Recovery, RecoveryReport, recover};
use crate::segment::{self, MANIFEST_NAME, fsync_dir, open_segment, seg_path};

const GROUP_COMMIT_WINDOW_NS: u64 = 100_000_000;

/// WAL instance. Recovery runs during `open()` before any append is accepted.
/// The writer starts exactly where recovery established the boundary.
///
/// # Lifecycle
///
/// A write-side I/O failure poisons the in-memory instance. After poisoning
/// `append`, `flush`, `sync`, `checkpoint` and retention all fail with
/// [`Error::Poisoned`]. The caller must drop the instance and reopen:
/// reopening performs recovery and establishes a new verified append boundary.
///
/// ```text
/// OPEN --write failure--> POISONED --reopen--> RECOVERY --> OPEN
/// ```
pub struct Wal {
    inner: Arc<Mutex<Inner>>,
    compression: Compression,
    durability: Durability,
    seg_max: u64,
    retention_max: u64,
    base_dir: String,
    /// Next LSN `append` would allocate (`next_lsn`). At exhaustion this holds
    /// the `u64::MAX` sentinel, which is why it must never be used to derive
    /// [`Wal::current_lsn`] — that reads `written_lsn` instead.
    lsn_counter: AtomicU64,
    ckpt_lsn: AtomicU64,
    recovery: Recovery,
    buffer_capacity: usize,
    checkpoint_lock: Mutex<()>,
}

/// Lifecycle of a live WAL instance.
///
/// A write-side I/O failure poisons the instance: after poisoning `append`,
/// `flush`, `sync`, `checkpoint` and retention all fail. The caller must drop
/// the instance and reopen, which runs recovery and establishes a new
/// verified append boundary.
enum WalState {
    Open,
    Poisoned,
}

struct Inner {
    writer: BufWriter<Arc<File>>,
    file: Arc<File>,
    bytes: u64,
    seg: u64,
    next_sync_due_ns: u64,
    /// Highest LSN successfully written by this instance (not necessarily
    /// durable). Authoritative: never derived from `lsn_counter`.
    written_lsn: u64,
    /// Highest LSN known to have crossed the durability barrier.
    /// Invariant: `written_lsn >= durable_lsn`.
    durable_lsn: u64,
    /// (segment, final LSN) for segments that can no longer be appended to.
    /// Seeded from the recovery scan, extended on rotation. This is the only
    /// source of retention metadata — there is no second record parser.
    closed_segments: Vec<(u64, u64)>,
    state: WalState,
    /// Test seam: makes the next `append` take the write-failure branch.
    /// `ponytail:` a flag instead of a `Box<dyn Write>` backend — the only
    /// thing under test is the poison transition, which is identical for an
    /// injected and a real `write_all` error. Swap in a fault-injecting
    /// writer when a test needs to fail *mid-payload* specifically.
    fail_next_write: bool,
    /// Test seam: makes the next `append` fail with `ENOSPC` (disk full)
    /// instead of a generic write error. Proves the `Error::from_io`
    /// mapping that routes `StorageFull` to `Error::DiskFull` is reachable
    /// through the full append→sync→poison path.
    fail_next_diskfull: bool,
    /// Test seam: makes the next rotation's `fsync_dir` fail. Same
    /// rationale as `fail_next_write` — a directory that cannot be
    /// fsynced leaves a new segment that may not survive a crash, so the
    /// instance is poisoned exactly as for a data fsync failure.
    fail_next_dir_fsync: bool,
}

impl Inner {
    /// Flush buffered bytes and fsync the file.
    /// Advancing `durable_lsn` is the caller's job — it must know the
    /// in-flight LSN range covered by this barrier.
    fn sync_inner(&mut self) -> std::io::Result<()> {
        self.writer.flush()?;
        self.file.sync_data()?;
        Ok(())
    }

    /// Group-Commit: if the window has elapsed, flush + fsync and reset the timer.
    /// Returns true if a barrier was issued.
    fn maybe_group_commit(&mut self) -> std::io::Result<bool> {
        let now = now_wall_ns();
        if now >= self.next_sync_due_ns {
            self.writer.flush()?;
            self.file.sync_data()?;
            self.next_sync_due_ns = now + GROUP_COMMIT_WINDOW_NS;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl Wal {
    pub fn open(config: Config) -> Result<Self, Error> {
        if config.seg_max_bytes < HEADER_SIZE as u64 {
            return Err(Error::InvalidConfiguration(
                "seg_max_bytes must be at least one record header".to_string(),
            ));
        }
        if config.buffer_capacity == 0 {
            return Err(Error::InvalidConfiguration(
                "buffer_capacity must be greater than zero".to_string(),
            ));
        }
        if config.retention_max_bytes > 0 && config.retention_max_bytes < HEADER_SIZE as u64 {
            return Err(Error::InvalidConfiguration(
                "retention_max_bytes is too small".to_string(),
            ));
        }

        let dir = config.dir.clone();
        std::fs::create_dir_all(&dir).map_err(Error::from_io)?;
        fsync_dir(&dir).map_err(Error::from_io)?;

        let recovery = recover(&dir)?;

        let manifest_path = PathBuf::from(&dir).join(MANIFEST_NAME);
        let ckpt_lsn: u64 = if manifest_path.exists() {
            std::fs::read_to_string(&manifest_path)
                .map_err(Error::from_io)?
                .lines()
                .find_map(|l| {
                    l.strip_prefix("lsn=")
                        .and_then(|v| v.trim().parse::<u64>().ok())
                })
                .ok_or_else(|| Error::Manifest("missing lsn= field".to_string()))?
        } else {
            0
        };

        // A checkpoint the WAL cannot substantiate makes the manifest a
        // claim about state that does not exist on disk.
        let recovered_last = recovery.report.last_lsn.map_or(0, Lsn::get);
        if ckpt_lsn > recovered_last {
            return Err(Error::Manifest(format!(
                "checkpoint LSN {ckpt_lsn} exceeds recovered WAL LSN {recovered_last}"
            )));
        }

        // Never overflow while establishing the counter: a WAL recovered at
        // u64::MAX starts terminal, and `append` reports LSN exhaustion.
        let start_lsn = match recovery.report.last_lsn {
            None => 1,
            Some(last) => last.checked_next().map_or(u64::MAX, Lsn::get),
        };

        let closed_segments: Vec<(u64, u64)> = recovery
            .report
            .segment_reports
            .iter()
            .filter_map(|s| s.last_lsn.map(|l| (s.segment, l.get())))
            .collect();

        let path = seg_path(&dir, recovery.append_position.segment.get());
        let file = Arc::new(open_segment(&path).map_err(Error::from_io)?);

        let wal = Self {
            inner: Arc::new(Mutex::new(Inner {
                writer: BufWriter::with_capacity(config.buffer_capacity, Arc::clone(&file)),
                file,
                bytes: recovery.append_position.offset.get(),
                seg: recovery.append_position.segment.get(),
                next_sync_due_ns: 0,
                written_lsn: recovery.report.last_lsn.map_or(0, Lsn::get),
                durable_lsn: recovery.report.last_lsn.map_or(0, Lsn::get),
                closed_segments,
                state: WalState::Open,
                fail_next_write: false,
                fail_next_diskfull: false,
                fail_next_dir_fsync: false,
            })),
            compression: config.compression,
            durability: config.durability,
            seg_max: config.seg_max_bytes,
            retention_max: config.retention_max_bytes,
            base_dir: dir,
            lsn_counter: AtomicU64::new(start_lsn),
            ckpt_lsn: AtomicU64::new(ckpt_lsn),
            recovery,
            buffer_capacity: config.buffer_capacity,
            checkpoint_lock: Mutex::new(()),
        };

        if wal.retention_max > 0 {
            wal.run_retention(ckpt_lsn)?;
        }

        Ok(wal)
    }

    /// Append one record. Returns its `Lsn`.
    ///
    /// Durability is governed by `config.durability`:
    /// - `Buffered`   — write to the buffer only.
    /// - `Explicit`   — write to the buffer only; call `sync()` to make durable.
    /// - `SyncEach`   — flush + fsync on every append (no timing gate).
    /// - `GroupCommit`— flush + fsync when the commit window is due.
    pub fn append(&self, payload: &[u8]) -> Result<Lsn, Error> {
        if payload.len() > MAX_RECORD_SIZE as usize {
            return Err(Error::InvalidConfiguration(format!(
                "record size {} exceeds max {}",
                payload.len(),
                MAX_RECORD_SIZE
            )));
        }
        let compress = matches!(self.compression, Compression::Lz4);
        let (stored, flags) = encode_payload(payload, compress, 64);

        // LSN is allocated under the same lock that orders the writes.
        // Allocating it before the lock lets two threads swap order,
        // which recovery correctly rejects as an LSN regression.
        let mut g = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if matches!(g.state, WalState::Poisoned) {
            return Err(Error::Poisoned);
        }

        // Storage primitives should not silently wrap identity.
        if self.lsn_counter.load(Ordering::SeqCst) == u64::MAX {
            return Err(Error::InvalidConfiguration(
                "LSN space exhausted".to_string(),
            ));
        }
        let lsn = self.lsn_counter.fetch_add(1, Ordering::SeqCst);
        let lsn_obj = Lsn::new(lsn);
        let rh = RecordHeader::new(lsn_obj, &stored, flags);

        let mut hdr_buf = [0u8; HEADER_SIZE];
        rh.encode(&mut hdr_buf);

        let must_rotate = {
            if g.fail_next_write {
                g.fail_next_write = false;
                let e = std::io::Error::other("injected write failure");
                g.state = WalState::Poisoned;
                return Err(Error::from_io(e));
            }
            if g.fail_next_diskfull {
                g.fail_next_diskfull = false;
                let e = std::io::Error::from_raw_os_error(28 /* ENOSPC */);
                g.state = WalState::Poisoned;
                return Err(Error::from_io(e));
            }
            if let Err(e) = g.writer.write_all(&hdr_buf) {
                g.state = WalState::Poisoned;
                return Err(Error::from_io(e));
            }
            if let Err(e) = g.writer.write_all(&stored) {
                g.state = WalState::Poisoned;
                return Err(Error::from_io(e));
            }
            g.bytes += (HEADER_SIZE + stored.len()) as u64;
            g.written_lsn = lsn;

            // A successful barrier makes everything up to `written_lsn` durable.
            let barrier = match self.durability {
                Durability::Buffered | Durability::Explicit => false,
                Durability::SyncEach => {
                    if let Err(e) = g.sync_inner() {
                        g.state = WalState::Poisoned;
                        return Err(Error::from_io(e));
                    }
                    true
                }
                Durability::GroupCommit => match g.maybe_group_commit() {
                    Ok(b) => b,
                    Err(e) => {
                        g.state = WalState::Poisoned;
                        return Err(Error::from_io(e));
                    }
                },
            };
            if barrier {
                g.durable_lsn = g.written_lsn;
            }

            // Segment rotation always flushes + fsyncs the old segment before
            // switching, so no buffered bytes are lost on rotation.
            if g.bytes >= self.seg_max {
                if let Err(e) = g.sync_inner() {
                    g.state = WalState::Poisoned;
                    return Err(Error::from_io(e));
                }
                g.durable_lsn = g.written_lsn;
                let closed = g.seg;
                let closed_last = g.written_lsn;
                // Update the entry if the recovery scan already seeded this
                // segment (it seeded the segment that was active at open, and
                // that is usually the one being closed now). One entry per
                // segment, holding its true last LSN.
                match g.closed_segments.iter_mut().find(|(s, _)| *s == closed) {
                    // `max`, not a plain assignment: LSNs are monotonic so the
                    // two are equivalent today, but taking the larger value
                    // keeps the entry a true upper bound even if the
                    // invariant is ever broken.
                    Some(entry) => entry.1 = entry.1.max(closed_last),
                    None => g.closed_segments.push((closed, closed_last)),
                }
                let new_seg = g.seg + 1;
                let path = seg_path(&self.base_dir, new_seg);
                let new_file = match crate::segment::create_segment_truncate(&path) {
                    Ok(f) => Arc::new(f),
                    Err(e) => {
                        g.state = WalState::Poisoned;
                        return Err(Error::from_io(e));
                    }
                };
                g.writer = BufWriter::with_capacity(self.buffer_capacity, Arc::clone(&new_file));
                g.file = new_file;
                g.bytes = 0;
                g.seg = new_seg;
                true
            } else {
                false
            }
        };

        // fsync_dir runs BEFORE the mutex is released: the new segment's
        // directory entry must be durable before any other thread can append
        // to it, otherwise a crash could leave durable file contents with no
        // durable directory entry (segment lost on recovery).
        if must_rotate {
            // `g` still owns `self.inner` here. Read and consume the injected
            // failure through that guard rather than locking `self.inner`
            // again: the mutex is non-reentrant, so reacquiring it here
            // deadlocks every append that rotates a segment.
            if g.fail_next_dir_fsync {
                g.fail_next_dir_fsync = false;
                g.state = WalState::Poisoned;
                return Err(Error::from_io(std::io::Error::other(
                    "injected dir fsync failure",
                )));
            }
            if let Err(e) = fsync_dir(&self.base_dir) {
                g.state = WalState::Poisoned;
                return Err(Error::from_io(e));
            }
        }

        // Keep the lock through directory fsync so no append can race ahead
        // of the durability barrier. Release it before retention, which takes
        // the same non-reentrant mutex internally.
        drop(g);
        if must_rotate && self.retention_max > 0 {
            // Retention errors are logged and ignored per spec §20.2-3.
            // We do not propagate them from append.
            let _ = self.run_retention(self.ckpt_lsn.load(Ordering::SeqCst));
        }

        Ok(lsn_obj)
    }

    /// Test seam: make the next `append` fail at the write site so the
    /// poison transition can be exercised without a failing filesystem.
    #[doc(hidden)]
    pub fn fail_next_write(&self) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fail_next_write = true;
    }

    /// Test seam: make the next rotation's `fsync_dir` fail.
    ///
    /// A directory that cannot be fsynced after rotation means the new
    /// segment's existence is not guaranteed on disk — the instance is
    /// poisoned exactly as for a data fsync failure.
    #[doc(hidden)]
    pub fn fail_next_dir_fsync(&self) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fail_next_dir_fsync = true;
    }

    /// Test seam: make the next `append` fail with an `ENOSPC` disk-full
    /// error. This exercises `Error::from_io`'s mapping of `StorageFull`
    /// to the public `Error::DiskFull` variant through the real append
    /// path — the closest a unit test can come to a full disk without
    /// one.
    #[doc(hidden)]
    pub fn fail_next_diskfull(&self) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fail_next_diskfull = true;
    }

    /// True once a write-side storage failure has poisoned this instance.
    /// A poisoned WAL rejects every mutating operation; reopen it to run
    /// recovery and establish a new verified append boundary.
    pub fn is_poisoned(&self) -> bool {
        matches!(
            self.inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .state,
            WalState::Poisoned
        )
    }

    pub fn flush(&self) -> Result<(), Error> {
        let mut g = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(g.state, WalState::Poisoned) {
            return Err(Error::Poisoned);
        }
        if let Err(e) = g.writer.flush() {
            g.state = WalState::Poisoned;
            return Err(Error::from_io(e));
        }
        Ok(())
    }

    /// Flush buffered bytes and fsync the active segment.
    ///
    /// Contract: when this returns `Ok`, all bytes written by prior
    /// successful `append` calls have crossed the configured durability
    /// barrier and are durable.
    pub fn sync(&self) -> Result<(), Error> {
        let mut g = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(g.state, WalState::Poisoned) {
            return Err(Error::Poisoned);
        }
        if let Err(e) = g.sync_inner() {
            g.state = WalState::Poisoned;
            return Err(Error::from_io(e));
        }
        g.durable_lsn = g.written_lsn;
        Ok(())
    }

    /// Highest LSN known durable.
    pub fn durable_lsn(&self) -> Lsn {
        Lsn::new(
            self.inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .durable_lsn,
        )
    }

    /// The recovery report computed when this WAL was opened.
    pub fn recovery_report(&self) -> &RecoveryReport {
        &self.recovery.report
    }

    /// Record a checkpoint at `lsn`. The application is responsible for
    /// persisting a snapshot up through this LSN before calling.
    ///
    /// Three invariants are enforced:
    /// 1. the LSN exists;
    /// 2. it is durable;
    /// 3. checkpoints never move backwards.
    pub fn checkpoint(&self, lsn: Lsn) -> Result<(), Error> {
        if self.is_poisoned() {
            return Err(Error::Poisoned);
        }
        // Serialize the full checkpoint transaction. Acquired before the
        // append mutex, and `append` never takes this lock, so the two can
        // never form a wait cycle. The append mutex is released before
        // manifest I/O; this lock is held until the transaction completes.
        let _ckpt_guard = self
            .checkpoint_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let g = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = g.written_lsn;
        let durable = g.durable_lsn;
        let previous = self.ckpt_lsn();

        if lsn > Lsn::new(current) {
            return Err(Error::InvalidConfiguration(format!(
                "checkpoint {} is beyond current LSN {}",
                lsn.get(),
                current
            )));
        }

        if lsn > Lsn::new(durable) {
            return Err(Error::CheckpointNotDurable {
                requested: lsn,
                durable: Lsn::new(durable),
            });
        }

        if lsn < previous {
            return Err(Error::CheckpointRegression {
                current: previous,
                requested: lsn,
            });
        }

        let manifest_path = PathBuf::from(&self.base_dir).join(MANIFEST_NAME);
        let tmp_path = manifest_path.with_extension("tmp");
        drop(g); // release append lock before filesystem I/O
        let storage_result = (|| {
            let mut f = {
                let mut opts = std::fs::OpenOptions::new();
                opts.create(true).write(true).truncate(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    opts.mode(0o600);
                }
                opts.open(&tmp_path).map_err(Error::from_io)?
            };

            writeln!(f, "lsn={}", lsn.get()).map_err(Error::from_io)?;

            f.sync_all().map_err(Error::from_io)?;

            std::fs::rename(&tmp_path, &manifest_path).map_err(Error::from_io)?;

            fsync_dir(&self.base_dir).map_err(Error::from_io)?;

            Ok::<(), Error>(())
        })();

        if let Err(err) = storage_result {
            let mut state = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            state.state = WalState::Poisoned;

            return Err(err);
        }

        self.ckpt_lsn.store(lsn.get(), Ordering::SeqCst);

        Ok(())
    }

    /// Delete segments whose highest LSN is below `lsn`.
    /// Rejects `lsn > checkpoint` — you must checkpoint first.
    pub fn truncate_before(&self, lsn: Lsn) -> Result<(), Error> {
        if self.is_poisoned() {
            return Err(Error::Poisoned);
        }
        let ckpt = self.ckpt_lsn();
        if lsn > ckpt {
            return Err(Error::CheckpointRequired {
                requested: lsn,
                checkpoint: ckpt,
            });
        }
        self.run_retention_raw(0, lsn.get())
    }

    /// Retention, using the closed-segment metadata gathered by recovery and
    /// extended on rotation. No independent record parser is involved.
    ///
    /// Policy: `max_bytes` is a target, not permission to destroy history no
    /// checkpoint covers. With `safe_lsn == 0` this is a no-op.
    fn run_retention(&self, safe_lsn: u64) -> Result<(), Error> {
        if safe_lsn == 0 {
            return Ok(());
        }
        self.run_retention_raw(self.retention_max, safe_lsn)
    }

    fn run_retention_raw(&self, max_bytes: u64, safe_lsn: u64) -> Result<(), Error> {
        if self.is_poisoned() {
            return Err(Error::Poisoned);
        }
        if safe_lsn == 0 {
            return Ok(());
        }

        let (active, closed) = {
            let g = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (g.seg, g.closed_segments.clone())
        };

        let segs = segment::list_segs_with_size(&self.base_dir).map_err(Error::from_io)?;
        let mut over: u64 = 0;
        if max_bytes > 0 {
            let total: u64 = segs.iter().map(|&(_, _, sz)| sz).sum();
            if total <= max_bytes {
                return Ok(());
            }
            over = total - max_bytes;
        }

        for &(idx, ref path, sz) in &segs {
            if max_bytes > 0 && over == 0 {
                break;
            }
            if idx == active {
                continue;
            }
            // The segment's last LSN must be the MAXIMUM recorded for it,
            // not the first.
            //
            // `closed_segments` is seeded from the recovery scan, which
            // includes the segment the writer goes on to use. When that
            // segment later rotates it is pushed onto the list AGAIN, with
            // its true (larger) last LSN. Taking the first match therefore
            // saw the STALE seeded value — the segment's last LSN as of
            // recovery, before this run appended anything — and the
            // `last_lsn >= safe_lsn` guard then passed for a segment that
            // actually held required history. The segment was deleted:
            // required WAL history silently lost.
            //
            // The maximum is the segment's true last LSN, so the guard
            // correctly keeps any segment that still holds records at or
            // above the safe LSN.
            let Some(last_lsn) = closed
                .iter()
                .filter(|&&(s, _)| s == idx)
                .map(|&(_, l)| l)
                .max()
            else {
                continue;
            };
            if last_lsn >= safe_lsn {
                continue;
            }
            std::fs::remove_file(path).map_err(Error::from_io)?;
            over = over.saturating_sub(sz);
        }
        Ok(())
    }

    pub fn ckpt_lsn(&self) -> Lsn {
        Lsn::new(self.ckpt_lsn.load(Ordering::SeqCst))
    }

    /// Highest LSN appended by this instance (may not yet be durable).
    ///
    /// Read from the writer rather than from the allocation counter: at LSN
    /// exhaustion the counter sits at `u64::MAX` without having been consumed,
    /// so `counter - 1` would under-report the last real LSN by one and make
    /// `checkpoint` reject a valid final record.
    pub fn current_lsn(&self) -> Lsn {
        Lsn::new(
            self.inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .written_lsn,
        )
    }

    pub fn base_dir(&self) -> &str {
        &self.base_dir
    }
}

fn now_wall_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
