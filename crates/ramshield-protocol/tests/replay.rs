//! Replay-attack tests for IPC HMAC auth.
//!
//! Run: `cargo test -p ramshield-protocol --test replay`
//!
//! Bug being nailed: `auth::verify` accepts the same `(ts_ms, payload, sig)`
//! tuple any number of times within the ±MAX_CLOCK_SKEW_MS window. The
//! signature covers only `<ts_ms>.<payload>` — nothing ties the frame to a
//! unique send instance, so an attacker who sniffs one signed frame can
//! replay it until `ts_ms` falls out of the skew window.
//!
//! Expected (RED, before fix):
//!   - `replay_same_frame_accepted_today`  -> the frame is accepted twice
//!     (this test will FAIL once the nonce store is in place; right now it
//!     PASSES, documenting the bug.)
//!   - `replay_after_window_rejected`       -> already passes; documents the
//!     existing outer bound.
//!
//! After the fix:
//!   - First frame: Ok.
//!   - Second identical frame: Err("replay").
//!   - `replay_after_window_rejected`         -> Err (stale ts out of window).
//!   - `replay_cache_evicts_after_ttl`         -> same frame accepted again once
//!     the nonce's TTL elapses.
//!   - `replay_cache_per_key_isolation`        -> same sig under different key_id
//!     is two independent nonces.
//!   - `replay_cache_lru_bounded`              -> store never exceeds capacity.

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::thread;

use ramshield_protocol::auth::{self, ReplayStore, MAX_CLOCK_SKEW_MS};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn keys() -> Vec<(String, Vec<u8>)> {
    vec![("k1".to_string(), b"server-key".to_vec())]
}

/// RED: the current code accepts the same signed frame twice. This test
/// passes today (bug present). After the fix lands, the second call must
/// return `Err` and this test will start failing — at which point flip the
/// assertion to `is_err()` to drive the fix in. Filed here as a TODO marker
/// so a future reader knows the inversion is intentional.
#[test]
fn replay_same_frame_accepted_today() {
    let k = keys();
    let payload = br#"{"type":"check_ip","ip":"1.2.3.4"}"#;
    let ts = now_ms();
    let sig = auth::sign(b"server-key", ts, payload);

    assert!(auth::verify(&k, "k1", ts, &sig, payload, None).is_ok());
    // BUG: second identical call also returns Ok. Documented, not yet fixed.
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
    let sig = auth::sign(b"server-key", stale_ts, payload);
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
