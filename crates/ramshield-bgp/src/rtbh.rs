use crate::Result;
use crate::attributes::communities::{BLACKHOLE, NO_EXPORT};
use crate::attributes::encode_community;
use crate::attributes::{MpReachNlri, PathAttr, PathAttrFlags, PathAttrType};

/// RFC 7999 Blackhole Community
///
/// Community value: 65535:666 (0xFFFF:0x029A)
/// Must always be accompanied by NO_EXPORT (65535:65281) to prevent
/// leaking beyond the immediate transit AS.
///
/// Usage:
///   - announce /32 prefix with community BLACKHOLE + NO_EXPORT
///   - upstream routers set next-hop to null interface (192.0.2.1)
///
/// Example announce structure:
///   ORIGIN IGP
///   COMMUNITY BLACKHOLE NO_EXPORT
///   MP_REACH_NLRI: single /32 prefix
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtbhCommunity {
    /// BLACKHOLE community 65535:666
    pub blackhole: (u16, u16),
    /// NO_EXPORT community 65535:65281
    pub no_export: (u16, u16),
}

impl Default for RtbhCommunity {
    fn default() -> Self {
        Self {
            blackhole: BLACKHOLE,
            no_export: NO_EXPORT,
        }
    }
}

impl RtbhCommunity {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Encode RTBH communities as BGP Extended Communities (8 octets each)
///
/// Returns two extended communities: BLACKHOLE + NO_EXPORT
pub fn encode_rtbh_communities() -> ([u8; 8], [u8; 8]) {
    let bh = {
        let mut arr = [0u8; 8];
        arr[0] = 0x02; // Type: Transitive 2-octet AS-specific (RFC 4360)
        arr[1..3].copy_from_slice(&BLACKHOLE.0.to_be_bytes()); // ASN
        arr[3..5].copy_from_slice(&BLACKHOLE.1.to_be_bytes()); // Value
        arr
    };
    let ne = {
        let mut arr = [0u8; 8];
        arr[0] = 0x02;
        arr[1..3].copy_from_slice(&NO_EXPORT.0.to_be_bytes());
        arr[3..5].copy_from_slice(&NO_EXPORT.1.to_be_bytes());
        arr
    };
    (bh, ne)
}

/// Announce RTBH prefix with communities
///
/// Creates an UPDATE message component:
///   - ORIGIN IGP
///   - COMMUNITIES BLACKHOLE + NO_EXPORT
///   - MP_REACH_NLRI: single /32 prefix (SAFI 1, RFC 7999)
pub fn announce_rtbh(ip: &str) -> Result<Vec<u8>> {
    let ipaddr: std::net::IpAddr = ip.parse()?;
    let v4 = match ipaddr {
        std::net::IpAddr::V4(v4) => v4,
        std::net::IpAddr::V6(_) => return Err(crate::BgpError::InvalidPrefixLen(129)),
    };
    let bh = encode_community(BLACKHOLE.0, BLACKHOLE.1);
    let ne = encode_community(NO_EXPORT.0, NO_EXPORT.1);

    let origin = PathAttr {
        flags: PathAttrFlags {
            optional: false,
            transitive: true,
            partial: false,
            extended_len: false,
        },
        attr_type: PathAttrType::Origin,
        value: vec![0], // ORIGIN IGP
    }
    .encode();
    let communities_attr = PathAttr {
        flags: PathAttrFlags {
            optional: true,
            transitive: true,
            partial: false,
            extended_len: false,
        },
        attr_type: PathAttrType::Community,
        value: [bh.as_slice(), ne.as_slice()].concat(),
    }
    .encode();
    // RFC 7999: RTBH uses IPv4 unicast SAFI (1) with a /32 prefix.
    // NLRI encoding: prefix_len(1) | prefix_bytes(ceil(prefix_len/8))
    let mp_reach = MpReachNlri {
        afi: 1,                     // IPv4
        safi: 1,                    // RFC 7999 RTBH uses IPv4 unicast SAFI
        next_hop: vec![0, 0, 0, 0], // discard next-hop
        nlri: {
            let mut nlri = vec![32u8]; // /32 prefix length
            nlri.extend_from_slice(&v4.octets());
            nlri
        },
    }
    .encode();
    let mp_reach_attr = PathAttr {
        flags: PathAttrFlags {
            optional: true,
            transitive: true,
            partial: false,
            extended_len: false,
        },
        attr_type: PathAttrType::MpReachNlri,
        value: mp_reach,
    }
    .encode();

    let path_attrs = [origin, communities_attr, mp_reach_attr].concat();
    // marker(16) + length(2) + type(1) + withdrawn_len(2) + attr_len(2) + attrs
    let length = 23u16 + path_attrs.len() as u16;
    if length > 4096 {
        return Err(crate::BgpError::Session(format!(
            "RTBH UPDATE too large: {length}"
        )));
    }
    let mut buf = Vec::with_capacity(length as usize);
    buf.extend_from_slice(&[0xFFu8; 16]);
    buf.extend_from_slice(&length.to_be_bytes());
    buf.push(2); // UPDATE
    buf.extend_from_slice(&0u16.to_be_bytes()); // withdrawn routes
    buf.extend_from_slice(&(path_attrs.len() as u16).to_be_bytes());
    buf.extend_from_slice(&path_attrs);
    Ok(buf)
}

/// RTBH verification: check if UPDATE has BLACKHOLE + NO_EXPORT communities
pub fn verify_rtbh_communities(communities: &[Vec<u8>]) -> bool {
    // Must have at least 2 communities (BLACKHOLE + NO_EXPORT)
    if communities.len() < 2 {
        return false;
    }

    // Check for 65535:666 (BLACKHOLE) — ASN=0xFFFF, Value=0x029A → bytes [0xFF, 0xFF, 0x02, 0x9A]
    let has_blackhole = communities
        .iter()
        .any(|c| c.len() >= 4 && c[0] == 0xFF && c[1] == 0xFF && c[2] == 0x02 && c[3] == 0x9A);

    // Check for 65535:65281 (0xFFFF:0xFF01 = NO_EXPORT)
    let has_no_export = communities
        .iter()
        .any(|c| c.len() >= 4 && c[0] == 0xFF && c[1] == 0xFF && c[2] == 0xFF && c[3] == 0x01);

    has_blackhole && has_no_export
}
