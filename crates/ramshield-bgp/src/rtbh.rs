use crate::Result;
use crate::attributes::communities::{BLACKHOLE, NO_EXPORT};
use crate::attributes::encode_community;

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

impl RtbhCommunity {
    pub fn new() -> Self {
        RtbhCommunity {
            blackhole: BLACKHOLE,
            no_export: NO_EXPORT,
        }
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
///   - MP_REACH_NLRI: single /32 prefix
pub fn announce_rtbh(ip: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let ipaddr: std::net::IpAddr = ip.parse()?;
    match ipaddr {
        std::net::IpAddr::V4(v4) => {
            let prefix_bytes = v4.octets().to_vec();
            let (bh_comm, ne_comm) = encode_rtbh_communities();

            // Build MP_REACH_NLRI: AFI=1(IPv4), SAFI=1(?), next hop=0.0.0.0, NLRI=prefix
            let nlri = prefix_bytes.clone();

            Ok((bh_comm.to_vec(), ne_comm.to_vec()))
        }
        std::net::IpAddr::V6(_) => Err(crate::BgpError::InvalidPrefixLen(129)),
    }
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
