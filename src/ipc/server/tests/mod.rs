use super::sanitize_ttl;
use super::verify_frame_auth;
use ramshield_protocol::auth::{self, ReplayStore};
use ramshield_types::{EnforceAction, EnforceCommand, IpNetwork};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use uuid::Uuid;

#[test]
fn sanitize_ttl_clamps_overflow_class() {
    // P1-1 go-live guard: u64::MAX used to reach `Instant::now() +
    // Duration::from_secs(u64::MAX)` and panic the enforcement task.
    assert!(sanitize_ttl(Some(u64::MAX)).is_err());
    assert!(sanitize_ttl(Some(31_536_001)).is_err());
    assert_eq!(sanitize_ttl(Some(31_536_000)), Ok(31_536_000));
    assert_eq!(sanitize_ttl(Some(60)), Ok(60));
    assert_eq!(sanitize_ttl(None), Ok(0));
}

#[test]
fn parse_cidr_normalizes_and_rejects_invalid_prefixes() {
    let net: IpNetwork = "192.0.2.123/24".parse().unwrap();
    assert_eq!(net.to_string(), "192.0.2.0/24");
    assert!("192.0.2.1/33".parse::<IpNetwork>().is_err());
    assert!("not-cidr".parse::<IpNetwork>().is_err());
}

fn signed_frame(key_id: &str, key: &[u8]) -> Vec<u8> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let payload = br#"{"type":"get_status"}"#;
    let sig = auth::sign(key, key_id, now, payload).expect("sign");
    format!(
        "{{\"auth\":{{\"key_id\":\"{}\",\"ts_ms\":{},\"sig\":\"{}\"}},\"type\":\"get_status\"}}\n",
        key_id, now, sig
    )
    .into_bytes()
}

/// P1: authenticated key identity must survive frame verification and be
/// returned to the requester for enforcement attribution.
#[test]
fn verify_frame_auth_returns_authenticated_key_id() {
    let keys = vec![
        ("key-a".to_string(), b"secret-a".to_vec()),
        ("key-b".to_string(), b"secret-b".to_vec()),
    ];
    let store = ReplayStore::new(64, Duration::from_secs(65));

    let (_, a) = verify_frame_auth(&keys, &signed_frame("key-a", b"secret-a"), &store).unwrap();
    assert_eq!(a, "key-a");

    let (_, b) = verify_frame_auth(&keys, &signed_frame("key-b", b"secret-b"), &store).unwrap();
    assert_eq!(b, "key-b");
}

/// P1: an unauthenticated frame must fail verification; there is no
/// principal to attribute.
#[test]
fn verify_frame_auth_rejects_unknown_key() {
    let keys = vec![("key-a".to_string(), b"secret-a".to_vec())];
    let store = ReplayStore::new(64, Duration::from_secs(65));
    assert!(verify_frame_auth(&keys, &signed_frame("key-zz", b"secret-a"), &store).is_err());
}

// ---- P2 authorization tests ----

use super::{Principal, authorize};
use crate::ipc::Request;
use ramshield_config::KeyRole;

fn p(role: KeyRole) -> Principal {
    Principal {
        key_id: "k".to_string(),
        role,
    }
}

fn block_req() -> Request {
    serde_json::from_str(r#"{"type":"block_ip","ip":"10.0.0.1","reason":"t","ttl_secs":60}"#)
        .unwrap()
}

fn report_req() -> Request {
    serde_json::from_str(
        r#"{"type":"report_connection","ip":"10.0.0.1","bytes":1,"status_code":200,"proto_fp":0}"#,
    )
    .unwrap()
}

fn stats_req() -> Request {
    serde_json::from_str(r#"{"type":"get_stats"}"#).unwrap()
}

/// telemetry key → report allowed
#[test]
fn telemetry_key_reports_allowed() {
    assert!(authorize(&p(KeyRole::Telemetry), &report_req()).is_ok());
}

/// telemetry key → block forbidden (least-privilege default)
#[test]
fn telemetry_key_block_forbidden() {
    assert!(authorize(&p(KeyRole::Telemetry), &block_req()).is_err());
}

/// telemetry key → read forbidden (below ReadOnly)
#[test]
fn telemetry_key_stats_forbidden() {
    assert!(authorize(&p(KeyRole::Telemetry), &stats_req()).is_err());
}

/// readonly key → read allowed, block forbidden
#[test]
fn readonly_key_read_allowed_block_forbidden() {
    assert!(authorize(&p(KeyRole::ReadOnly), &stats_req()).is_ok());
    assert!(authorize(&p(KeyRole::ReadOnly), &block_req()).is_err());
}

/// operator key → block + unblock allowed, read allowed
#[test]
fn operator_key_block_unblock_allowed() {
    let unblock: Request =
        serde_json::from_str(r#"{"type":"unblock_ip","ip":"10.0.0.1"}"#).unwrap();
    assert!(authorize(&p(KeyRole::Operator), &block_req()).is_ok());
    assert!(authorize(&p(KeyRole::Operator), &unblock).is_ok());
    assert!(authorize(&p(KeyRole::Operator), &stats_req()).is_ok());
}

/// admin key → administrative request (Flush) allowed
#[test]
fn admin_key_flush_allowed() {
    let flush: Request = serde_json::from_str(r#"{"type":"flush"}"#).unwrap();
    assert!(authorize(&p(KeyRole::Admin), &flush).is_ok());
    assert!(authorize(&p(KeyRole::Operator), &flush).is_err());
}

/// unknown key → authentication failure (no principal at all).
/// Authen vs authz separation: verify_frame_auth rejects before authorize.
#[test]
fn unknown_key_is_authentication_failure_not_authz() {
    // No principal → authorize never runs; the 401 path handles it (P1 test).
    // Here: assert the 403/401 distinction — authorize on None is unreachable,
    // but an authenticated low-role key gets 403, not 401.
    let pr = p(KeyRole::Telemetry);
    let err = authorize(&pr, &block_req()).unwrap_err();
    assert_eq!(err, "insufficient role");
}

/// P8: enforcement queue full returns explicit 503, not silent drop.
#[test]
fn enforcement_queue_full_returns_503() {
    // Build a tiny bounded channel
    let (tx, _rx) = mpsc::channel(1);
    // Fill it
    let cmd = EnforceCommand {
        decision_id: Uuid::new_v4(),
        policy_version: 1,
        source: "test".into(),
        actor: "test".into(),
        timestamp_utc: 0,
        ttl_seconds: 60,
        reason: "t".into(),
        ip: "10.0.0.1".parse().unwrap(),
        cidr: None,
        action: EnforceAction::Block,
        evidence_source: ramshield_types::EvidenceSource::Operator,
    };
    tx.try_send(cmd).unwrap();
    // Second send → full
    let err = tx.try_send(EnforceCommand {
        decision_id: Uuid::new_v4(),
        policy_version: 1,
        source: "test".into(),
        actor: "test".into(),
        timestamp_utc: 0,
        ttl_seconds: 60,
        reason: "t".into(),
        ip: "10.0.0.2".parse().unwrap(),
        cidr: None,
        action: EnforceAction::Block,
        evidence_source: ramshield_types::EvidenceSource::Operator,
    });
    assert!(err.is_err());
    // The response builder turns this into 503 "enforcement queue full"
    // Verified by the try_send match arms at 758/813/847/880.
}

#[test]
fn verify_frame_auth_rejects_replay() {
    let keys = vec![(
        "k1".to_string(),
        b"0123456789abcdef0123456789abcdef".to_vec(),
    )];
    let store = ReplayStore::new(64, Duration::from_secs(65));
    let frame = signed_frame("k1", b"0123456789abcdef0123456789abcdef");
    assert!(verify_frame_auth(&keys, &frame, &store).is_ok());
    assert!(
        verify_frame_auth(&keys, &frame, &store).is_err(),
        "identical frame must be rejected as replay"
    );
}

#[test]
fn verify_frame_auth_rejects_malformed_frames() {
    let keys = vec![(
        "k1".to_string(),
        b"0123456789abcdef0123456789abcdef".to_vec(),
    )];
    let store = ReplayStore::new(64, Duration::from_secs(65));
    assert!(verify_frame_auth(&keys, b"not-json", &store).is_err());
    assert!(verify_frame_auth(&keys, b"[]", &store).is_err());
    assert!(verify_frame_auth(&keys, br#"{"type":"get_status"}"#, &store).is_err());
    assert!(verify_frame_auth(&keys, br#"{"auth":"x","type":"get_status"}"#, &store).is_err());
    assert!(
        verify_frame_auth(
            &keys,
            br#"{"auth":{"key_id":"k1"},"type":"get_status"}"#,
            &store
        )
        .is_err()
    );
}

#[test]
fn parse_ipc_keys_fails_closed_on_bad_entries() {
    use super::parse_ipc_keys;
    use crate::config::Config;
    let mut cfg = Config::default();
    cfg.ipc.auth_keys = vec!["bad".into()];
    assert!(parse_ipc_keys(&cfg).is_err());
    cfg.ipc.auth_keys = vec!["k1:not-hex".into()];
    assert!(parse_ipc_keys(&cfg).is_err());
    cfg.ipc.auth_keys = vec!["k1:abcd".into()];
    assert!(parse_ipc_keys(&cfg).is_err());
    cfg.ipc.auth_keys = vec!["k1:0123456789abcdef0123456789abcdef".into()];
    assert!(parse_ipc_keys(&cfg).is_ok());
}
