use ramshield_bgp::FlowSpecNLRI;

#[test]
fn flowspec_nlri_has_rfc8955_length_tuple() {
    let fs = FlowSpecNLRI::rtbh_flowspec("198.51.100.1").unwrap();
    let enc = fs.encode().unwrap();
    // RFC 8955 section 4: MP_REACH_NLRI NLRI is one or more <length, NLRI value>.
    // The single component value is [type=1, prefix_len=32, 4 octets] = 6 bytes.
    assert_eq!(enc.first(), Some(&6), "missing RFC 8955 length tuple");
}
