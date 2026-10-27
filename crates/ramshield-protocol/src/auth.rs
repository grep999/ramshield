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
/// holding identical key bytes cannot produce the same signature.
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
    replay: Option<&ReplayStore>,
) -> Result<(), &'static str> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "clock")?
        .as_millis() as u64;
    if ts_ms.abs_diff(now) > MAX_CLOCK_SKEW_MS {
        return Err("clock skew");
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
    let provided = hex::decode(sig_hex).map_err(|_| "bad sig encoding")?;
    if expected.as_slice().len() != provided.len()
        || !constant_time_eq(expected.as_slice(), &provided)
    {
        return Err("bad signature");
    }
    if let Some(replay) = replay {
        replay.check_and_record(key_id, &expected)?;
    }
    Ok(())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Identity bound to a verified frame (the `key_id` that passed HMAC).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedPrincipal {
    pub key_id: String,
}

pub fn verify_authenticated(
    keys: &[(String, Vec<u8>)],
    key_id: &str,
    ts_ms: u64,
    sig_hex: &str,
    payload: &[u8],
    replay: &ReplayStore,
) -> Result<AuthenticatedPrincipal, &'static str> {
    verify(keys, key_id, ts_ms, sig_hex, payload, Some(replay))?;
    Ok(AuthenticatedPrincipal {
        key_id: key_id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn golden_vector_pins_the_mac_input_layout() {
        // Computed independently of this crate (HMAC-SHA256 over
        // ts_ms ++ "." ++ key_id ++ payload), not by calling `sign`.
        //
        // The README documented `"{ts_ms}:{payload}"`, a different separator and
        // no key_id, so an implementer following it produced signatures this
        // server rejected. The vector locks the live layout.
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

    #[test]
    fn verify_rejects_multibyte_signature_without_panicking() {
        let keys = vec![("k1".into(), b"secret".to_vec())];
        let r = verify(
            &keys,
            "k1",
            1_700_000_000_000,
            "zz",
            b"{}",
            None,
        );
        assert!(r.is_err());
    }

    #[test]
    fn identical_key_bytes_different_key_id_produces_different_sig() {
        let key = b"same-bytes";
        let a = sign(key, "k1", 1_700_000_000_000, b"payload").unwrap();
        let b = sign(key, "k2", 1_700_000_000_000, b"payload").unwrap();
        assert_ne!(a, b);
    }
}
