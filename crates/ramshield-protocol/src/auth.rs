//! IPC frame authentication. HMAC-SHA256 over
//! `ts_ms` ++ "." ++ `key_id` ++ `payload` (i.e. `"<ts_ms>.<key_id><payload>"`)
//! with a shared key. Optional per-frame envelope field:
//! `"auth":{"key_id":"k1","ts_ms":...,"sig":"<hex>"}`.
//! Server enforces only when `[ipc] auth_keys` is configured; senders without
//! the field keep working against open servers (zero-config compat).
//!
//! Replay protection: when a `&ReplayStore` is supplied, the verifier records
//! each accepted frame's HMAC digest in the store and rejects subsequent
//! identical frames within the store's TTL window. The signature scheme
//! itself is unchanged — the digest is checked *separately* after constant-
//! time compare, so existing senders and on-the-wire bytes are unaffected.

use hmac::{Hmac, Mac};
use sha2::Sha256;

mod replay_store;
pub use replay_store::ReplayStore;

type HmacSha256 = Hmac<Sha256>;

/// Max clock skew accepted between signer and verifier.
/// Reduced to 10s to mitigate NTP step clock issues — signer and verifier
/// must have wall clocks within 10s, enforced via SystemTime comparison.
pub const MAX_CLOCK_SKEW_MS: u64 = 10_000;

/// Compute hex signature for a payload with a key at the given timestamp.
///
/// Result reserved for future MAC swaps with length limits — current HMAC
/// accepts any key length, so `Err` is unreachable in practice.
///
/// `key_id` is bound into the MAC input (after `ts_ms`) so two key_ids
/// holding identical key bytes produce different signatures — a frame
/// captured under `k1` can't be accepted under `k2`.
pub fn sign(key: &[u8], key_id: &str, ts_ms: u64, payload: &[u8]) -> Result<String, &'static str> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| "bad key")?;
    mac.update(ts_ms.to_string().as_bytes());
    mac.update(b".");
    mac.update(key_id.as_bytes());
    mac.update(payload);
    Ok(hex::encode(mac.finalize().into_bytes()))
}

/// Verify an incoming frame's auth object against configured keys.
/// Returns Ok(()) when valid; Err(reason) otherwise.
///
/// When `replay` is `Some`, an accepted frame's HMAC digest is recorded;
/// a subsequent frame presenting the same `(key_id, sig)` within the
/// store's TTL window returns `Err("replay")`. When `replay` is `None`,
/// behaviour is identical to the pre-replay-protection code path.
pub fn verify(
    keys: &[(String, Vec<u8>)], // (key_id, key bytes)
    key_id: &str,
    ts_ms: u64,
    sig_hex: &str,
    payload: &[u8],
    replay: &ReplayStore,
) -> Result<(), &'static str> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if ts_ms.abs_diff(now) > MAX_CLOCK_SKEW_MS {
        return Err("timestamp outside allowed skew window");
    }
    let (_, key) = keys
        .iter()
        .find(|(id, _)| id == key_id)
        .ok_or("unknown key_id")?;
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| "bad key")?;
    mac.update(ts_ms.to_string().as_bytes());
    mac.update(b".");
    mac.update(key_id.as_bytes());
    mac.update(payload);
    let expected = mac.finalize().into_bytes();
    // Constant-time compare.
    let got = decode_hex(sig_hex).ok_or("malformed signature")?;
    if got.len() != expected.len() {
        return Err("signature length mismatch");
    }
    let mut diff = 0u8;
    for (a, b) in got.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    if diff != 0 {
        return Err("signature mismatch");
    }
    // Replay check runs *after* constant-time compare so an attacker
    // can't use timing to probe the store for seen digests. The store is a
    // mandatory parameter: a caller cannot opt out of replay protection by
    // passing None (compile-time enforced).
    replay.check_and_record(key_id, &expected)?;
    Ok(())
}

/// Authenticated principal after HMAC + replay checks.
/// Role assignment is IPC config, not this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedPrincipal {
    pub key_id: String,
}

/// Production verifier: replay store is required, never `None`.
pub fn verify_authenticated(
    keys: &[(String, Vec<u8>)],
    key_id: &str,
    ts_ms: u64,
    sig_hex: &str,
    payload: &[u8],
    replay: &ReplayStore,
) -> Result<AuthenticatedPrincipal, &'static str> {
    verify(keys, key_id, ts_ms, sig_hex, payload, replay)?;
    Ok(AuthenticatedPrincipal {
        key_id: key_id.to_string(),
    })
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    // Slice the BYTES, not the `&str`. A `&str` slice index must land on a
    // char boundary, so an even byte length is not enough: a multi-byte
    // character made `s[i..i+2]` panic. `as_bytes()` has no such
    // precondition, and the byte pair is validated as UTF-8 below.
    let bytes = s.as_bytes();
    (0..bytes.len())
        .step_by(2)
        .map(|i| {
            let pair = std::str::from_utf8(bytes.get(i..i + 2)?).ok()?;
            u8::from_str_radix(pair, 16).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn roundtrip_signs_and_verifies() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let payload = br#"{"type":"check_ip","ip":"1.2.3.4"}"#;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let sig = sign(b"secret-key", "k1", now, payload).expect("test key non-empty");
        assert!(verify(&keys, "k1", now, &sig, payload, &ReplayStore::default()).is_ok());
    }

    /// I-024: the MAC input layout is a wire protocol, so pin it byte for
    /// byte. The digest below was computed independently of this crate
    /// (HMAC-SHA256 over `ts_ms` ++ "." ++ key_id ++ payload), not by calling
    /// `sign` - a test that recomputes the value with the same code it is
    /// testing proves nothing.
    ///
    /// The README documented `"{ts_ms}:{payload}"`, a different separator and
    /// no key_id, so an implementer following it produced signatures this
    /// crate rejects.
    #[test]
    fn golden_vector_pins_the_mac_input_layout() {
        const KEY: &[u8] = b"secret-key";
        const KEY_ID: &str = "k1";
        const TS_MS: u64 = 1_700_000_000_000;
        const PAYLOAD: &[u8] = br#"{"type":"check_ip","ip":"1.2.3.4"}"#;
        const EXPECTED: &str = "3bba49ffd4684df1846e0a7af7568d43805e25daffebb33f22f911cc0a8cad8a";

        let sig = sign(KEY, KEY_ID, TS_MS, PAYLOAD).expect("test key non-empty");
        assert_eq!(
            sig, EXPECTED,
            "the MAC input layout changed: ts_ms ++ \".\" ++ key_id ++ payload"
        );

        // The layout is not incidental: binding key_id means two key_ids
        // holding identical key bytes must not produce the same signature.
        let other = sign(KEY, "k2", TS_MS, PAYLOAD).expect("test key non-empty");
        assert_ne!(
            sig, other,
            "key_id must be bound into the MAC input, or a frame signed under one key id is valid under another"
        );
    }

    /// P1: a malformed signature must be REJECTED, never panic.
    ///
    /// `auth.sig` is client-supplied and is decoded before any credential is
    /// checked, so this is reachable pre-authentication. The decoder used to
    /// slice a `&str` at byte offsets: an even byte length passed the length
    /// guard, but a multi-byte character made the slice land inside a char
    /// boundary — a panic. With `panic = "abort"` in the release profile that
    /// aborts the whole daemon.
    #[test]
    fn decode_hex_rejects_multibyte_input_without_panicking() {
        for s in [
            "a\u{e9}b",
            "a\u{e9}a",
            "\u{e9}\u{e9}",
            "\u{4e2d}a",
            "a\u{1f600}",
        ] {
            let r = std::panic::catch_unwind(|| decode_hex(s));
            assert!(r.is_ok(), "decode_hex panicked on {s:?}");
        }
    }

    /// The same input, through the public verification entry point the IPC
    /// server actually calls.
    #[test]
    fn verify_rejects_multibyte_signature_without_panicking() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let r = std::panic::catch_unwind(|| {
            verify(
                &keys,
                "k1",
                now,
                "a\u{e9}a",
                b"payload",
                &ReplayStore::default(),
            )
        });
        assert!(r.is_ok(), "verify panicked on a multibyte signature");
        assert!(
            r.unwrap().is_err(),
            "a malformed signature must be rejected"
        );
    }

    #[test]
    fn empty_key_still_signs_invariant() {
        // HMAC accepts any key length — empty included. Pins the invariant so
        // sign() stays equation-correct if a length-limited MAC is swapped in.
        let payload = b"x";
        assert!(sign(b"", "", 1, payload).is_ok());
    }

    #[test]
    fn rejects_tampered_payload() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let sig = sign(b"secret-key", "k1", now, b"honest payload").expect("test key non-empty");
        assert!(
            verify(
                &keys,
                "k1",
                now,
                &sig,
                b"evil payload",
                &ReplayStore::default()
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_wrong_key_and_stale_ts() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let sig = sign(b"other-key", "k1", now, b"x").expect("test key non-empty");
        let store = ReplayStore::new(10, Duration::from_secs(1));
        assert!(verify(&keys, "k1", now, &sig, b"x", &store).is_err());
        let good_sig = sign(b"secret-key", "k1", now, b"x").expect("test key non-empty");
        let old = now - MAX_CLOCK_SKEW_MS - 1000;
        let store = ReplayStore::new(10, Duration::from_secs(1));
        assert!(verify(&keys, "k1", old, &good_sig, b"x", &store).is_err());
    }

    /// GREEN: replay rejected when the same store is reused (P0-C made the
    /// store a required parameter — passing a fresh store per call is a
    /// caller bug, not an API escape hatch).
    #[test]
    fn replay_without_store_accepted() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let payload = br#"{"type":"check_ip","ip":"1.2.3.4"}"#;
        let sig = sign(b"secret-key", "k1", now, payload).expect("test key non-empty");
        let store = ReplayStore::new(10, Duration::from_secs(1));
        assert!(verify(&keys, "k1", now, &sig, payload, &store).is_ok());
        // Second identical frame must be rejected as a replay.
        assert_eq!(
            verify(&keys, "k1", now, &sig, payload, &store),
            Err("replay")
        );
    }
}

#[cfg(test)]
mod replay_tests {
    use super::*;
    use std::time::Duration;

    fn keys() -> Vec<(String, Vec<u8>)> {
        vec![("k1".to_string(), b"secret-key".to_vec())]
    }

    #[test]
    fn replay_same_frame_is_rejected() {
        let keys = keys();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let payload = br#"{"type":"check_ip","ip":"1.2.3.4"}"#;
        let sig = sign(b"secret-key", "k1", now, payload).expect("test key non-empty");
        let store = ReplayStore::new(64, Duration::from_millis(MAX_CLOCK_SKEW_MS));

        // First call should succeed
        assert!(verify(&keys, "k1", now, &sig, payload, &store).is_ok());
        // Second call with identical frame should be rejected as replay
        assert_eq!(
            verify(&keys, "k1", now, &sig, payload, &store),
            Err("replay")
        );
    }

    #[test]
    fn replay_different_key_id_is_allowed() {
        let keys = vec![
            ("k1".to_string(), b"key-a".to_vec()),
            ("k2".to_string(), b"key-b".to_vec()),
        ];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let payload = br#"{"type":"check_ip","ip":"5.6.7.8"}"#;

        let sig1 = sign(b"key-a", "k1", now, payload).expect("test key non-empty");
        let sig2 = sign(b"key-b", "k2", now, payload).expect("test key non-empty");
        let store = ReplayStore::new(64, Duration::from_millis(MAX_CLOCK_SKEW_MS));
        // Different keys, same payload: both should pass (different signatures)
        assert!(verify(&keys, "k1", now, &sig1, payload, &store).is_ok());
        assert!(verify(&keys, "k2", now, &sig2, payload, &store).is_ok());
    }

    #[test]
    fn replay_different_payload_same_ts_is_allowed() {
        let keys = keys();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let sig1 = sign(b"secret-key", "k1", now, b"payload-a").expect("test key non-empty");
        let sig2 = sign(b"secret-key", "k1", now, b"payload-b").expect("test key non-empty");
        let store = ReplayStore::new(64, Duration::from_millis(MAX_CLOCK_SKEW_MS));
        // Different payloads: both should pass
        assert!(verify(&keys, "k1", now, &sig1, b"payload-a", &store).is_ok());
        assert!(verify(&keys, "k1", now, &sig2, b"payload-b", &store).is_ok());
    }

    /// RED -> GREEN: H5 — two key_ids with identical key bytes MUST produce
    /// different signatures. Before the fix: same bytes, same signature,
    /// cross-key replay accepted. After the fix: key_id is in the MAC input,
    /// so identical bytes yield different sigs, and verify under k2 rejects
    /// a frame signed under k1.
    #[test]
    fn identical_key_bytes_different_key_id_produces_different_sig() {
        let keys = vec![
            ("k1".to_string(), b"same-secret-key".to_vec()),
            ("k2".to_string(), b"same-secret-key".to_vec()),
        ];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let payload = br#"{"type":"check_ip","ip":"1.2.3.4"}"#;

        let sig1 = sign(b"same-secret-key", "k1", now, payload).expect("sign k1");
        let sig2 = sign(b"same-secret-key", "k2", now, payload).expect("sign k2");
        // Signatures differ even though key material is identical.
        assert_ne!(sig1, sig2, "identical bytes + different key_id must differ");
        // A frame signed under k1 must NOT be accepted as k1 if key_id is wrong.
        assert!(verify(&keys, "k1", now, &sig2, payload, &ReplayStore::default()).is_err());
        assert!(verify(&keys, "k2", now, &sig1, payload, &ReplayStore::default()).is_err());
        // Each frame still validates under its own key_id.
        assert!(verify(&keys, "k1", now, &sig1, payload, &ReplayStore::default()).is_ok());
        assert!(verify(&keys, "k2", now, &sig2, payload, &ReplayStore::default()).is_ok());
    }
}
#[cfg(test)]
mod verify_authenticated_tests {
    use super::*;
    use std::time::Duration;

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    #[test]
    fn first_valid_frame_accepted() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = now_ms();
        let payload = br#"{"type":"check_ip"}"#;
        let sig = sign(b"secret-key", "k1", now, payload).unwrap();
        let store = ReplayStore::new(64, Duration::from_millis(MAX_CLOCK_SKEW_MS));
        let p = verify_authenticated(&keys, "k1", now, &sig, payload, &store).unwrap();
        assert_eq!(p.key_id, "k1");
    }

    #[test]
    fn same_frame_again_rejected() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = now_ms();
        let payload = br#"{"type":"check_ip"}"#;
        let sig = sign(b"secret-key", "k1", now, payload).unwrap();
        let store = ReplayStore::new(64, Duration::from_millis(MAX_CLOCK_SKEW_MS));
        assert!(verify_authenticated(&keys, "k1", now, &sig, payload, &store).is_ok());
        assert_eq!(
            verify_authenticated(&keys, "k1", now, &sig, payload, &store),
            Err("replay")
        );
    }

    #[test]
    fn different_payload_independently_authenticated() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = now_ms();
        let store = ReplayStore::new(64, Duration::from_millis(MAX_CLOCK_SKEW_MS));
        let s1 = sign(b"secret-key", "k1", now, b"a").unwrap();
        let s2 = sign(b"secret-key", "k1", now, b"b").unwrap();
        assert!(verify_authenticated(&keys, "k1", now, &s1, b"a", &store).is_ok());
        assert!(verify_authenticated(&keys, "k1", now, &s2, b"b", &store).is_ok());
    }

    #[test]
    fn expired_timestamp_rejected() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = now_ms();
        let old = now - MAX_CLOCK_SKEW_MS - 1000;
        let sig = sign(b"secret-key", "k1", old, b"x").unwrap();
        let store = ReplayStore::new(64, Duration::from_millis(MAX_CLOCK_SKEW_MS));
        assert!(verify_authenticated(&keys, "k1", old, &sig, b"x", &store).is_err());
    }

    #[test]
    fn invalid_signature_rejected() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = now_ms();
        let store = ReplayStore::new(64, Duration::from_millis(MAX_CLOCK_SKEW_MS));
        assert!(verify_authenticated(&keys, "k1", now, "00", b"x", &store).is_err());
    }

    #[test]
    fn unknown_key_rejected() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = now_ms();
        let sig = sign(b"secret-key", "k1", now, b"x").unwrap();
        let store = ReplayStore::new(64, Duration::from_millis(MAX_CLOCK_SKEW_MS));
        assert!(verify_authenticated(&keys, "nope", now, &sig, b"x", &store).is_err());
    }
}
