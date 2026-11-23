use ramshield_bgp::BgpConfig;
use ramshield_bgp::BgpError;
use ramshield_bgp::attributes::{communities, encode_community};
use ramshield_bgp::flowspec::{FlowComponent, FlowComponentType, FlowOperator, FlowSpecNLRI};
use ramshield_bgp::fsm::{BgpEvent, BgpFsm, BgpState};
use ramshield_bgp::messages::{BgpHeader, BgpMsgType};
use ramshield_bgp::rtbh::{announce_rtbh, encode_rtbh_communities, verify_rtbh_communities};

// Helper: default equality operator for tests
fn default_op() -> FlowOperator {
    FlowOperator {
        equality: true,
        and_mask: false,
        operand_len: 0,
        not: false,
        mask: false,
        reserved: false,
    }
}

#[test]
fn test_flow_component_encode_decode_operator() {
    // Equality operator with length 0: 0x01
    let op = FlowOperator {
        equality: true,
        and_mask: false,
        operand_len: 0,
        not: false,
        mask: false,
        reserved: false,
    };
    assert_eq!(op.encode(), 0x01);

    // Inequality AND mask with operand length 3: bit 0=0, bit1=1, bits2-4=011 => 0x02 | 0x0C = 0x0E
    let op2 = FlowOperator {
        equality: false,
        and_mask: true,
        operand_len: 3,
        not: false,
        mask: false,
        reserved: false,
    };
    assert_eq!(op2.encode(), 0x0E);

    // NOT match: bit 5 set => 0x20
    let op3 = FlowOperator {
        equality: true,
        and_mask: false,
        operand_len: 0,
        not: true,
        mask: false,
        reserved: false,
    };
    assert_eq!(op3.encode(), 0x21);

    // Mask operation: bit 6 set => 0x40
    let op4 = FlowOperator {
        equality: true,
        and_mask: false,
        operand_len: 0,
        not: false,
        mask: true,
        reserved: false,
    };
    assert_eq!(op4.encode(), 0x41);
}

#[test]
fn test_flow_component_roundtrip() {
    let comp = FlowComponent {
        comp_type: FlowComponentType::DestPrefix,
        operator: default_op(),
        value: vec![10, 0, 0, 0],
    };
    let encoded = comp.encode();
    // Type(1) + Length(1) + Operator(1) + Value(4) = 7 bytes
    assert_eq!(encoded.len(), 7);
    assert_eq!(encoded[0], 1);
    assert_eq!(encoded[1], 5); // operator(1) + value(4) = 5
    assert_eq!(encoded[2], 0x01); // equality operator
    assert_eq!(&encoded[3..7], &[10, 0, 0, 0]);
}

#[test]
fn test_flowspec_nlri_ordering() {
    // Valid: DestPrefix(1) < IpProtocol(3) < SourcePort(6)
    let valid = FlowSpecNLRI {
        components: vec![
            FlowComponent {
                comp_type: FlowComponentType::DestPrefix,
                operator: default_op(),
                value: vec![1],
            },
            FlowComponent {
                comp_type: FlowComponentType::IpProtocol,
                operator: default_op(),
                value: vec![17],
            },
            FlowComponent {
                comp_type: FlowComponentType::SourcePort,
                operator: default_op(),
                value: vec![0, 53],
            },
        ],
    };
    assert!(valid.encode().is_ok());

    // Invalid: IpProtocol(3) before DestPrefix(1)
    let invalid = FlowSpecNLRI {
        components: vec![
            FlowComponent {
                comp_type: FlowComponentType::IpProtocol,
                operator: default_op(),
                value: vec![17],
            },
            FlowComponent {
                comp_type: FlowComponentType::DestPrefix,
                operator: default_op(),
                value: vec![1],
            },
        ],
    };
    assert!(matches!(
        invalid.encode().err(),
        Some(BgpError::ComponentOrdering)
    ));

    // Invalid: same type twice
    let dup = FlowSpecNLRI {
        components: vec![
            FlowComponent {
                comp_type: FlowComponentType::DestPrefix,
                operator: default_op(),
                value: vec![1],
            },
            FlowComponent {
                comp_type: FlowComponentType::DestPrefix,
                operator: default_op(),
                value: vec![2],
            },
        ],
    };
    assert!(matches!(
        dup.encode().err(),
        Some(BgpError::ComponentOrdering)
    ));
}

#[test]
fn test_rtbh_communities_encoding() {
    let (bh, ne) = encode_rtbh_communities();
    // BLACKHOLE: type=0x02, ASN=0xFFFF(65535), Value=0x029A(666)
    assert_eq!(bh[0], 0x02);
    assert_eq!(&bh[1..3], &0xFFFFu16.to_be_bytes());
    assert_eq!(&bh[3..5], &0x029Au16.to_be_bytes());
    assert_eq!(&bh[5..8], &[0u8; 3]);

    // NO_EXPORT: type=0x02, ASN=0xFFFF, Value=0xFF01(65281)
    assert_eq!(ne[0], 0x02);
    assert_eq!(&ne[1..3], &0xFFFFu16.to_be_bytes());
    assert_eq!(&ne[3..5], &0xFF01u16.to_be_bytes());
    assert_eq!(&ne[5..8], &[0u8; 3]);
}

#[test]
fn test_rtbh_announce() {
    let update = announce_rtbh("198.51.100.1").unwrap();
    // marker + length + type + withdrawn + attr_len + attrs
    assert!(update.len() > 23);
    assert_eq!(&update[0..16], &[0xFFu8; 16]);
    assert_eq!(update[18], 2, "UPDATE type");
    let length = u16::from_be_bytes([update[16], update[17]]) as usize;
    assert_eq!(length, update.len(), "BGP header length matches buffer");
    let withdrawn = u16::from_be_bytes([update[19], update[20]]);
    assert_eq!(withdrawn, 0, "no withdrawn routes");
    let attr_len = u16::from_be_bytes([update[21], update[22]]) as usize;
    assert_eq!(attr_len, update.len() - 23, "path attr length matches");
    // /32 prefix present in MP_REACH_NLRI
    assert!(update.windows(4).any(|w| w == [198, 51, 100, 1]));
    // IPv6 rejected
    assert!(announce_rtbh("2001:db8::1").is_err());
}

#[test]
fn test_rtbh_verify_communities() {
    let bh = encode_community(communities::BLACKHOLE.0, communities::BLACKHOLE.1);
    let ne = encode_community(communities::NO_EXPORT.0, communities::NO_EXPORT.1);

    // Manually check the encoded community bytes
    // BLACKHOLE: ASN=0xFFFF, Value=0x029A → bytes [0xFF, 0xFF, 0x02, 0x9A]
    assert_eq!(bh, [0xFF, 0xFF, 0x02, 0x9A], "BLACKHOLE community encoding");
    // NO_EXPORT: ASN=0xFFFF, Value=0xFF01 → bytes [0xFF, 0xFF, 0xFF, 0x01]
    assert_eq!(ne, [0xFF, 0xFF, 0xFF, 0x01], "NO_EXPORT community encoding");

    // Both present => verify_rtbh_communities should return true
    assert!(
        verify_rtbh_communities(&[bh.to_vec(), ne.to_vec()]),
        "verify_rtbh_communities should return true when both BLACKHOLE and NO_EXPORT are present"
    );

    // Only blackhole => should be false
    assert!(
        !verify_rtbh_communities(&[bh.to_vec()]),
        "verify_rtbh_communities should return false when only BLACKHOLE is present"
    );

    // Empty => should be false
    assert!(
        !verify_rtbh_communities(&[]),
        "verify_rtbh_communities should return false when no communities present"
    );
}

#[test]
fn test_bgp_header_verify() {
    // Valid KEEPALIVE header
    let mut buf = [0u8; 19];
    buf[0..16].copy_from_slice(&[0xFFu8; 16]);
    buf[16..18].copy_from_slice(&19u16.to_be_bytes());
    buf[18] = 4; // KEEPALIVE
    let hdr = BgpHeader::verify(&buf).unwrap();
    assert_eq!(hdr.msg_type, BgpMsgType::Keepalive);
    assert_eq!(hdr.length, 19);

    // Bad marker (not all 0xFF)
    let mut bad_marker = [0u8; 19];
    bad_marker[0..16].copy_from_slice(&[0x00u8; 16]);
    bad_marker[16..18].copy_from_slice(&19u16.to_be_bytes());
    bad_marker[18] = 4;
    assert!(matches!(
        BgpHeader::verify(&bad_marker),
        Err(BgpError::BadMagic(_))
    ));

    // Too short
    let short = [0u8; 10];
    assert!(matches!(
        BgpHeader::verify(&short),
        Err(BgpError::TooShort { .. })
    ));

    // Bad length
    let mut bad_len = [0u8; 19];
    bad_len[0..16].copy_from_slice(&[0xFFu8; 16]);
    bad_len[16..18].copy_from_slice(&100u16.to_be_bytes()); // length > actual data
    bad_len[18] = 4;
    assert!(matches!(
        BgpHeader::verify(&bad_len),
        Err(BgpError::TooShort { .. })
    ));
}

#[test]
fn test_bgp_keepalive_encode_decode() {
    let data = ramshield_bgp::messages::BgpKeepalive::encode();
    assert_eq!(data.len(), 19);
    ramshield_bgp::messages::decode_keepalive(&data).unwrap();

    // Wrong type should fail
    let mut bad = data.clone();
    bad[18] = 2; // UPDATE type in KEEPALIVE message
    assert!(ramshield_bgp::messages::decode_keepalive(&bad).is_err());
}

#[test]
fn test_flowspec_udp_reflection() {
    // Pass just the IP prefix without CIDR notation
    let nflri = FlowSpecNLRI::udp_reflection("198.51.100.0", &[53, 123]).unwrap();
    assert_eq!(nflri.components.len(), 3);
    assert_eq!(nflri.components[0].comp_type, FlowComponentType::DestPrefix);
    assert_eq!(nflri.components[1].comp_type, FlowComponentType::IpProtocol);
    assert_eq!(nflri.components[2].comp_type, FlowComponentType::SourcePort);
    // Single SourcePort component with concatenated port bytes (53, 123)
    assert_eq!(nflri.components[2].value, vec![0u8, 53, 0u8, 123]);
    // encode() must succeed (no duplicate component type)
    let _ = nflri.encode().unwrap();
}

#[test]
fn test_bgp_fsm_transitions() {
    let peer_ip = std::net::IpAddr::from([198, 51, 100, 1]);
    let mut fsm = BgpFsm::new(65001, 174, peer_ip, 0x0A0B0C0D);
    assert_eq!(fsm.state(), BgpState::Idle);
    assert!(!fsm.is_established());

    // Manual start -> Connect
    fsm.process_event(BgpEvent::ManualStart).unwrap();
    assert_eq!(fsm.state(), BgpState::Connect);

    // TCP connected -> OpenSent
    fsm.process_event(BgpEvent::TcpConnectionConfirmed).unwrap();
    assert_eq!(fsm.state(), BgpState::OpenSent);

    // Open received -> OpenConfirm
    fsm.process_event(BgpEvent::OpenReceived).unwrap();
    assert_eq!(fsm.state(), BgpState::OpenConfirm);

    // Keepalive received -> Established
    fsm.process_event(BgpEvent::KeepaliveReceived).unwrap();
    assert_eq!(fsm.state(), BgpState::Established);
    assert!(fsm.is_established());

    // Manual stop -> Idle
    fsm.process_event(BgpEvent::ManualStop).unwrap();
    assert_eq!(fsm.state(), BgpState::Idle);
    assert!(!fsm.is_established());

    // Hold timer expired in Established -> Idle
    fsm.process_event(BgpEvent::ManualStart).unwrap();
    fsm.process_event(BgpEvent::TcpConnectionConfirmed).unwrap();
    fsm.process_event(BgpEvent::OpenReceived).unwrap();
    fsm.process_event(BgpEvent::KeepaliveReceived).unwrap();
    assert!(fsm.is_established());
    fsm.process_event(BgpEvent::HoldTimerExpired).unwrap();
    assert_eq!(fsm.state(), BgpState::Idle);
}

#[test]
fn test_flowspec_rtbh() {
    let fs = FlowSpecNLRI::rtbh_flowspec("198.51.100.1").unwrap();
    assert_eq!(fs.components.len(), 1);
    assert_eq!(fs.components[0].comp_type, FlowComponentType::DestPrefix);
    assert_eq!(fs.components[0].value, vec![198, 51, 100, 1]);

    // IPv6 should fail
    assert!(FlowSpecNLRI::rtbh_flowspec("2001:db8::1").is_err());
}

#[test]
fn test_hmac_verify() {
    let key = b"secret";
    let msg = b"hello world";
    // Correct tag
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key).unwrap();
    use hmac::Mac;
    mac.update(msg);
    let tag = mac.finalize().into_bytes();
    assert!(ramshield_bgp::verify_hmac(msg, key, &tag).is_ok());

    // Wrong tag → HmacFailed
    assert!(matches!(
        ramshield_bgp::verify_hmac(msg, key, &[0u8; 32]),
        Err(BgpError::HmacFailed)
    ));

    // Wrong key → HmacFailed
    assert!(matches!(
        ramshield_bgp::verify_hmac(msg, b"other", &tag),
        Err(BgpError::HmacFailed)
    ));
}

#[test]
fn test_config_debug_redacts_peer_key() {
    let cfg = BgpConfig {
        peer_shared_key: Some(vec![0xAA, 0xBB]),
        ..Default::default()
    };
    let debug = format!("{cfg:?}");
    assert!(debug.contains("REDACTED"), "debug output: {debug}");
    assert!(
        !debug.contains("170, 187"),
        "secret bytes leaked in debug output: {debug}"
    );
}
