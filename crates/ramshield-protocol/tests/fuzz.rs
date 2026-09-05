//! Property-based fuzz harnesses for the IPC protocol.
//!
//! Run: `cargo test -p ramshield-protocol --test fuzz`
//! (also runs in normal `cargo test -p ramshield-protocol`).
//!
//! These target two surfaces:
//! 1. `serde_json` deserialization of `Request` and `Response` from arbitrary
//!    bytes — must never panic. Wrong shape → Err, not unwind.
//! 2. `auth::verify` with adversarial (key_id, ts_ms, sig_hex, payload) — must
//!    never panic, must return Err for malformed input, must NEVER return
//!    Ok for non-matching key/timestamp/signature.

use proptest::prelude::*;
use ramshield_protocol::{Request, Response, auth};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// Any byte slice fed to `serde_json::from_slice::<Request>` must produce
    /// `Ok` or `Err` — never panic, never abort.
    #[test]
    fn request_parse_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = serde_json::from_slice::<Request>(&bytes);
    }

    /// Same for `Response`.
    #[test]
    fn response_parse_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = serde_json::from_slice::<Response>(&bytes);
    }

    /// The "unknown field" guarantee: a request with an extra field must be
    /// rejected (not silently accepted, which previously produced permanent
    /// blocks when TTL typos landed as `ttl_secs` vs `ttl_seconds`).
    #[test]
    fn request_rejects_unknown_fields(extra in "[a-z_]{1,32}") {
        let payload = format!(
            r#"{{"type":"check_ip","ip":"1.2.3.4","{extra}":"x"}}"#
        );
        let res = serde_json::from_str::<Request>(&payload);
        prop_assert!(res.is_err(), "request with unknown field was accepted: {payload}");
    }
}

/// Adversarial auth verification: any combination of (key_id, ts_ms, sig_hex,
/// payload) against a known key must either return Ok (only when truly valid)
/// or Err (for every other case). Must not panic on weird inputs.
fn arb_auth() -> impl Strategy<Value = (String, u64, String, Vec<u8>)> {
    (
        "[a-zA-Z0-9_]{0,16}",                           // key_id
        any::<u64>(),                                   // ts_ms (any u64 incl. overflow risks)
        "[0-9a-fA-Fx]{0,128}", // sig_hex (may be malformed, odd-length, non-hex)
        proptest::collection::vec(any::<u8>(), 0..512), // payload
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    #[test]
    fn auth_verify_never_panics(input in arb_auth()) {
        let (kid, ts, sig, payload) = input;
        let keys = vec![("k1".to_string(), b"the-real-key".to_vec())];
        let _ = auth::verify(&keys, &kid, ts, &sig, &payload, None);
    }

    /// A signature produced from a *different* key must always be rejected,
    /// regardless of payload content.
    #[test]
    fn auth_rejects_wrong_key_signature(
        payload in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let sig = auth::sign(b"attacker-key", now, &payload);
        let keys = vec![("k1".to_string(), b"server-key".to_vec())];
        let res = auth::verify(&keys, "k1", now, &sig, &payload, None);
        prop_assert!(res.is_err(), "forged signature accepted");
    }
}
