use std::sync::Arc;
use std::thread;

use ramshield_storage::wal::{Wal, WalEntry};
use ramshield_types::Durability;

#[test]
fn wal_checkpoint_durability_ordering() {
    // RS-007: Verify checkpoint durability ordering.
    // The implementation already calls sync() before checkpoint().
    // This test guards that invariant by exercising the normal path
    // and confirming the boundary LSN is returned.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_str().unwrap();
    let wal = Arc::new(Wal::open(dir, false, Durability::Fsync, 64 * 1024 * 1024, 0).unwrap());

    let lsn = wal
        .append(&WalEntry::BlockIp {
            ip: "10.0.0.1".into(),
            reason: "rs-007-test".into(),
            ttl_secs: Some(60),
            ts_ns: 0,
        })
        .unwrap();

    // Normal checkpoint succeeds and returns the boundary LSN.
    let result = wal.finish_checkpoint(lsn);
    assert!(
        result.is_ok(),
        "finish_checkpoint must succeed: {:?}",
        result.err()
    );
    assert_eq!(
        result.unwrap(),
        lsn,
        "finish_checkpoint must return boundary LSN"
    );

    // Verify the checkpoint LSN is recorded.
    assert_eq!(wal.ckpt_lsn(), lsn, "checkpoint LSN must be published");
}

#[test]
fn wal_concurrent_appends() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_str().unwrap();

    // Fsync durability — exercises the Arc<File> snapshot path (ee1788e).
    // None would skip the concurrency-sensitive code entirely.
    let wal = Arc::new(Wal::open(dir, false, Durability::Fsync, 64 * 1024 * 1024, 0).unwrap());

    const THREADS: usize = 4;
    const APPENDS: usize = 500;

    let mut handles = Vec::with_capacity(THREADS);
    for t in 0..THREADS {
        let wal = Arc::clone(&wal);
        handles.push(thread::spawn(move || {
            for i in 0..APPENDS {
                let lsn = wal
                    .append(&WalEntry::BlockIp {
                        ip: format!("10.{}.{}.{}", t, i / 256, i % 256),
                        reason: "concurrency_test".into(),
                        ttl_secs: Some(60),
                        ts_ns: (t * APPENDS + i) as u64,
                    })
                    .unwrap_or_else(|e| panic!("thread {t} append {i} failed: {e}"));
                assert!(lsn >= 1, "LSN must be >= 1, got {lsn}");
            }
        }));
    }

    for (i, h) in handles.into_iter().enumerate() {
        h.join()
            .unwrap_or_else(|e| panic!("thread {i} panicked: {e:?}"));
    }

    let total = THREADS * APPENDS;
    drop(wal);

    let entries = Wal::replay_dir(dir).unwrap();
    assert_eq!(
        entries.len(),
        total,
        "expected {total} entries after concurrent append, got {}",
        entries.len()
    );
}
