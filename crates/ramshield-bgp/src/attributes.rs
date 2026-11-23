use crate::{BgpError, Result};
use std::net::IpAddr;

/// Path attribute type codes (RFC 4271 Section 5)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PathAttrType {
    Origin = 1,
    AsPath = 2,
    NextHop = 3,
    MultiExitDisc = 4,
    LocalPref = 5,
    AtomicAggregate = 6,
    Aggregator = 7,
    Community = 8,
    OriginatorId = 9,
    ClusterList = 10,
    MpReachNlri = 14,
    MpUnreachNlri = 15,
    ExtendedCommunities = 16,
    As4Path = 17,
    As4Aggregator = 18,
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
    /// Encode path attribute to bytes
    pub fn encode(&self) -> Vec<u8> {
        let len = self.value.len();
        let mut buf = Vec::with_capacity(2 + (if self.flags.extended_len { 2 } else { 1 }) + len);
        buf.push(self.flags.encode());
        buf.push(self.attr_type as u8);
        if self.flags.extended_len || len > 255 {
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
            _ => PathAttrType::Origin, // fallback
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

/// FlowSpec Transitive Extended Community (Type 0x07, RFC 8955 Section 7)
pub mod flowspec_communities {
    pub const TRAFFIC_RATE: u8 = 0x01; // rate (4) + burst (4)
    pub const TRAFFIC_ACTION: u8 = 0x02; // drop=1, sample=2, redirect=3
    pub const REDIRECT: u8 = 0x03; // IPv4 next-hop (4) or IPv6 (16)
    pub const TRAFFIC_MARKING: u8 = 0x04; // DSCP value (4)
    pub const REDIRECT_IP: u8 = 0x05; // Extended next-hop
    pub const RT_REDIRECT: u8 = 0x08; // AS-specific or IPv4/IPv6
    pub const TRAFFIC_REMARKING: u8 = 0x09; // DSCP remarking
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
