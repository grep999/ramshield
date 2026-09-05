use std::sync::Arc;
use std::thread;

use ramshield_storage::wal::{Wal, WalEntry};
use ramshield_types::Durability;

#[test]
fn wal_concurrent_appends() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_str().unwrap();

    // Fsync durability — exercises the Arc<File> snapshot path (ee1788e).
    // None would skip the concurrency-sensitive code entirely.
    let wal = Arc::new(
        Wal::open(dir, false, Durability::Fsync, 64 * 1024 * 1024, 0).unwrap(),
    );

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
        h.join().unwrap_or_else(|e| panic!("thread {i} panicked: {e:?}"));
    }

    let total = THREADS * APPENDS;
    drop(wal);

    let entries = Wal::replay(dir).unwrap();
    assert_eq!(
        entries.len(),
        total,
        "expected {total} entries after concurrent append, got {}",
        entries.len()
    );
}
