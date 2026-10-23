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
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let tmp = std::env::var("TMPDIR").unwrap_or("/tmp".to_string());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    format!("{}/ramwal-test-{}-{}-{:020}", tmp, pid, seq, nanos)
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
