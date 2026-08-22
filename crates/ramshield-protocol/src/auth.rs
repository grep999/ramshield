//! IPC frame authentication. HMAC-SHA256 over `<ts_ms>.<payload>` with a
//! shared key. Optional per-frame envelope field:
//! `"auth":{"key_id":"k1","ts_ms":...,"sig":"<hex>"}`.
//! Server enforces only when `[ipc] auth_keys` is configured; senders without
//! the field keep working against open servers (zero-config compat).

use hmac::{Hmac, Mac};
use sha2::Sha256;

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
pub fn verify(
    keys: &[(String, Vec<u8>)], // (key_id, key bytes)
    key_id: &str,
    ts_ms: u64,
    sig_hex: &str,
    payload: &[u8],
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
    // Constant-time compare via subtle (re-exported through hmac's deps).
    let got = decode_hex(sig_hex).ok_or("malformed signature")?;
    if got.len() != expected.len() {
        return Err("signature length mismatch");
    }
    let mut diff = 0u8;
    for (a, b) in got.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    if diff == 0 {
        Ok(())
    } else {
        Err("signature mismatch")
    }
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
        assert!(verify(&keys, "k1", now, &sig, payload).is_ok());
    }

    #[test]
    fn rejects_tampered_payload() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let sig = sign(b"secret-key", now, b"honest payload");
        assert!(verify(&keys, "k1", now, &sig, b"evil payload").is_err());
    }

    #[test]
    fn rejects_wrong_key_and_stale_ts() {
        let keys = vec![("k1".to_string(), b"secret-key".to_vec())];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let sig = sign(b"other-key", now, b"x");
        assert!(verify(&keys, "k1", now, &sig, b"x").is_err());
        let good_sig = sign(b"secret-key", now, b"x");
        let old = now - MAX_CLOCK_SKEW_MS - 1000;
        assert!(verify(&keys, "k1", old, &good_sig, b"x").is_err());
    }
}
