//! Replay-attack tests for IPC HMAC auth.
//!
//! Run: `cargo test -p ramshield-protocol --test replay`
//!
//! Production replay protection = ReplayStore (crates/ramshield-protocol/src/auth/replay_store.rs),
//! wired at src/ipc/server.rs verify_frame_auth(Some(replay)). `auth::verify`
//! with a required store: identical signed frames are rejected as replays
//! inside the store TTL, and frames outside the skew window are rejected
//! regardless of store state.
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

/// P0-C: verify() requires a store. Duplicate frames against the same
/// store are rejected as replays; skew rejection works independently.
#[test]
fn replay_protection_requires_store() {
    let k = keys();
    let payload = br#"{"type":"check_ip","ip":"1.2.3.4"}"#;
    let ts = now_ms();
    let sig = auth::sign(b"server-key", "k1", ts, payload).expect("test key non-empty");

    let store = ReplayStore::new(10, Duration::from_secs(1));
    // P0-C: verify() now REQUIRES a store; first frame accepted, identical
    // second frame rejected as replay within the store TTL.
    assert!(auth::verify(&k, "k1", ts, &sig, payload, &store).is_ok());
    let second = auth::verify(&k, "k1", ts, &sig, payload, &store);
    assert_eq!(second, Err("replay"));
}

/// GREEN (forward-looking): a replay with `ts_ms` outside the skew window
/// must always be rejected, even without a nonce store.
#[test]
fn replay_outside_window_rejected() {
    let k = keys();
    let payload = b"x";
    let stale_ts = now_ms() - MAX_CLOCK_SKEW_MS - 1;
    let sig = auth::sign(b"server-key", "k1", stale_ts, payload).expect("test key non-empty");
    let store = ReplayStore::new(10, Duration::from_secs(1));
    assert!(auth::verify(&k, "k1", stale_ts, &sig, payload, &store).is_err());
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

/// GREEN: store never exceeds capacity; live markers are not evicted under pressure.
#[test]
fn replay_store_capacity_rejects_without_evicting_live() {
    let cap = 8;
    let store = ReplayStore::new(cap, Duration::from_secs(60));
    for i in 0u8..cap as u8 {
        let n = format!("n{}", i);
        assert!(
            store.check_and_record("k1", n.as_bytes()).is_ok(),
            "first {cap} inserts must succeed"
        );
    }
    // Further inserts must fail closed without dropping live markers.
    assert!(
        store.check_and_record("k1", b"overflow").is_err(),
        "saturated store must reject new nonces"
    );
    assert_eq!(store.len(), cap);
    // Original markers remain replay-protected.
    assert!(store.check_and_record("k1", b"n0").is_err());
}
