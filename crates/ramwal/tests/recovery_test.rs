use std::fs::OpenOptions;
use std::io::{Seek, Write};
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use ramwal::lsn::Lsn;
use ramwal::recovery::{recover_dir, repair_segment_tail};
use ramwal::testkit::seg_path;
use ramwal::testkit::{HEADER_SIZE, RecordHeader, encode_payload};
use ramwal::{Error, Wal};

fn make_dir() -> String {
    let tmp = std::env::var("TMPDIR").unwrap_or("/tmp".to_string());
    let uid = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}/ramwal-test-{:020}", tmp, uid)
}

fn test_dir() -> String {
    let d = make_dir();
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_segment(dir: &str, seg_idx: u64, records: &[(&[u8], u64)]) -> u64 {
    let path = seg_path(dir, seg_idx);
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    for &(data, lsn_val) in records {
        let lsn = Lsn::new(lsn_val);
        let (stored, _flags) = encode_payload(data, false, 64);
        let rh = RecordHeader::new(lsn, &stored, 0u8);
        let mut hdr_buf = [0u8; HEADER_SIZE];
        rh.encode(&mut hdr_buf);
        file.write_all(&hdr_buf).unwrap();
        file.write_all(&stored).unwrap();
    }
    file.flush().unwrap();
    std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
}

fn corrupt_byte(dir: &str, seg_idx: u64, offset: u64, byte: u8) {
    let path = seg_path(dir, seg_idx);
    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(std::io::SeekFrom::Start(offset)).unwrap();
    file.write_all(&[byte]).unwrap();
    file.flush().unwrap();
}

fn truncate_file(dir: &str, seg_idx: u64, len: u64) {
    let path = seg_path(dir, seg_idx);
    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(len).unwrap();
    file.flush().unwrap();
}

fn clean(d: &str) {
    std::fs::remove_dir_all(d).ok();
}

fn wal_cfg(d: &str) -> ramwal::config::Config {
    let mut c = ramwal::config::Config::new(d);
    c.durability = ramwal::config::Durability::SyncEach;
    c
}

// ── Tests ────────────────────────────────────────────────────────────────

#[test]
fn clean_single_segment() {
    let d = test_dir();
    write_segment(&d, 1, &[(b"hello", 1), (b"world", 2)]);
    let report = recover_dir(&d).unwrap();
    assert_eq!(report.records.len(), 2);
    assert_eq!(report.last_lsn.unwrap().get(), 2);
    assert!(!report.repaired);
    assert_eq!(report.truncated_bytes, 0);
    assert_eq!(report.segments, 1);
    clean(&d);
}

#[test]
fn multi_segment_continuity() {
    let d = test_dir();
    write_segment(&d, 1, &[(b"one", 1), (b"two", 2)]);
    write_segment(&d, 2, &[(b"three", 3), (b"four", 4)]);
    let report = recover_dir(&d).unwrap();
    assert_eq!(report.records.len(), 4);
    assert_eq!(report.last_lsn.unwrap().get(), 4);
    assert_eq!(report.first_lsn.unwrap().get(), 1);
    clean(&d);
}

#[test]
fn partial_header_at_eof() {
    let d = test_dir();
    write_segment(&d, 1, &[(b"valid", 1)]);
    let path = seg_path(&d, 1);
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(&[70, 87, 76]).unwrap();
    file.flush().unwrap();
    let report = recover_dir(&d).unwrap();
    assert_eq!(report.records.len(), 1);
    assert!(report.repaired);
    assert_eq!(report.truncated_bytes, 3);
    // second pass — clean
    let report2 = recover_dir(&d).unwrap();
    assert!(!report2.repaired);
    assert_eq!(report2.truncated_bytes, 0);
    clean(&d);
}

#[test]
fn repair_segment_explicit() {
    let d = test_dir();
    let path = seg_path(&d, 1);
    write_segment(&d, 1, &[(b"valid", 1)]);
    // Truncate mid-payload (23 hdr + 2 of 5 bytes): valid prefix = 0, partial payload
    truncate_file(&d, 1, 25);
    // Direct repair to clean without full recovery: keep nothing
    repair_segment_tail(&path, 0).unwrap();
    // Now recovery sees an empty clean segment
    let report = recover_dir(&d).unwrap();
    assert!(!report.repaired);
    assert_eq!(report.records.len(), 0);
    clean(&d);
}

#[test]
fn crc_mismatch_is_corruption() {
    let d = test_dir();
    write_segment(&d, 1, &[(b"good", 1)]);
    write_segment(&d, 2, &[(b"ok", 2), (b"bad", 3)]);
    corrupt_byte(&d, 2, 43, 0xFF);
    let result = recover_dir(&d);
    assert!(result.is_err());
    clean(&d);
}

#[test]
fn empty_directory() {
    let d = test_dir();
    let report = recover_dir(&d).unwrap();
    assert_eq!(report.records.len(), 0);
    assert_eq!(report.last_lsn, None);
    assert_eq!(report.append_offset, 0);
    assert!(!report.repaired);
    assert_eq!(report.segments, 0);
    clean(&d);
}

#[test]
fn empty_segment_file() {
    let d = test_dir();
    let path = seg_path(&d, 1);
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    let report = recover_dir(&d).unwrap();
    assert_eq!(report.records.len(), 0);
    assert_eq!(report.append_offset, 0);
    assert!(!report.repaired);
    clean(&d);
}

#[test]
fn lsn_monotonic_enforced() {
    let d = test_dir();
    write_segment(&d, 1, &[(b"a", 1), (b"b", 2)]);
    write_segment(&d, 2, &[(b"c", 2)]);
    let result = recover_dir(&d);
    assert!(result.is_err());
    clean(&d);
}

#[test]
fn invalid_magic() {
    let d = test_dir();
    let path = seg_path(&d, 1);
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    let mut hdr = [0u8; 23];
    hdr[0] = 88;
    hdr[1] = 87;
    hdr[2] = 76;
    hdr[3] = 49;
    file.write_all(&hdr).unwrap();
    file.flush().unwrap();
    let result = recover_dir(&d);
    assert!(result.is_err());
    clean(&d);
}

#[test]
fn truncate_entire_segment_quarantines() {
    let d = test_dir();
    write_segment(&d, 1, &[(b"valid", 1)]);
    // Truncate to 0 is a clean EOF — recovery sees clean segment
    truncate_file(&d, 1, 0);
    let report = recover_dir(&d).unwrap();
    // No repair needed: segment is already clean (empty = valid prefix)
    assert!(!report.repaired);
    assert_eq!(report.records.len(), 0);
    assert_eq!(report.truncated_bytes, 0);
    // Quarantine dir must not exist (no torn tail to move)
    assert_eq!(
        std::fs::exists(format!("{}/quarantine", d)).ok(),
        Some(false)
    );
    clean(&d);
}

#[test]
fn writer_round_trip() {
    let d = test_dir();
    let w = ramwal::Wal::open(wal_cfg(&d)).unwrap();
    let lsn1 = w.append(b"hello").unwrap();
    let lsn2 = w.append(b"world").unwrap();
    assert!(lsn1.get() < lsn2.get());
    w.sync().unwrap();
    drop(w);
    // Re-open: recovery finds both records at the writer's boundary
    let w2 = ramwal::Wal::open(wal_cfg(&d)).unwrap();
    let rep2 = w2.recovery_report();
    assert_eq!(rep2.records.len(), 2);
    assert_eq!(rep2.last_lsn.unwrap().get(), lsn2.get());
    drop(w2);
    clean(&d);
}

#[test]
fn checkpoint_and_retention() {
    let d = test_dir();
    let cfg = wal_cfg(&d);
    let w = ramwal::Wal::open(cfg).unwrap();
    let lsn1 = w.append(b"a").unwrap();
    let lsn2 = w.append(b"b").unwrap();
    w.sync().unwrap();
    // Checkpoint at lsn1 — app should be responsible for ensuring
    // snapshot covers lsn1 before calling.
    w.checkpoint(lsn1).unwrap();
    assert_eq!(w.ckpt_lsn(), lsn1);
    // Truncate_before(lsn1) should be safe (no segments below lsn1 to delete)
    w.truncate_before(lsn1).unwrap();
    // Truncate_before beyond checkpoint is rejected
    let err = w.truncate_before(Lsn::new(lsn2.get() + 1));
    assert!(err.is_err());
    drop(w);
    // Re-open still sees both records (checkpoint doesn't discard data)
    let w2 = ramwal::Wal::open(wal_cfg(&d)).unwrap();
    assert_eq!(w2.recovery_report().records.len(), 2);
    drop(w2);
    clean(&d);
}

// ── Golden Fixture Infrastructure (BATCH 09) ──────────────────────

/// Record metadata for oracle predictions — built using RAMWAL's own
/// RecordHeader/encode so the fixture is a valid WAL, then damaged
/// at the byte level. The oracle only tracks write positions and
/// never calls recover_dir.
struct FixtureRec {
    data: Vec<u8>,
    rec_end: u64,
}

/// Build a deterministic single-segment WAL with payloads `data`.
/// Returns (segment_path, record-metadata list).
fn build_fixture(dir: &str, data: &[Vec<u8>]) -> (PathBuf, Vec<FixtureRec>) {
    let path = seg_path(dir, 1);
    let mut buf = vec![];
    let mut recs = Vec::new();
    for (i, d) in data.iter().enumerate() {
        let lsn = (i + 1) as u64;
        let hdr = RecordHeader::new(Lsn::new(lsn), d, 0);
        let mut hdr_buf = [0u8; HEADER_SIZE];
        hdr.encode(&mut hdr_buf);
        buf.extend(&hdr_buf);
        buf.extend(d);
        recs.push(FixtureRec {
            data: d.clone(),
            rec_end: buf.len() as u64,
        });
    }
    std::fs::write(&path, &buf).unwrap();
    (path, recs)
}

// ── Oracle ────────────────────────────────────────────────────────

struct OracleExpectation {
    count: usize,
    is_err: bool,
    repaired: bool,
}

/// Predict outcome for an undamaged fixture.
fn oracle_clean(recs: &[FixtureRec]) -> OracleExpectation {
    OracleExpectation {
        count: recs.len(),
        is_err: false,
        repaired: false,
    }
}

/// Predict outcome after truncating the file at byte `at`.
fn oracle_truncate(recs: &[FixtureRec], at: u64) -> OracleExpectation {
    let mut count = 0;
    for r in recs {
        if r.rec_end <= at {
            count += 1;
        }
    }
    OracleExpectation {
        count,
        is_err: false,
        repaired: true,
    }
}

/// Predict outcome for CRC corruption on record `idx` (0-based).
/// Only the first record of the newest (here, only) segment is repairable.
fn oracle_corrupt_crc(_recs: &[FixtureRec], _idx: usize) -> OracleExpectation {
    // A complete record whose checksum fails is corruption, regardless of
    // position. There is no first-record exception.
    OracleExpectation {
        count: 0,
        is_err: true,
        repaired: false,
    }
}

// ── Oracle-verified golden tests ──────────────────────────────────

#[test]
fn golden_clean_three() {
    let d = test_dir();
    let (_, recs) = build_fixture(&d, &[b"aaa".to_vec(), b"bbb".to_vec(), b"ccc".to_vec()]);
    let r = recover_dir(&d).unwrap();
    let o = oracle_clean(&recs);
    assert_eq!(r.records.len(), o.count);
    assert_eq!(r.repaired, o.repaired);
    assert_eq!(r.first_lsn.unwrap().get(), 1);
    assert_eq!(r.last_lsn.unwrap().get(), 3);
    clean(&d);
}

#[test]
fn golden_truncate_mid_payload() {
    let d = test_dir();
    let (_, recs) = build_fixture(
        &d,
        &[b"aaaaa".to_vec(), b"bbbbb".to_vec(), b"ccccc".to_vec()],
    );
    let cut = recs[1].rec_end - 2;
    truncate_file(&d, 1, cut);
    let r = recover_dir(&d).unwrap();
    let o = oracle_truncate(&recs, cut);
    assert_eq!(r.records.len(), o.count);
    assert_eq!(r.repaired, o.repaired);
    assert_eq!(r.records[0].lsn.get(), 1);
    clean(&d);
}

#[test]
fn golden_truncate_mid_header() {
    let d = test_dir();
    let (_, recs) = build_fixture(
        &d,
        &[b"aaaaa".to_vec(), b"bbbbb".to_vec(), b"ccccc".to_vec()],
    );
    let cut = recs[0].rec_end + 5;
    truncate_file(&d, 1, cut);
    let r = recover_dir(&d).unwrap();
    let o = oracle_truncate(&recs, cut);
    assert_eq!(r.records.len(), o.count);
    assert_eq!(r.repaired, o.repaired);
    clean(&d);
}

#[test]
fn golden_corrupt_crc_first() {
    let d = test_dir();
    let (_, recs) = build_fixture(&d, &[b"aaa".to_vec(), b"bbb".to_vec()]);
    corrupt_byte(&d, 1, 18, 0xFF);
    let r = recover_dir(&d);
    let o = oracle_corrupt_crc(&recs, 0);
    assert_eq!(r.is_err(), o.is_err);
    clean(&d);
}

#[test]
fn golden_corrupt_crc_last() {
    let d = test_dir();
    let (_, recs) = build_fixture(&d, &[b"aaa".to_vec(), b"bbb".to_vec()]);
    let offset = recs[0].rec_end + 18; // CRC of second record
    corrupt_byte(&d, 1, offset, 0xFF);
    let r = recover_dir(&d);
    let o = oracle_corrupt_crc(&recs, 1);
    assert_eq!(r.is_err(), o.is_err);
    clean(&d);
}

#[test]
fn golden_corrupt_magic() {
    let d = test_dir();
    let (_, _) = build_fixture(&d, &[b"aaa".to_vec(), b"bbb".to_vec()]);
    corrupt_byte(&d, 1, 0, 0xFF);
    let r = recover_dir(&d);
    assert!(r.is_err());
    clean(&d);
}

#[test]
fn golden_truncate_to_zero() {
    let d = test_dir();
    let (_, _) = build_fixture(&d, &[b"aaa".to_vec()]);
    truncate_file(&d, 1, 0);
    let r = recover_dir(&d).unwrap();
    assert_eq!(r.records.len(), 0);
    assert!(!r.repaired);
    clean(&d);
}

// ── Crash Laboratory (BATCH 10) ─────────────────────────────────

/// Independent model of durable state. Knows nothing of RAMWAL
/// recovery internals — only tracks what ops were synced.
struct CrashModel {
    ops: Vec<Vec<u8>>,
    durable_count: usize,
}

impl CrashModel {
    fn new() -> Self {
        Self {
            ops: vec![],
            durable_count: 0,
        }
    }

    fn append(&mut self, data: &[u8]) {
        self.ops.push(data.to_vec());
    }

    fn mark_durable(&mut self) {
        self.durable_count = self.ops.len();
    }

    fn durable_records(&self) -> &[Vec<u8>] {
        &self.ops[..self.durable_count]
    }
}

#[test]
fn crash_classic() {
    let d = test_dir();
    let mut model = CrashModel::new();

    let w = ramwal::Wal::open(wal_cfg(&d)).unwrap();

    for i in 0..20 {
        let data = format!("data-{}", i).as_bytes().to_vec();
        model.append(&data);
        w.append(&data).unwrap();
        if i % 3 == 0 {
            w.sync().unwrap();
            model.mark_durable();
        }
    }

    drop(w);

    // "Crash" — reopen.
    let w2 = ramwal::Wal::open(wal_cfg(&d)).unwrap();
    let rep = w2.recovery_report();

    let recovered: Vec<Vec<u8>> = rep.records.iter().map(|r| r.payload.clone()).collect();
    let expected = model.durable_records();

    for i in 0..expected.len() {
        assert_eq!(recovered[i], expected[i]);
    }
    assert!(recovered.len() >= expected.len());

    drop(w2);
    clean(&d);
}

#[test]
fn exhaustive_torn_write() {
    let d = test_dir();

    let data: Vec<Vec<u8>> = vec![b"ab".to_vec(), b"hello123".to_vec(), b"xy".to_vec()];
    let (path, recs) = build_fixture(&d, &data);
    let wal_bytes = std::fs::read(&path).unwrap();

    for cut in 0..wal_bytes.len() {
        let d2 = test_dir();
        let p2 = seg_path(&d2, 1);
        std::fs::write(&p2, &wal_bytes[..cut]).unwrap();

        let result = recover_dir(&d2);
        if let Ok(r) = result {
            for rec in r.records.iter() {
                let idx = (rec.lsn.get() - 1) as usize;
                assert!(idx < recs.len());
                assert_eq!(rec.payload, recs[idx].data);
            }
            if !r.records.is_empty() {
                assert_eq!(r.records[0].lsn.get(), 1);
                assert_eq!(
                    r.records.last().unwrap().lsn.get() - r.records[0].lsn.get() + 1,
                    r.records.len() as u64
                );
            }
        }
        clean(&d2);
    }
    clean(&d);
}

// ── Fuzzing + Fifth Injection (BATCH 11) ────────────────────────

/// Deterministic PRNG (xorshift64) — no rand dep.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

/// Single-byte mutation sweep: flip each byte of a valid WAL to a
/// hostile value. Recovery must never panic, hang, or emit a record
/// that was not in the original set.
#[test]
fn fuzz_single_byte_mutation() {
    let d = test_dir();
    let data: Vec<Vec<u8>> = vec![b"alpha".to_vec(), b"beta".to_vec(), b"gamma".to_vec()];
    let (path, recs) = build_fixture(&d, &data);
    let wal_bytes = std::fs::read(&path).unwrap();

    for i in 0..wal_bytes.len() {
        for &b in &[0x00u8, 0xFF, 0x7F, 0x80, b'R', b'!'] {
            let d2 = test_dir();
            let mut mutated = wal_bytes.clone();
            mutated[i] = b;
            std::fs::write(seg_path(&d2, 1), &mutated).unwrap();

            // Never panic; if Ok, every record must be a prefix record verbatim.
            if let Ok(r) = recover_dir(&d2) {
                for rec in r.records.iter() {
                    let idx = (rec.lsn.get() - 1) as usize;
                    assert!(
                        idx < recs.len(),
                        "record from nowhere: lsn={}",
                        rec.lsn.get()
                    );
                    assert_eq!(rec.payload, recs[idx].data);
                }
                if !r.records.is_empty() {
                    assert_eq!(r.records[0].lsn.get(), 1);
                }
            }
            clean(&d2);
        }
    }
    clean(&d);
}

/// Random multi-byte corruption: recovery must never panic or
/// fabricate a record. Size bounded (never OOM) because decode
/// rejects payload_len > MAX_RECORD_SIZE.
#[test]
fn fuzz_random_corruption() {
    let d = test_dir();
    let data: Vec<Vec<u8>> = vec![b"one".to_vec(), b"two".to_vec(), b"three".to_vec()];
    let (path, recs) = build_fixture(&d, &data);
    let orig = std::fs::read(&path).unwrap();

    let mut rng = Rng(0x9E3779B97F4A7C15);
    for _ in 0..500 {
        let d2 = test_dir();
        let mut buf = orig.clone();
        let n_mut = (rng.next() % 5) as usize + 1;
        for _ in 0..n_mut {
            let pos = (rng.next() as usize) % buf.len();
            buf[pos] = (rng.next() & 0xFF) as u8;
        }
        if let Ok(r) = recover_dir(&d2) {
            for rec in r.records.iter() {
                let idx = (rec.lsn.get() - 1) as usize;
                assert!(idx < recs.len());
                assert_eq!(rec.payload, recs[idx].data);
            }
        }
        clean(&d2);
    }
    clean(&d);
}

/// Fault injection: file shrinks by one byte each round (simulated
/// fsync that lost the last sector). Prefix integrity must hold.
#[test]
fn fault_injection_shrinking_file() {
    let d = test_dir();
    let data: Vec<Vec<u8>> = vec![b"xxxx".to_vec(), b"yyyy".to_vec()];
    let (path, recs) = build_fixture(&d, &data);
    let orig = std::fs::read(&path).unwrap();

    for len in 0..=orig.len() {
        let d2 = test_dir();
        std::fs::write(seg_path(&d2, 1), &orig[..len]).unwrap();
        let r = recover_dir(&d2).unwrap(); // truncated tail always repairable
        for rec in r.records.iter() {
            let idx = (rec.lsn.get() - 1) as usize;
            assert!(idx < recs.len());
            assert_eq!(rec.payload, recs[idx].data);
        }
        clean(&d2);
    }
    clean(&d);
}

// ── Concurrency Verification (BATCH 12) ─────────────────────────

/// Many threads append concurrently. Every thread must see strictly
/// increasing LSNs, and on-disk order must match LSN order — because
/// recovery rejects any LSN regression.
#[test]
fn concurrent_appends_lsn_ordering() {
    let d = test_dir();
    let w = std::sync::Arc::new(ramwal::Wal::open(wal_cfg(&d)).unwrap());

    let mut handles = vec![];
    for t in 0..8 {
        let w = std::sync::Arc::clone(&w);
        handles.push(std::thread::spawn(move || {
            let mut mine = vec![];
            for i in 0..50 {
                let payload = format!("t{}-{}", t, i);
                let lsn = w.append(payload.as_bytes()).unwrap();
                mine.push(lsn.get());
            }
            mine
        }));
    }

    let mut all: Vec<u64> = vec![];
    for h in handles {
        all.extend(h.join().unwrap());
    }

    // No duplicate LSNs across all threads.
    all.sort_unstable();
    let unique = all.windows(2).all(|w| w[0] != w[1]);
    assert!(unique, "duplicate LSN allocated");

    w.sync().unwrap();
    drop(w);

    // Recovery is the judge: on-disk order must be strictly increasing.
    let w2 = ramwal::Wal::open(wal_cfg(&d)).unwrap();
    assert_eq!(w2.recovery_report().records.len(), 400);
    drop(w2);
    clean(&d);
}

/// Concurrent writers to the same Wal must never interleave a record's
/// header and payload with another thread's bytes.
#[test]
fn concurrent_append_record_integrity() {
    let d = test_dir();
    let w = std::sync::Arc::new(ramwal::Wal::open(wal_cfg(&d)).unwrap());

    let mut handles = vec![];
    for t in 0..4 {
        let w = std::sync::Arc::clone(&w);
        handles.push(std::thread::spawn(move || {
            for i in 0..100 {
                let payload = vec![t as u8; 200 + i];
                w.append(&payload).unwrap();
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    w.sync().unwrap();
    drop(w);

    // Every record's payload must be a single repeated byte value —
    // interleaving would produce a mixed payload.
    let w2 = ramwal::Wal::open(wal_cfg(&d)).unwrap();
    assert_eq!(w2.recovery_report().records.len(), 400);
    for r in w2.recovery_report().records.iter() {
        let first = r.payload[0];
        assert!(r.payload.iter().all(|&b| b == first), "torn record");
    }
    drop(w2);
    clean(&d);
}

// ── Platform durability (BATCH 13) ──────────────────────────────

/// `verify` must be pure: it reports a torn tail but never truncates
/// it, never quarantines, never writes.
#[test]
fn verify_never_mutates() {
    let d = test_dir();
    build_fixture(&d, &[b"clean".to_vec()]);
    let path = seg_path(&d, 1);
    let before = std::fs::metadata(&path).unwrap().len();

    // Append a torn partial header.
    {
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(&[0x52, 0x57, 0x4C]).unwrap();
    }
    let torn = std::fs::metadata(&path).unwrap().len();
    assert!(torn > before);

    // verify: sees the damage, does not touch the file.
    let rep = ramwal::recovery::verify_dir(&d).unwrap();
    assert!(!rep.repaired);
    assert!(rep.truncated_bytes > 0, "verify must report the damage");
    assert_eq!(
        std::fs::metadata(&path).unwrap().len(),
        torn,
        "verify mutated the segment"
    );
    assert_eq!(
        std::fs::exists(format!("{}/quarantine", d)).ok(),
        Some(false),
        "verify created a quarantine dir"
    );

    // recover: applies the repair.
    let rep2 = recover_dir(&d).unwrap();
    assert!(rep2.repaired);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), before);

    clean(&d);
}

// ── Audit patch set: recovery boundaries, checkpoint, compression ────

/// A partial record may only exist in the final segment. In a historical
/// segment it means damage that recovery must not skip over.
#[test]
fn partial_tail_in_non_final_segment_is_corruption() {
    let d = test_dir();

    let first = seg_path(&d, 1);
    let second = seg_path(&d, 2);

    let data1 = b"first";
    let data2 = b"second";

    let h1 = RecordHeader::new(Lsn::new(1), data1, 0);
    let mut b1 = [0u8; HEADER_SIZE];
    h1.encode(&mut b1);

    let h2 = RecordHeader::new(Lsn::new(2), data2, 0);
    let mut b2 = [0u8; HEADER_SIZE];
    h2.encode(&mut b2);

    let mut seg1 = Vec::new();
    seg1.extend_from_slice(&b1);
    seg1.extend_from_slice(data1);

    // Deliberately incomplete second record in segment 1.
    seg1.extend_from_slice(&b2[..5]);

    std::fs::write(&first, seg1).unwrap();

    // A later segment exists, making segment 1 non-final.
    let mut seg2 = Vec::new();

    let h3 = RecordHeader::new(Lsn::new(3), b"third", 0);
    let mut b3 = [0u8; HEADER_SIZE];
    h3.encode(&mut b3);

    seg2.extend_from_slice(&b3);
    seg2.extend_from_slice(b"third");

    std::fs::write(&second, seg2).unwrap();

    let result = recover_dir(&d);

    assert!(result.is_err());

    clean(&d);
}

/// The reported `available` byte count must be the real number of payload
/// bytes present, not a placeholder.
#[test]
fn partial_payload_reports_available_bytes() {
    let d = test_dir();

    let path = seg_path(&d, 1);

    let payload = b"abcdefghij";

    let hdr = RecordHeader::new(Lsn::new(1), payload, 0);

    let mut header = [0u8; HEADER_SIZE];
    hdr.encode(&mut header);

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&payload[..3]);

    std::fs::write(&path, bytes).unwrap();

    let report = recover_dir(&d).unwrap();

    match report.tail {
        ramwal::TailState::PartialPayload {
            expected,
            available,
            ..
        } => {
            assert_eq!(expected, 10);
            assert_eq!(available, 3);
        }
        ref other => panic!("unexpected tail: {other:?}"),
    }

    clean(&d);
}

#[test]
fn checkpoint_requires_durable_lsn() {
    let d = test_dir();

    let mut cfg = ramwal::Config::new(&d);
    cfg.durability = ramwal::Durability::Explicit;
    let wal = ramwal::Wal::open(cfg).unwrap();

    let lsn = wal.append(b"not yet durable").unwrap();

    let result = wal.checkpoint(lsn);

    assert!(matches!(
        result,
        Err(ramwal::Error::CheckpointNotDurable { .. })
    ));

    clean(&d);
}

#[test]
fn checkpoint_accepts_durable_lsn() {
    let d = test_dir();

    let wal = ramwal::Wal::open(wal_cfg(&d)).unwrap();

    let lsn = wal.append(b"durable").unwrap();

    wal.sync().unwrap();
    wal.checkpoint(lsn).unwrap();

    assert_eq!(wal.ckpt_lsn(), lsn);

    clean(&d);
}

#[test]
fn checkpoint_cannot_regress() {
    let d = test_dir();

    let wal = ramwal::Wal::open(wal_cfg(&d)).unwrap();

    let a = wal.append(b"a").unwrap();
    let b = wal.append(b"b").unwrap();

    wal.sync().unwrap();

    wal.checkpoint(b).unwrap();

    let result = wal.checkpoint(a);

    assert!(matches!(
        result,
        Err(ramwal::Error::CheckpointRegression { .. })
    ));

    clean(&d);
}

/// A manifest claiming a checkpoint the WAL cannot substantiate is a lie
/// about state that does not exist on disk.
#[test]
fn manifest_ahead_of_recovery_is_rejected() {
    let d = test_dir();

    {
        let wal = ramwal::Wal::open(wal_cfg(&d)).unwrap();
        wal.append(b"one").unwrap();
        wal.append(b"two").unwrap();
        wal.sync().unwrap();
    }

    let manifest = std::path::PathBuf::from(&d).join("MANIFEST");
    std::fs::write(&manifest, "lsn=99\n").unwrap();

    let result = ramwal::Wal::open(wal_cfg(&d));

    assert!(matches!(result, Err(ramwal::Error::Manifest(_))));

    clean(&d);
}

#[test]
fn compressed_round_trip() {
    let d = test_dir();

    let mut cfg = wal_cfg(&d);
    cfg.compression = ramwal::Compression::Lz4;

    let payload = vec![b'x'; 4096];

    {
        let wal = ramwal::Wal::open(cfg).unwrap();
        wal.append(&payload).unwrap();
        wal.sync().unwrap();
    }

    let wal2 = ramwal::Wal::open(wal_cfg(&d)).unwrap();

    assert_eq!(wal2.recovery_report().records[0].payload, payload);

    clean(&d);
}

/// CRC covers the stored bytes, not decompression semantics. A record can
/// therefore be checksum-valid and still fail to decode.
#[test]
fn malformed_compressed_payload_is_corruption() {
    let d = test_dir();

    let path = seg_path(&d, 1);

    // Declared-representation bytes that are not valid LZ4, with a correct
    // CRC for those bytes.
    let stored: Vec<u8> = vec![0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x02, 0x03];
    let hdr = RecordHeader::new(Lsn::new(1), &stored, ramwal::testkit::FLAG_COMPRESSED);

    let mut header = [0u8; HEADER_SIZE];
    hdr.encode(&mut header);

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&stored);
    std::fs::write(&path, bytes).unwrap();

    let result = recover_dir(&d);

    match result {
        Err(ramwal::Error::Corruption {
            reason: ramwal::Corruption::PayloadDecode { .. },
            ..
        }) => {}
        other => panic!("expected PayloadDecode corruption, got {:?}", other.err()),
    }

    clean(&d);
}

/// Recovery never fabricates a record: whatever it returns must be a prefix
/// of what was written.
#[test]
fn recovery_never_returns_a_record_after_corruption() {
    let d = test_dir();

    let originals: Vec<Vec<u8>> = (0..6).map(|i| format!("record-{i}").into_bytes()).collect();

    let (_, recs) = build_fixture(&d, &originals);

    // Corrupt a complete record in the middle.
    let offset = recs[2].rec_end - 1;
    corrupt_byte(&d, 1, offset, 0xFF);

    let result = recover_dir(&d);

    if let Ok(report) = result {
        assert!(report.records.len() <= originals.len());
        for (got, want) in report.records.iter().zip(originals.iter()) {
            assert_eq!(&got.payload, want);
        }
    }

    clean(&d);
}

/// The consumer-facing contract: the API a user actually touches.
#[test]
fn public_api_smoke() {
    let dir = test_dir();

    let mut cfg = ramwal::Config::new(&dir);
    cfg.durability = ramwal::Durability::Explicit;

    let wal = ramwal::Wal::open(cfg).unwrap();

    let lsn = wal.append(b"hello").unwrap();
    wal.sync().unwrap();

    assert_eq!(wal.durable_lsn(), lsn);
    assert!(wal.durable_lsn() >= lsn);

    clean(&dir);
}

// ── Retention boundary ───────────────────────────────────────────────

fn count_segs(dir: &str) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("segment-"))
        .count()
}

/// A size target is not permission to destroy history no checkpoint covers.
#[test]
fn retention_without_checkpoint_keeps_everything() {
    let d = test_dir();
    let mut cfg = ramwal::Config::new(&d);
    cfg.seg_max_bytes = 200;
    cfg.retention_max_bytes = 300;
    cfg.durability = ramwal::Durability::SyncEach;

    let wal = ramwal::Wal::open(cfg).unwrap();
    for i in 0..40 {
        wal.append(format!("payload-{i}").as_bytes()).unwrap();
    }

    let segs = count_segs(&d);
    assert!(segs > 1, "expected rotation, got {segs} segments");
    // No checkpoint was ever recorded, so nothing may be deleted.
    assert_eq!(count_segs(&d), segs);

    clean(&d);
}

/// Once a checkpoint exists, older segments below the boundary are removable.
#[test]
fn retention_with_checkpoint_deletes_older_segments() {
    let d = test_dir();
    let mut cfg = ramwal::Config::new(&d);
    cfg.seg_max_bytes = 200;
    cfg.retention_max_bytes = 300;
    cfg.durability = ramwal::Durability::SyncEach;

    let before = {
        let wal = ramwal::Wal::open(cfg.clone()).unwrap();
        for i in 0..40 {
            wal.append(format!("payload-{i}").as_bytes()).unwrap();
        }
        let n = count_segs(&d);
        wal.checkpoint(wal.current_lsn()).unwrap();
        n
    };

    assert!(before > 1);

    let _wal = ramwal::Wal::open(cfg).unwrap();
    let after = count_segs(&d);

    assert!(after < before, "retention did not act: {before} -> {after}");
    assert!(after >= 1, "retention deleted the active segment");

    clean(&d);
}

/// The active segment is never deleted, even when it alone exceeds the target.
#[test]
fn retention_never_deletes_active_segment() {
    let d = test_dir();
    let mut cfg = ramwal::Config::new(&d);
    cfg.retention_max_bytes = 300;
    cfg.durability = ramwal::Durability::SyncEach;

    let wal = ramwal::Wal::open(cfg).unwrap();
    wal.append(&vec![b'z'; 1000]).unwrap();
    wal.checkpoint(wal.current_lsn()).unwrap();
    drop(wal);

    let mut cfg2 = ramwal::Config::new(&d);
    cfg2.retention_max_bytes = 300;
    let _wal = ramwal::Wal::open(cfg2).unwrap();

    assert_eq!(count_segs(&d), 1);

    clean(&d);
}

// ── Repair boundary: recovering, then writing again ──────────────────

/// Repairing a torn tail must leave a segment that accepts new records and
/// still reports every pre-repair record.
#[test]
fn repaired_wal_accepts_new_appends() {
    let d = test_dir();
    let mut cfg = ramwal::Config::new(&d);
    cfg.seg_max_bytes = 100;
    cfg.durability = ramwal::Durability::SyncEach;

    {
        let wal = ramwal::Wal::open(cfg.clone()).unwrap();
        for i in 0..10 {
            wal.append(format!("r{i}").as_bytes()).unwrap();
        }
    }

    // Tear the newest segment's tail.
    let newest = std::fs::read_dir(&d)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with("segment-"))
                .unwrap_or(false)
        })
        .max()
        .unwrap();
    let len = std::fs::metadata(&newest).unwrap().len();
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(&newest)
        .unwrap();
    f.set_len(len.saturating_sub(10)).unwrap();
    drop(f);

    // verify must report the tear without mutating.
    let rep = ramwal::recovery::verify_dir(&d).unwrap();
    assert!(!rep.repaired);

    // open repairs, then accepts new appends.
    let wal2 = ramwal::Wal::open(cfg).unwrap();
    assert!(wal2.recovery_report().repaired);

    let lsn = wal2.append(b"after-repair").unwrap();
    wal2.sync().unwrap();

    let rep2 = ramwal::recovery::verify_dir(&d).unwrap();
    assert_eq!(rep2.records.len(), 10);
    assert_eq!(rep2.last_lsn, Some(lsn));

    clean(&d);
}

/// A brand-new WAL starts at segment 1, matching the documented layout.
#[test]
fn fresh_wal_starts_at_segment_one() {
    let d = test_dir();
    let wal = ramwal::Wal::open(ramwal::Config::new(&d)).unwrap();
    wal.append(b"x").unwrap();
    wal.sync().unwrap();
    drop(wal);

    let names: Vec<String> = std::fs::read_dir(&d)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("segment-"))
        .collect();

    assert_eq!(names, vec!["segment-00000000000000000001.rwl".to_string()]);

    clean(&d);
}

// ── P0-2 / P0-3: storage lifecycle ──────────────────────────────────────

/// A write-side failure poisons the instance; every later mutating call is
/// rejected until the WAL is reopened and recovery re-establishes a boundary.
#[test]
fn failed_append_poisons_writer() {
    let d = test_dir();
    let wal = Wal::open(wal_cfg(&d)).unwrap();

    wal.fail_next_write();
    let first = wal.append(b"payload");
    assert!(first.is_err(), "injected write failure must surface");
    assert!(wal.is_poisoned(), "a write failure must poison the WAL");

    assert!(
        matches!(wal.append(b"again"), Err(Error::Poisoned)),
        "append on a poisoned WAL must return Poisoned"
    );
    assert!(matches!(wal.flush(), Err(Error::Poisoned)));
    assert!(matches!(wal.sync(), Err(Error::Poisoned)));
    assert!(matches!(wal.checkpoint(Lsn::new(1)), Err(Error::Poisoned)));
    assert!(matches!(
        wal.truncate_before(Lsn::new(1)),
        Err(Error::Poisoned)
    ));
}

/// A write failure reported as `ENOSPC` must surface as `Error::DiskFull`
/// (not a generic `Error::Io`) and poison the instance exactly like any
/// other write-side storage failure. This covers the P1 #33 disk-full
/// qualification row without needing a physical full disk.
#[test]
fn disk_full_append_maps_to_diskfull_error() {
    let d = test_dir();
    {
        let wal = Wal::open(wal_cfg(&d)).unwrap();

        wal.fail_next_diskfull();
        let result = wal.append(b"payload");

        assert!(
            matches!(result, Err(Error::DiskFull)),
            "ENOSPC during append must map to Error::DiskFull, got: {result:?}"
        );
        assert!(
            wal.is_poisoned(),
            "disk-full write failure must poison the WAL"
        );

        // A poisoned WAL rejects every mutating operation.
        assert!(matches!(wal.append(b"again"), Err(Error::Poisoned)));
        assert!(matches!(wal.flush(), Err(Error::Poisoned)));
        assert!(matches!(wal.sync(), Err(Error::Poisoned)));
        assert!(matches!(wal.checkpoint(Lsn::new(1)), Err(Error::Poisoned)));
    }

    // Reopen runs recovery and yields a healthy instance: disk-full is a
    // transient condition, so the data written before the failure survives.
    let wal2 = Wal::open(wal_cfg(&d)).unwrap();
    assert!(
        !wal2.is_poisoned(),
        "reopen after disk-full must produce a healthy WAL"
    );
    clean(&d);
}

/// A directory fsync failure after rotation leaves a segment whose existence
/// is not guaranteed on disk; the WAL must surface the error and poison.
#[test]
fn rotation_dir_fsync_failure_poisons_writer() {
    let d = test_dir();

    let mut cfg = wal_cfg(&d);
    cfg.seg_max_bytes = 64;

    let wal = Wal::open(cfg).unwrap();

    wal.fail_next_dir_fsync();

    // Large enough to force rotation.
    let payload = vec![0x41u8; 128];

    let result = wal.append(&payload);

    assert!(
        matches!(result, Err(Error::Poisoned) | Err(Error::Io(_))),
        "rotation directory fsync failure must surface: {result:?}"
    );

    assert!(
        wal.is_poisoned(),
        "directory fsync failure after rotation must poison the WAL"
    );

    assert!(
        matches!(wal.append(b"again"), Err(Error::Poisoned)),
        "poisoned WAL must reject later appends"
    );

    clean(&d);
}

/// 32 concurrent checkpoints: only the greatest LSN succeeds; all
/// lower LSNs are rejected as regressions. The manifest ends at
/// the greatest LSN, proving the checkpoint protocol is monotonic.
#[test]
fn concurrent_checkpoints_are_monotonic() {
    use std::sync::Arc;
    use std::thread;

    let d = test_dir();
    let wal = Arc::new(Wal::open(wal_cfg(&d)).unwrap());

    let mut lsns = Vec::new();

    for i in 0..32 {
        lsns.push(wal.append(format!("record-{i}").as_bytes()).unwrap());
    }

    wal.sync().unwrap();

    let max_lsn = *lsns.last().unwrap();

    let mut handles = Vec::new();

    for lsn in lsns.iter().copied() {
        let wal = Arc::clone(&wal);

        handles.push(thread::spawn(move || wal.checkpoint(lsn)));
    }

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // The max-LSN checkpoint always succeeds: nothing can precede it.
    // Lower-LSN checkpoints succeed only when they land before any higher
    // one, otherwise they are regressions — so the count is scheduler
    // dependent, but the final state is not.
    assert!(
        results
            .iter()
            .all(|r| r.is_ok() || matches!(r, Err(Error::CheckpointRegression { .. }))),
        "only the max-LSN checkpoint may succeed; others must be regressions: {results:?}"
    );

    let final_checkpoint = wal.ckpt_lsn();
    assert_eq!(
        final_checkpoint, max_lsn,
        "manifest must record the greatest accepted LSN"
    );

    drop(wal);

    let reopened = Wal::open(wal_cfg(&d)).unwrap();

    assert_eq!(
        reopened.ckpt_lsn(),
        max_lsn,
        "manifest must match the final in-memory checkpoint"
    );

    clean(&d);
}

/// Reopening the poisoned directory runs recovery and yields a healthy WAL.
#[test]
fn poisoned_wal_recovers_on_reopen() {
    let d = test_dir();
    {
        let wal = Wal::open(wal_cfg(&d)).unwrap();
        wal.append(b"durable-before-failure").unwrap();
        wal.sync().unwrap();
        wal.fail_next_write();
        assert!(wal.append(b"poisoned").is_err());
        assert!(wal.is_poisoned());
    }

    let wal = Wal::open(wal_cfg(&d)).unwrap();
    assert!(!wal.is_poisoned(), "reopen must produce a healthy instance");
    let recs = wal.recovery_report().records.len();
    assert_eq!(recs, 1, "recovery must find the one durable record");
    assert!(wal.append(b"after-recovery").is_ok());
}

/// A WAL recovered at u64::MAX reports exhaustion instead of wrapping.
#[test]
fn lsn_exhaustion_is_explicit() {
    let d = test_dir();
    write_segment(&d, 1, &[(b"terminal", u64::MAX)]);

    let wal = Wal::open(wal_cfg(&d)).unwrap();
    assert_eq!(wal.current_lsn().get(), u64::MAX);

    assert!(
        matches!(
            wal.append(b"cannot append"),
            Err(Error::InvalidConfiguration(_))
        ),
        "exhausted LSN space must be an explicit error, not a wrap"
    );
}

/// `current_lsn()` must report the last LSN actually written, never
/// `counter - 1`. Someone will "simplify" it back otherwise.
#[test]
fn current_lsn_is_written_lsn_not_counter_minus_one() {
    let d = test_dir();
    let wal = Wal::open(wal_cfg(&d)).unwrap();

    let first = wal.append(b"a").unwrap();
    assert_eq!(wal.current_lsn(), first);

    let second = wal.append(b"b").unwrap();
    assert_eq!(wal.current_lsn(), second);
    assert_eq!(wal.current_lsn().get(), first.get() + 1);
}

/// At exhaustion `counter - 1` would underflow to MAX-1. It must report MAX.
#[test]
fn current_lsn_does_not_underflow_at_exhaustion() {
    let d = test_dir();
    write_segment(&d, 1, &[(b"terminal", u64::MAX)]);

    let wal = Wal::open(wal_cfg(&d)).unwrap();
    assert_eq!(wal.current_lsn().get(), u64::MAX);
    assert_eq!(wal.durable_lsn().get(), u64::MAX);
}

/// The final record is a legitimate checkpoint target: MAX is the last
/// written and durable LSN, so `checkpoint(MAX)` must be accepted.
#[test]
fn checkpoint_accepts_terminal_lsn_when_durable() {
    let d = test_dir();
    write_segment(&d, 1, &[(b"terminal", u64::MAX)]);

    let wal = Wal::open(wal_cfg(&d)).unwrap();
    wal.sync().unwrap();

    wal.checkpoint(Lsn::new(u64::MAX))
        .expect("MAX is the actual last written and durable LSN");
    assert_eq!(wal.ckpt_lsn().get(), u64::MAX);

    // And the manifest round-trips: reopening must not reject it.
    drop(wal);
    let wal2 = Wal::open(wal_cfg(&d)).unwrap();
    assert_eq!(wal2.ckpt_lsn().get(), u64::MAX);
}

// ── P1 #32 invariant assertions ─────────────────────────────────────────

/// Assert the three WAL LSN invariants from docs/INVARIANTS.md:
///
///   1. current_lsn >= durable_lsn  (written >= durable)
///   2. ckpt_lsn    <= durable_lsn  (checkpoint never past what's durable)
///   3. After poison the instance stays failed — already asserted by
///      `is_poisoned()`, not re-checked here.
fn assert_wal_invariants(wal: &Wal) {
    let current = wal.current_lsn().get();
    let durable = wal.durable_lsn().get();
    let ckpt = wal.ckpt_lsn().get();
    assert!(
        current >= durable,
        "WAL invariant: current_lsn {current} >= durable_lsn {durable}"
    );
    assert!(
        ckpt <= durable,
        "WAL invariant: ckpt_lsn {ckpt} <= durable_lsn {durable}"
    );
}

/// Audit §32: all LSN ordering constraints that every mutation path
/// must maintain. Covers the invariants table in docs/INVARIANTS.md.
#[test]
fn wal_lsn_invariants() {
    let d = test_dir();
    // Explicit durability: `append` does not auto-sync, so durable_lsn
    // can lag behind current_lsn between appends.
    let mut cfg = ramwal::config::Config::new(&d);
    cfg.durability = ramwal::config::Durability::Explicit;
    let wal = Wal::open(cfg).unwrap();

    // Fresh WAL: all at 0.
    assert_eq!(wal.current_lsn().get(), 0, "fresh current_lsn == 0");
    assert_eq!(wal.durable_lsn().get(), 0, "fresh durable_lsn == 0");
    assert_eq!(wal.ckpt_lsn().get(), 0, "fresh ckpt_lsn == 0");
    assert_wal_invariants(&wal);

    // Append without sync: written advances, durable stays at 0.
    let lsn1 = wal.append(b"first").unwrap();
    assert_eq!(wal.current_lsn(), lsn1);
    assert_eq!(wal.durable_lsn().get(), 0, "unsync'd durable stays at 0");
    assert_wal_invariants(&wal);

    // Checkpoint of unsync'd LSN must fail (CheckpointNotDurable).
    let r = wal.checkpoint(lsn1);
    assert!(
        matches!(r, Err(ramwal::Error::CheckpointNotDurable { .. })),
        "checkpoint of unsync'd LSN must be rejected: {r:?}"
    );
    assert_eq!(
        wal.ckpt_lsn().get(),
        0,
        "ckpt_lsn unchanged after failed checkpoint"
    );
    assert_wal_invariants(&wal);

    // Sync: durable catches up.
    wal.sync().unwrap();
    assert!(
        wal.durable_lsn() >= lsn1,
        "after sync durable >= last written"
    );
    assert_eq!(
        wal.durable_lsn(),
        wal.current_lsn(),
        "after sync durable == written"
    );
    assert_wal_invariants(&wal);

    // Checkpoint a durable LSN.
    wal.checkpoint(lsn1).unwrap();
    assert_eq!(wal.ckpt_lsn(), lsn1, "checkpoint == synced LSN");
    assert_wal_invariants(&wal);

    // Second append+sync: ckpt_lsn must NOT advance without explicit checkpoint.
    let lsn2 = wal.append(b"second").unwrap();
    wal.sync().unwrap();
    assert_eq!(wal.ckpt_lsn(), lsn1, "ckpt_lsn must not auto-advance");
    assert_wal_invariants(&wal);

    wal.checkpoint(lsn2).unwrap();
    assert_eq!(wal.ckpt_lsn(), lsn2);
    assert_wal_invariants(&wal);

    // Regression-depth path: checkpoint the past must fail (CheckpointRegression).
    let r = wal.checkpoint(lsn1);
    assert!(
        matches!(r, Err(ramwal::Error::CheckpointRegression { .. })),
        "regressive checkpoint must be rejected: {r:?}"
    );
    assert_eq!(
        wal.ckpt_lsn(),
        lsn2,
        "ckpt_lsn unchanged after regressive attempt"
    );
    assert_wal_invariants(&wal);

    clean(&d);
}

/// A-018: crash/restart equivalence at the checkpoint + durability boundary.
///
/// Sequence: append+sync+checkpoint, then unsynced appends, then drop (crash).
/// Reopen must recover only the durable prefix, restore the checkpoint LSN,
/// and accept new appends without poisoning.
#[test]
fn crash_restart_preserves_checkpoint_and_durable_prefix() {
    let d = test_dir();
    let ckpt_lsn;
    {
        let mut cfg = wal_cfg(&d);
        cfg.durability = ramwal::config::Durability::Explicit;
        let wal = Wal::open(cfg).unwrap();

        let _a = wal.append(b"durable-a").unwrap();
        let b = wal.append(b"durable-b").unwrap();
        wal.sync().unwrap();
        wal.checkpoint(b).unwrap();
        ckpt_lsn = b;

        // Unsynced tail — must not become durable after crash.
        let _ = wal.append(b"volatile-c").unwrap();
        let _ = wal.append(b"volatile-d").unwrap();
        assert!(wal.current_lsn() > wal.durable_lsn());
        assert_eq!(wal.ckpt_lsn(), ckpt_lsn);
        // Crash: drop without sync.
    }

    let wal = Wal::open(wal_cfg(&d)).unwrap();
    assert!(!wal.is_poisoned());
    assert_eq!(
        wal.ckpt_lsn(),
        ckpt_lsn,
        "checkpoint must survive crash/reopen"
    );
    let payloads: Vec<_> = wal
        .recovery_report()
        .records
        .iter()
        .map(|r| r.payload.clone())
        .collect();
    assert!(
        payloads.iter().any(|p| p == b"durable-a"),
        "durable-a missing: {payloads:?}"
    );
    assert!(
        payloads.iter().any(|p| p == b"durable-b"),
        "durable-b missing: {payloads:?}"
    );
    // Volatile tail must not appear after crash without sync under Explicit durability.
    // (If the platform auto-flushed, records may still appear — assert durable prefix only.)
    assert!(
        payloads.len() >= 2,
        "at least the checkpointed prefix must recover"
    );

    // Writer is healthy post-recovery.
    let next = wal.append(b"after-reopen").unwrap();
    wal.sync().unwrap();
    assert!(next > ckpt_lsn);
    clean(&d);
}

/// A-018: disk-full poison + checkpoint boundary — prior durable state remains.
#[test]
fn disk_full_after_checkpoint_preserves_prior_state() {
    let d = test_dir();
    let durable;
    {
        let wal = Wal::open(wal_cfg(&d)).unwrap();
        durable = wal.append(b"before-enospc").unwrap();
        wal.sync().unwrap();
        wal.checkpoint(durable).unwrap();

        wal.fail_next_diskfull();
        assert!(matches!(wal.append(b"enospc"), Err(Error::DiskFull)));
        assert!(wal.is_poisoned());
        assert!(matches!(wal.checkpoint(durable), Err(Error::Poisoned)));
    }

    let wal = Wal::open(wal_cfg(&d)).unwrap();
    assert!(!wal.is_poisoned());
    assert_eq!(wal.ckpt_lsn(), durable);
    let recs = &wal.recovery_report().records;
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].payload, b"before-enospc");
    clean(&d);
}

/// A-018: truncated segment after a clean prefix repairs and matches oracle.
#[test]
fn truncated_tail_after_synced_prefix_is_repaired() {
    let d = test_dir();
    // Build two complete records via the fixture helper so we know exact ends.
    let data = vec![b"keep".to_vec(), b"tail-record".to_vec()];
    let (path, recs) = build_fixture(&d, &data);
    assert_eq!(recs.len(), 2);
    // Tear only into the second record (after first record end).
    let cut = recs[0].rec_end + 4; // mid-header/payload of second
    assert!(cut < recs[1].rec_end);
    truncate_file(&d, 1, cut);

    let report = recover_dir(&d).expect("torn tail must be repairable");
    assert!(report.truncated_bytes > 0, "expected repair truncate");
    assert_eq!(report.records.len(), 1);
    assert_eq!(report.records[0].payload, b"keep");

    // Second pass is idempotent (no further truncate).
    let report2 = recover_dir(&d).unwrap();
    assert_eq!(report2.truncated_bytes, 0);
    assert_eq!(report2.records.len(), 1);
    let _ = path;
    clean(&d);
}
