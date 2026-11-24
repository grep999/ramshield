use crate::{BgpError, Result};
use std::net::IpAddr;

/// Path attribute type codes (RFC 4271 Section 5)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PathAttrType {
    Origin,
    AsPath,
    NextHop,
    MultiExitDisc,
    LocalPref,
    AtomicAggregate,
    Aggregator,
    Community,
    OriginatorId,
    ClusterList,
    MpReachNlri,
    MpUnreachNlri,
    ExtendedCommunities,
    As4Path,
    As4Aggregator,
    Unknown(u8),
}

/// Path attribute flags (RFC 4271 Section 5)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathAttrFlags {
    /// Optional bit: 1 = optional, 0 = well-known
    pub optional: bool,
    /// Transitive bit: 1 = transitive (optional), must be 1 for well-known
    pub transitive: bool,
    /// Partial bit: 1 = partial info (optional transitive), must be 0 for well-known/non-transitive
    pub partial: bool,
    /// Extended length: 1 = 2-octet length, 0 = 1-octet length
    pub extended_len: bool,
}

impl PathAttrFlags {
    /// Encode to single byte
    pub fn encode(&self) -> u8 {
        let mut b = 0u8;
        if self.optional {
            b |= 0x80;
        }
        if self.transitive {
            b |= 0x40;
        }
        if self.partial {
            b |= 0x20;
        }
        if self.extended_len {
            b |= 0x10;
        }
        b
    }

    /// Decode from single byte
    pub fn decode(b: u8) -> Self {
        PathAttrFlags {
            optional: (b & 0x80) != 0,
            transitive: (b & 0x40) != 0,
            partial: (b & 0x20) != 0,
            extended_len: (b & 0x10) != 0,
        }
    }
}

/// Path attribute (RFC 4271 Section 5)
#[derive(Debug, Clone)]
pub struct PathAttr {
    pub flags: PathAttrFlags,
    pub attr_type: PathAttrType,
    pub value: Vec<u8>,
}

impl PathAttr {
    /// Encode path attribute to bytes.
    ///
    /// Returns an empty `Vec` if the value exceeds 65535 octets: no 2-octet
    /// length can represent it, and a wrapped length would desync the peer.
    /// Message builders must use [`encode_attrs`] to turn that sentinel into
    /// an error instead of silently omitting the attribute.
    pub fn encode(&self) -> Vec<u8> {
        let len = self.value.len();
        if len > u16::MAX as usize {
            return Vec::new();
        }
        let mut buf = Vec::with_capacity(2 + (if self.flags.extended_len { 2 } else { 1 }) + len);
        buf.push(self.flags.encode());
        buf.push(self.attr_type.code());
        if self.flags.extended_len || len > 255 {
            // RFC 4271 §5: the Extended Length flag MUST be set whenever the
            // attribute is longer than 255 octets. Encoding a 2-byte length
            // without the flag desynchronises the peer's parser.
            buf[0] |= 0x10;
            buf.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            buf.push(len as u8);
        }
        buf.extend_from_slice(&self.value);
        buf
    }

    /// Decode path attribute from bytes
    pub fn decode(buf: &[u8]) -> Result<(Self, usize)> {
        if buf.len() < 3 {
            return Err(BgpError::TooShort {
                expected: 3,
                got: buf.len(),
            });
        }
        let flags = PathAttrFlags::decode(buf[0]);
        let attr_type = PathAttrType::from(buf[1]);
        let mut offset = 2;

        let (len, len_bytes) = if flags.extended_len {
            if buf.len() < 4 {
                return Err(BgpError::TooShort {
                    expected: 4,
                    got: buf.len(),
                });
            }
            // SAFETY: buf.len() >= 4 above ensures buf[2..4] is exactly 2 bytes.
            (u16::from_be_bytes([buf[2], buf[3]]) as usize, 2)
        } else {
            (buf[2] as usize, 1)
        };
        offset += len_bytes;

        if buf.len() < offset + len {
            return Err(BgpError::TooShort {
                expected: offset + len,
                got: buf.len(),
            });
        }
        let value = buf[offset..offset + len].to_vec();
        offset += len;

        Ok((
            PathAttr {
                flags,
                attr_type,
                value,
            },
            offset,
        ))
    }
}

/// Encode a list of attributes, rejecting an oversized value instead of
/// silently emitting an empty sentinel from [`PathAttr::encode`].
pub(crate) fn encode_attrs(attrs: &[PathAttr]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for attr in attrs {
        let encoded = attr.encode();
        if encoded.is_empty() {
            return Err(BgpError::Session(
                "path attribute value exceeds 65535 octets".into(),
            ));
        }
        out.extend_from_slice(&encoded);
    }
    Ok(out)
}

impl PathAttrType {
    pub fn from(b: u8) -> Self {
        match b {
            1 => PathAttrType::Origin,
            2 => PathAttrType::AsPath,
            3 => PathAttrType::NextHop,
            4 => PathAttrType::MultiExitDisc,
            5 => PathAttrType::LocalPref,
            6 => PathAttrType::AtomicAggregate,
            7 => PathAttrType::Aggregator,
            8 => PathAttrType::Community,
            9 => PathAttrType::OriginatorId,
            10 => PathAttrType::ClusterList,
            14 => PathAttrType::MpReachNlri,
            15 => PathAttrType::MpUnreachNlri,
            16 => PathAttrType::ExtendedCommunities,
            17 => PathAttrType::As4Path,
            18 => PathAttrType::As4Aggregator,
            _ => PathAttrType::Unknown(b),
        }
    }

    /// Wire code for this attribute type (unknown codes round-trip unchanged).
    pub fn code(self) -> u8 {
        match self {
            PathAttrType::Origin => 1,
            PathAttrType::AsPath => 2,
            PathAttrType::NextHop => 3,
            PathAttrType::MultiExitDisc => 4,
            PathAttrType::LocalPref => 5,
            PathAttrType::AtomicAggregate => 6,
            PathAttrType::Aggregator => 7,
            PathAttrType::Community => 8,
            PathAttrType::OriginatorId => 9,
            PathAttrType::ClusterList => 10,
            PathAttrType::MpReachNlri => 14,
            PathAttrType::MpUnreachNlri => 15,
            PathAttrType::ExtendedCommunities => 16,
            PathAttrType::As4Path => 17,
            PathAttrType::As4Aggregator => 18,
            PathAttrType::Unknown(b) => b,
        }
    }
}

/// ORIGIN attribute (well-known mandatory)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Igp = 0,
    Egp = 1,
    Incomplete = 2,
}

/// AS_PATH segment types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsPathSegType {
    AsSet = 1,
    AsSequence = 2,
}

/// NEXT_HOP attribute encoding
pub fn encode_next_hop(ip: IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(v4) => {
            let mut buf = vec![0u8; 4];
            buf.copy_from_slice(&v4.octets());
            buf
        }
        IpAddr::V6(v6) => v6.octets().to_vec(),
    }
}

/// COMMUNITY attribute encoding (4 octets per community: high 2 bytes = ASN, low 2 = value)
pub fn encode_community(asn: u16, val: u16) -> [u8; 4] {
    let mut buf = [0u8; 4];
    buf[0..2].copy_from_slice(&asn.to_be_bytes());
    buf[2..4].copy_from_slice(&val.to_be_bytes());
    buf
}

/// Well-known communities
pub mod communities {
    pub const NO_EXPORT: (u16, u16) = (0xFFFF, 0xFF01); // 65535:65281
    pub const NO_ADVERTISE: (u16, u16) = (0xFFFF, 0xFF02); // 65535:65282
    pub const LOCAL_AS: (u16, u16) = (0xFFFF, 0xFF03); // 65535:65283
    pub const BLACKHOLE: (u16, u16) = (0xFFFF, 0x029A); // 65535:666 (RFC 7999)
}

/// Extended Community encoding (8 octets) — RFC 4360 / RFC 7153
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtendedCommunity {
    /// High octet: IANA bit (bit 7) | Transitive bit (bit 6) | 6-bit type
    pub type_field: u8,
    /// Remaining 7 octets
    pub value: [u8; 7],
}

impl ExtendedCommunity {
    pub fn encode(&self) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0] = self.type_field;
        buf[1..8].copy_from_slice(&self.value);
        buf
    }
}

/// FlowSpec Transitive Extended Community (RFC 8955 §7)
pub mod flowspec_communities {
    /// Sub-type 0x06: rate as IEEE-754 float32 bytes/second (§7.1).
    pub const TRAFFIC_RATE_BYTES: u8 = 0x06;
    /// Sub-type 0x07: terminal/sample bitmask (§7.3).
    pub const TRAFFIC_ACTION: u8 = 0x07;
    /// Sub-Type 0x08: redirect to a 2-octet AS + 4-octet value (§7.4).
    pub const REDIRECT: u8 = 0x08;
    pub const TRAFFIC_MARKING: u8 = 0x09;
    pub const REDIRECT_IP: u8 = 0x05;
    pub const RT_REDIRECT: u8 = 0x08;
    pub const TRAFFIC_REMARKING: u8 = 0x09;
}

/// MP_REACH_NLRI attribute (Type 14) — RFC 4760
#[derive(Debug, Clone)]
pub struct MpReachNlri {
    pub afi: u16,          // Address Family Identifier (1=IPv4, 2=IPv6)
    pub safi: u8,          // Subsequent Address Family Identifier (133=FlowSpec)
    pub next_hop: Vec<u8>, // Next hop address
    pub nlri: Vec<u8>,     // Network Layer Reachability Information
}

impl MpReachNlri {
    /// Encode MP_REACH_NLRI value (RFC 4760)
    ///
    /// Layout: AFI(2) | SAFI(1) | NH_Len(1) | NH(var) | Reserved(1) | NLRI(var)
    /// Reserved is always present and MUST be 0 per RFC 4760 §3.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.afi.to_be_bytes()); // AFI (2)
        buf.push(self.safi); // SAFI (1)
        buf.push(self.next_hop.len() as u8); // NextHop Length (1)
        buf.extend_from_slice(&self.next_hop); // NextHop (variable)
        buf.push(0); // Reserved (1) — always present, MUST be 0
        buf.extend_from_slice(&self.nlri); // NLRI (variable)
        buf
    }
}

/// FlowSpec SAFI values
pub mod safi {
    pub const FLOW_SPEC_V4: u8 = 133; // IPv4 FlowSpec (RFC 8955)
    pub const FLOW_SPEC_VPN_V4: u8 = 134; // VPNv4 FlowSpec
    pub const FLOW_SPEC_V6: u8 = 133; // IPv6 FlowSpec (RFC 8956) — same SAFI
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_length_flag_is_set_when_attribute_exceeds_255_octets() {
        // RFC 4271 §5: the flag MUST be set whenever a 2-byte length is used.
        let attr = PathAttr {
            flags: PathAttrFlags {
                optional: true,
                transitive: false,
                partial: false,
                extended_len: false,
            },
            attr_type: PathAttrType::Community,
            value: vec![0u8; 300],
        };
        let encoded = attr.encode();
        assert_eq!(&encoded[0..1], &[0x90], "optional | extended length");
        let len = u16::from_be_bytes([encoded[2], encoded[3]]) as usize;
        assert_eq!(len, 300);
        assert_eq!(&encoded[4..], &vec![0u8; 300][..]);
    }

    #[test]
    fn unknown_attr_type_preserves_wire_code() {
        let unknown = PathAttrType::from(0xFF);
        assert_eq!(unknown, PathAttrType::Unknown(0xFF));
        assert_eq!(unknown.code(), 0xFF);
        assert_eq!(
            PathAttrType::from(PathAttrType::Community.code()),
            PathAttrType::Community
        );
    }

    #[test]
    fn decoding_unknown_attr_type_does_not_produce_origin() {
        let (attr, used) = PathAttr::decode(&[0x80, 0xFF, 0x01, 0xAA]).unwrap();
        assert_eq!(used, 4);
        assert_eq!(attr.attr_type, PathAttrType::Unknown(0xFF));
        assert_eq!(attr.value, vec![0xAA]);
        assert_eq!(
            attr.encode()[1],
            0xFF,
            "re-encoding preserves the original code"
        );
    }

    #[test]
    fn oversized_attribute_does_not_emit_wrapped_length() {
        let attr = PathAttr {
            flags: PathAttrFlags {
                optional: true,
                transitive: false,
                partial: false,
                extended_len: false,
            },
            attr_type: PathAttrType::Community,
            value: vec![0u8; u16::MAX as usize + 1],
        };
        // ponytail: empty Vec is the documented failure sentinel; no 2-octet
        // length can represent this value without desynchronising the peer.
        assert!(attr.encode().is_empty());
        assert!(matches!(encode_attrs(&[attr]), Err(BgpError::Session(_))));
    }

    #[test]
    fn short_attribute_keeps_one_byte_length_and_clears_flag() {
        let attr = PathAttr {
            flags: PathAttrFlags {
                optional: true,
                transitive: false,
                partial: false,
                extended_len: false,
            },
            attr_type: PathAttrType::Community,
            value: vec![0u8; 6],
        };
        let encoded = attr.encode();
        assert_eq!(&encoded[0..1], &[0x80], "optional, no extended length");
        assert_eq!(encoded[2] as usize, 6);
    }
}

impl ExtendedCommunity {
    /// FlowSpec traffic-rate action (RFC 8955 §7.1, sub-type 0x06).
    ///
    /// Layout: type 0x80 (transitive, generic two-octet AS), sub-type 0x06,
    /// 2-octet informational AS id, then the rate as an IEEE-754 float32 in
    /// bytes/second. Rate 0 means "discard all traffic".
    pub fn flowspec_traffic_rate(rate_bytes_per_sec: f32) -> Self {
        let mut value = [0u8; 7];
        value[0] = flowspec_communities::TRAFFIC_RATE_BYTES;
        // value[1..3] is the informational AS number; 0 is valid.
        value[3..7].copy_from_slice(&rate_bytes_per_sec.to_bits().to_be_bytes());
        ExtendedCommunity {
            // 0x80 = transitive, generic (non-AS-specific) two-octet type.
            type_field: 0x80,
            value,
        }
    }

    /// FlowSpec traffic-action "discard" (RFC 8955 §7.3, sub-type 0x07).
    ///
    /// Bitmask: terminal (T, bit 47) + sample off. Terminal means evaluation
    /// stops at this rule, which is what a discard action requires.
    pub fn flowspec_discard() -> Self {
        let mut value = [0u8; 7];
        value[0] = flowspec_communities::TRAFFIC_ACTION;
        value[6] = 0x01; // T bit = terminal action
        ExtendedCommunity {
            type_field: 0x80,
            value,
        }
    }
}

#[cfg(test)]
mod action_tests {
    use super::*;

    #[test]
    fn traffic_rate_uses_float32_and_subtype_0x06() {
        // RFC 8955 §7.1: type 0x80, sub-type 0x06, 2-octet AS, float32 rate.
        let ec = ExtendedCommunity::flowspec_traffic_rate(1_000_000.0);
        let enc = ec.encode();
        assert_eq!(enc[0], 0x80, "transitive generic two-octet type");
        assert_eq!(enc[1], 0x06, "traffic-rate-bytes sub-type");
        assert_eq!(&enc[2..4], &[0, 0], "informational AS 0");
        let rate = f32::from_bits(u32::from_be_bytes([enc[4], enc[5], enc[6], enc[7]]));
        assert!((rate - 1_000_000.0).abs() < 1.0, "rate round-trips");
    }

    #[test]
    fn discard_sets_terminal_bit() {
        // RFC 8955 §7.3: terminal bit is the low bit of the last octet.
        let enc = ExtendedCommunity::flowspec_discard().encode();
        assert_eq!(enc[0], 0x80);
        assert_eq!(enc[1], 0x07, "traffic-action sub-type");
        assert_eq!(enc[7] & 0x01, 0x01, "terminal action set");
    }
}
