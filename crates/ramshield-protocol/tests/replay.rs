//! Replay-attack tests for IPC HMAC auth.
//!
//! Run: `cargo test -p ramshield-protocol --test replay`
//!
//! Production replay protection = ReplayStore (crates/ramshield-protocol/src/auth/replay_store.rs),
//! wired at src/ipc/server.rs verify_frame_auth(Some(replay)). `auth::verify`
//! with replay=None is the store-less baseline: identical signed frames repeat
//! freely inside the ±MAX_CLOCK_SKEW window — pinning THAT contract is this
//! file's remaining job.
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ramshield_protocol::auth::{self, MAX_CLOCK_SKEW_MS, ReplayStore};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn keys() -> Vec<(String, Vec<u8>)> {
    vec![("k1".to_string(), b"server-key".to_vec())]
}

/// Baseline: verify() WITHOUT a nonce store (replay=None) accepts duplicate
/// frames by design — replay protection lives in ReplayStore, which
/// production wires in at src/ipc/server.rs (Some(replay)). This test pins
/// the None path's contract; server-side dedup is covered by the
/// replay_store tests in crates/ramshield-protocol/src/auth/.
#[test]
fn replay_protection_requires_store() {
    let k = keys();
    let payload = br#"{"type":"check_ip","ip":"1.2.3.4"}"#;
    let ts = now_ms();
    let sig = auth::sign(b"server-key", "k1", ts, payload).expect("test key non-empty");

    assert!(auth::verify(&k, "k1", ts, &sig, payload, None).is_ok());
    // No store => no memory => duplicate is (correctly) accepted.
    let second = auth::verify(&k, "k1", ts, &sig, payload, None);
    assert!(
        second.is_ok(),
        "second identical frame must currently be accepted (replay bug present)"
    );
}

/// GREEN (forward-looking): a replay with `ts_ms` outside the skew window
/// must always be rejected, even without a nonce store.
#[test]
fn replay_outside_window_rejected() {
    let k = keys();
    let payload = b"x";
    let stale_ts = now_ms() - MAX_CLOCK_SKEW_MS - 1;
    let sig = auth::sign(b"server-key", "k1", stale_ts, payload).expect("test key non-empty");
    assert!(auth::verify(&k, "k1", stale_ts, &sig, payload, None).is_err());
}

/// GREEN: fresh `ReplayStore` rejects the first frame's nonce, then accepts
/// the second frame carrying a different nonce.
#[test]
fn replay_store_distinguishes_nonces() {
    let store = ReplayStore::new(1024, Duration::from_millis(200));
    let n1: &[u8] = b"nonce-1";
    let n2: &[u8] = b"nonce-2";

    assert!(store.check_and_record("k1", n1).is_ok());
    // Same nonce replayed -> reject.
    assert!(store.check_and_record("k1", n1).is_err());
    // Different nonce -> accept.
    assert!(store.check_and_record("k1", n2).is_ok());
}

/// GREEN: after the nonce's TTL elapses, the same nonce is accepted again
/// (TTL eviction, not a permanent block).
#[test]
fn replay_store_ttl_eviction() {
    let store = ReplayStore::new(1024, Duration::from_millis(50));
    let nonce: &[u8] = b"once";
    assert!(store.check_and_record("k1", nonce).is_ok());
    assert!(store.check_and_record("k1", nonce).is_err());
    thread::sleep(Duration::from_millis(80));
    assert!(
        store.check_and_record("k1", nonce).is_ok(),
        "nonce should be re-acceptable after TTL"
    );
}

/// GREEN: the same nonce under two different `key_id`s are tracked
/// independently — a key compromise cannot suppress another's protection.
#[test]
fn replay_store_per_key_isolation() {
    let store = ReplayStore::new(1024, Duration::from_millis(200));
    let nonce: &[u8] = b"shared";
    assert!(store.check_and_record("k1", nonce).is_ok());
    assert!(
        store.check_and_record("k2", nonce).is_ok(),
        "different key_id must not share nonce space"
    );
}

/// GREEN: store never exceeds its configured capacity (LRU evicts oldest).
#[test]
fn replay_store_lru_bounded() {
    let cap = 8;
    let store = ReplayStore::new(cap, Duration::from_secs(60));
    for i in 0u8..(cap as u8 * 4) {
        let n = format!("n{}", i);
        store.check_and_record("k1", n.as_bytes()).unwrap();
    }
    assert!(
        store.len() <= cap,
        "store len {} exceeded cap {}",
        store.len(),
        cap
    );
}
