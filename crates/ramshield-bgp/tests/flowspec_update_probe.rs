use ramshield_bgp::attributes::{ExtendedCommunity, PathAttr};
use ramshield_bgp::flowspec::FlowSpecNLRI;
use ramshield_bgp::messages::{BgpCapability, BgpOpen};
use ramshield_bgp::session::BgpSession;
use std::time::Duration;
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn flowspec_update_uses_zero_nh_len_and_length_tuple() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut sess = BgpSession::new(65001, 65002, addr.ip(), addr.port());
    sess.connect().await.unwrap();
    // RFC 4760 §5: SAFI 133 must be advertised by the peer before UPDATE.
    sess.validate_open(&BgpOpen {
        version: 4,
        my_asn: 65002,
        hold_time: 90,
        bgp_id: 0x0A00_0002,
        opt_params: vec![BgpCapability {
            code: 1,
            length: 4,
            value: [0, 1, 0, 133],
        }],
    })
    .await
    .unwrap();
    let fs = FlowSpecNLRI::rtbh_flowspec("198.51.100.1").unwrap();
    sess.send_update(fs, ExtendedCommunity::flowspec_traffic_rate(0.0))
        .await
        .unwrap();
    let (mut sock, _) = listener.accept().await.unwrap();
    let mut buf = [0u8; 512];
    let n = tokio::time::timeout(Duration::from_secs(1), sock.read(&mut buf))
        .await
        .unwrap()
        .unwrap();
    let msg = &buf[..n];

    let hdr_len = u16::from_be_bytes([msg[16], msg[17]]) as usize;
    assert_eq!(hdr_len, n, "UPDATE header length matches bytes on wire");

    let total_attr_len = u16::from_be_bytes([msg[21], msg[22]]) as usize;
    let mut offset = 23;
    let end = 23 + total_attr_len;
    let mut attrs = Vec::new();
    while offset < end {
        let (attr, used) = PathAttr::decode(&msg[offset..end]).unwrap();
        offset += used;
        attrs.push(attr);
    }
    assert_eq!(offset, end, "attributes end exactly at total length");

    let codes: Vec<u8> = attrs.iter().map(|a| a.attr_type.code()).collect();
    assert!(codes.contains(&1), "ORIGIN is well-known mandatory");
    assert!(codes.contains(&2), "AS_PATH is well-known mandatory");

    let mp = attrs.iter().find(|a| a.attr_type.code() == 14).unwrap();
    assert_eq!(
        mp.flags.encode(),
        0x80,
        "MP_REACH is optional non-transitive"
    );
    let val = &mp.value;
    assert_eq!(u16::from_be_bytes([val[0], val[1]]), 1, "AFI IPv4");
    assert_eq!(val[2], 133, "SAFI FlowSpec");
    assert_eq!(
        val[3], 0,
        "RFC 8955 section 4: FlowSpec next-hop length MUST be 0"
    );
    assert_eq!(val[4], 0, "reserved octet MUST be 0");
    assert_eq!(val[5], 6, "RFC 8955 section 4: NLRI value length");
}
