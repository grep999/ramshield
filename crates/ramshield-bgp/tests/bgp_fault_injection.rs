//! BGP fault injection tests — mirrors mesh_fault_injection.rs pattern.
//!
//! Exercises: session crash/restart, HMAC failure handling, FSM flap detection,
//! and convergence after notification.

use ramshield_bgp::fsm::{BgpEvent, BgpFsm, BgpState};
use ramshield_bgp::messages::BgpHeader;
use ramshield_bgp::{BgpConfig, BgpError, verify_hmac};
use std::net::IpAddr;
use std::time::Duration;

/// Verify that a sequence of BGP FSM events produces the expected state transitions.
/// Simulates a session that establishes, receives a notification (peer error),
/// and restarts.
#[test]
fn fsm_establish_notify_restart_converges() {
    let peer_ip = IpAddr::from([192, 0, 2, 1]);
    let mut fsm = BgpFsm::new(65001, 174, peer_ip, 0x0A00_0001);

    // Idle -> Connect -> OpenSent -> OpenConfirm -> Established
    fsm.process_event(BgpEvent::ManualStart).unwrap();
    assert_eq!(fsm.state(), BgpState::Connect);
    fsm.process_event(BgpEvent::TcpConnectionConfirmed).unwrap();
    assert_eq!(fsm.state(), BgpState::OpenSent);
    fsm.process_event(BgpEvent::OpenReceived).unwrap();
    assert_eq!(fsm.state(), BgpState::OpenConfirm);
    fsm.process_event(BgpEvent::KeepaliveReceived).unwrap();
    assert_eq!(fsm.state(), BgpState::Established);
    assert!(fsm.is_established());

    // Peer sends NOTIFICATION → Idle
    fsm.process_event(BgpEvent::NotificationReceived).unwrap();
    assert_eq!(fsm.state(), BgpState::Idle);
    assert!(!fsm.is_established());

    // Restart: Idle -> Connect -> OpenSent -> OpenConfirm -> Established
    fsm.process_event(BgpEvent::ManualStart).unwrap();
    fsm.process_event(BgpEvent::TcpConnectionConfirmed).unwrap();
    fsm.process_event(BgpEvent::OpenReceived).unwrap();
    fsm.process_event(BgpEvent::KeepaliveReceived).unwrap();
    assert_eq!(fsm.state(), BgpState::Established);
    assert!(fsm.is_established(), "session must reconverge after notify");
}

/// Verify that a hold timer expiry in Established returns to Idle.
#[test]
fn hold_timer_expiry_returns_to_idle() {
    let peer_ip = IpAddr::from([192, 0, 2, 2]);
    let mut fsm = BgpFsm::new(65001, 174, peer_ip, 0x0A00_0002);

    fsm.process_event(BgpEvent::ManualStart).unwrap();
    fsm.process_event(BgpEvent::TcpConnectionConfirmed).unwrap();
    fsm.process_event(BgpEvent::OpenReceived).unwrap();
    fsm.process_event(BgpEvent::KeepaliveReceived).unwrap();
    assert!(fsm.is_established());

    fsm.process_event(BgpEvent::HoldTimerExpired).unwrap();
    assert_eq!(fsm.state(), BgpState::Idle);
    assert!(!fsm.is_established());
}

/// Verify HMAC verification rejects tampered messages.
#[test]
fn hmac_rejects_tampered_message() {
    let key = b"peer-shared-secret";
    let msg = b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x13\x04";
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key).unwrap();
    use hmac::Mac;
    mac.update(msg);
    let good_tag = mac.finalize().into_bytes();

    // Good tag passes
    assert!(verify_hmac(msg, key, &good_tag).is_ok());

    // Tampered tag fails
    let mut bad_tag = good_tag;
    bad_tag[0] ^= 0xFF;
    assert!(matches!(
        verify_hmac(msg, key, &bad_tag),
        Err(BgpError::HmacFailed)
    ));

    // Wrong key fails
    assert!(matches!(
        verify_hmac(msg, b"wrong-key", &good_tag),
        Err(BgpError::HmacFailed)
    ));

    // Empty key fails gracefully
    assert!(matches!(
        verify_hmac(msg, b"", &good_tag),
        Err(BgpError::HmacFailed)
    ));
}

/// Verify that BgpHeader rejects malformed headers.
#[test]
fn header_rejects_malformed() {
    // Bad marker
    let mut buf = [0u8; 19];
    buf[0..16].copy_from_slice(&[0x00u8; 16]);
    buf[16..18].copy_from_slice(&19u16.to_be_bytes());
    buf[18] = 4;
    assert!(matches!(
        BgpHeader::verify(&buf),
        Err(BgpError::BadMagic(_))
    ));

    // Length too short
    let mut buf2 = [0u8; 19];
    buf2[0..16].copy_from_slice(&[0xFFu8; 16]);
    buf2[16..18].copy_from_slice(&18u16.to_be_bytes()); // < 19
    buf2[18] = 4;
    assert!(matches!(
        BgpHeader::verify(&buf2),
        Err(BgpError::TooShort { .. })
    ));

    // Unknown message type
    let mut buf3 = [0u8; 19];
    buf3[0..16].copy_from_slice(&[0xFFu8; 16]);
    buf3[16..18].copy_from_slice(&19u16.to_be_bytes());
    buf3[18] = 99;
    assert!(matches!(
        BgpHeader::verify(&buf3),
        Err(BgpError::BadType(99))
    ));
}

/// Verify that the Schmitt trigger engages after threshold flaps and disengages
/// after a quiet window.
#[test]
fn schmitt_trigger_engage_disengage_cycle() {
    let mut trigger = ramshield_bgp::controller::SchmittTrigger::new(3, 5);

    // Below threshold — no engage
    trigger.record_flap();
    trigger.record_flap();
    assert!(!trigger.should_engage(), "2 flaps < 3 threshold");

    // Third flap — engage
    trigger.record_flap();
    assert!(trigger.should_engage(), "3 flaps >= 3 threshold");

    // Simulate engagement
    trigger.engaged_at = Some(std::time::Instant::now());
    trigger.last_flap_at = Some(std::time::Instant::now());

    // Not enough time elapsed — no disengage
    assert!(
        !trigger.should_disengage(),
        "fresh engagement must not disengage"
    );

    // Simulate time passing: engagement + quiet window
    trigger.engaged_at = Some(std::time::Instant::now() - Duration::from_secs(10));
    trigger.last_flap_at = Some(std::time::Instant::now() - Duration::from_secs(10));
    assert!(
        trigger.should_disengage(),
        "past disengage window must disengage"
    );

    // Reset
    trigger.reset();
    assert!(!trigger.should_engage(), "reset must clear flap count");
    assert!(!trigger.should_disengage(), "reset must clear engagement");
}

/// Verify BgpConfig debug redacts the shared key.
#[test]
fn config_debug_redacts_key() {
    let cfg = BgpConfig {
        peer_shared_key: Some(vec![0xAA, 0xBB, 0xCC]),
        ..Default::default()
    };
    let debug = format!("{cfg:?}");
    assert!(
        debug.contains("[REDACTED]"),
        "key must be redacted: {debug}"
    );
    assert!(
        !debug.contains("170, 187, 204"),
        "key bytes leaked in debug output: {debug}"
    );
}

/// Verify BgpConfig from_toml parses optional fields correctly.
#[test]
fn config_from_toml_parses_optional_fields() {
    let toml_str = r#"
        local_asn = 65001
        peer_asn = 174
        peer_ip = "192.0.2.1"
        router_id = 3221225985
        peer_port = 179
        hold_time = 90
        connect_retry_time = 120
    "#;
    let cfg = BgpConfig::from_toml(toml_str).unwrap();
    assert_eq!(cfg.local_asn, 65001);
    assert_eq!(cfg.peer_asn, 174);
    assert_eq!(cfg.peer_port, 179);
    assert_eq!(cfg.hold_time, 90);
    assert_eq!(cfg.connect_retry_time, 120);
    assert!(cfg.peer_shared_key.is_none());
}

/// Verify BgpConfig from_toml parses HMAC key.
#[test]
fn config_from_toml_parses_hmac_key() {
    let toml_str = r#"
        local_asn = 65001
        peer_asn = 174
        peer_ip = "192.0.2.1"
        router_id = 3221225985
        peer_shared_key = "aabbccdd"
    "#;
    let cfg = BgpConfig::from_toml(toml_str).unwrap();
    assert_eq!(cfg.peer_shared_key, Some(vec![0xAA, 0xBB, 0xCC, 0xDD]));
}
