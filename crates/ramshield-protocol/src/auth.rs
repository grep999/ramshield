//! IPC frame authentication. HMAC-SHA256 over `<ts_ms>.<payload>` with a
//! shared key. Optional per-frame envelope field:
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
#[allow(unused_imports)]
pub use replay_store::ReplayStore;

type HmacSha256 = Hmac<Sha256>;

/// Max clock skew accepted between signer and verifier.
pub const MAX_CLOCK_SKEW_MS: u64 = 30_000;

/// Compute hex signature for a payload with a key at the given timestamp.
pub fn sign(key: &[u8], ts_ms: u64, payload: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(ts_ms.to_string().as_bytes());
    mac.update(b".");
    mac.update(payload);
    hex::encode(mac.finalize().into_bytes())
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
    replay: Option<&ReplayStore>,
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
    // can't use timing to probe the store for seen digests.
    if let Some(store) = replay {
        store.check_and_record(key_id, &expected)?;
    }
    Ok(())
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_signs_and_verifies() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let payload = br#"{"type":"check_ip","ip":"1.2.3.4"}"#;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let sig = sign(b"secret-key", now, payload);
        assert!(verify(&keys, "k1", now, &sig, payload, None).is_ok());
    }

    #[test]
    fn rejects_tampered_payload() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let sig = sign(b"secret-key", now, b"honest payload");
        assert!(verify(&keys, "k1", now, &sig, b"evil payload", None).is_err());
    }

    #[test]
    fn rejects_wrong_key_and_stale_ts() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let sig = sign(b"other-key", now, b"x");
        assert!(verify(&keys, "k1", now, &sig, b"x", None).is_err());
        let good_sig = sign(b"secret-key", now, b"x");
        let old = now - MAX_CLOCK_SKEW_MS - 1000;
        assert!(verify(&keys, "k1", old, &good_sig, b"x", None).is_err());
    }

    /// RED: replay accepted without a ReplayStore (documents the bug).
    #[test]
    fn replay_without_store_accepted() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let payload = br#"{"type":"check_ip","ip":"1.2.3.4"}"#;
        let sig = sign(b"secret-key", now, payload);
        assert!(verify(&keys, "k1", now, &sig, payload, None).is_ok());
        // BUG: second call passes — no store supplied, no replay protection.
        assert!(verify(&keys, "k1", now, &sig, payload, None).is_ok());
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
        let sig = sign(b"secret-key", now, payload);
        let store = ReplayStore::new(64, Duration::from_millis(MAX_CLOCK_SKEW_MS));

        // First call should succeed
        assert!(verify(&keys, "k1", now, &sig, payload, Some(&store)).is_ok());
        // Second call with identical frame should be rejected as replay
        assert_eq!(
            verify(&keys, "k1", now, &sig, payload, Some(&store)),
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

        let sig1 = sign(b"key-a", now, payload);
        let sig2 = sign(b"key-b", now, payload);
        let store = ReplayStore::new(64, Duration::from_millis(MAX_CLOCK_SKEW_MS));
        // Different keys, same payload: both should pass (different signatures)
        assert!(verify(&keys, "k1", now, &sig1, payload, Some(&store)).is_ok());
        assert!(verify(&keys, "k2", now, &sig2, payload, Some(&store)).is_ok());
    }

    #[test]
    fn replay_different_payload_same_ts_is_allowed() {
        let keys = keys();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let sig1 = sign(b"secret-key", now, b"payload-a");
        let sig2 = sign(b"secret-key", now, b"payload-b");
        let store = ReplayStore::new(64, Duration::from_millis(MAX_CLOCK_SKEW_MS));
        // Different payloads: both should pass
        assert!(verify(&keys, "k1", now, &sig1, b"payload-a", Some(&store)).is_ok());
        assert!(verify(&keys, "k1", now, &sig2, b"payload-b", Some(&store)).is_ok());
    }
}
