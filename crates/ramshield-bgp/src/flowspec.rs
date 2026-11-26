use crate::BgpError;
use crate::Result;

/// FlowSpec component types per RFC 8955/8956
/// Strict increasing order required — components must be sorted by type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FlowComponentType {
    DestPrefix = 1,
    SourcePrefix = 2,
    IpProtocol = 3,
    Port = 4,
    DestPort = 5,
    SourcePort = 6,
    IcmpType = 7,
    IcmpCode = 8,
    TcpFlags = 9,
    PacketLength = 10,
    Dscp = 11,
    Fragment = 12,
}

impl FlowComponentType {
    /// Prefix components (Types 1 and 2) carry `<length, prefix>` with no
    /// operator octet; every other type carries `{numeric_op, value}` pairs.
    /// RFC 8955 §4.2.2.1-4.2.2.2.
    fn is_prefix(self) -> bool {
        matches!(
            self,
            FlowComponentType::DestPrefix | FlowComponentType::SourcePrefix
        )
    }

    /// Bitmask operator (Figure 3) is used only by TCP Flags (Type 9) and
    /// Fragment (Type 12). RFC 8955 §4.2.2.9, §4.2.2.12.
    fn is_bitmask(self) -> bool {
        matches!(
            self,
            FlowComponentType::TcpFlags | FlowComponentType::Fragment
        )
    }
}

/// Operator encoding for FlowSpec components (RFC 8955 §4.2.1)
///
/// Numeric operator (Figure 2):
/// ```text
///  0   1   2   3   4   5   6   7
/// +---+---+---+---+---+---+---+---+
/// | e | a |  len  | 0 |lt |gt |eq |
/// +---+---+---+---+---+---+---+---+
/// ```
///
/// Bitmask operator (Figure 3) shares the `e`, `a`, `len` fields and
/// re-purposes the low nibble as `0 | 0 |not| m`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowOperator {
    /// Bit 1: 1 = AND mask, 0 = OR mask. MUST be 0 in the first pair of a
    /// sequence (RFC 8955 §4.2.1.1).
    pub and_mask: bool,
    /// Number of value octets: 1, 2, 4 or 8. Encoded as `log2(len)`.
    pub operand_len: u8,
    /// Equality comparison (numeric operator, bit 0).
    pub equality: bool,
    /// Less-than comparison (numeric operator, bit 2).
    pub less_than: bool,
    /// Greater-than comparison (numeric operator, bit 1).
    pub greater_than: bool,
    /// Logical negation (bitmask operator, bit 1).
    pub not: bool,
    /// Bitwise match `(data AND value) == value` (bitmask operator, bit 0).
    pub mask: bool,
}

impl Default for FlowOperator {
    fn default() -> Self {
        FlowOperator {
            and_mask: false,
            operand_len: 1,
            equality: true,
            less_than: false,
            greater_than: false,
            not: false,
            mask: false,
        }
    }
}

impl FlowOperator {
    /// Encode operator to a single octet. `end_of_list` is not stored on the
    /// struct: the encoder sets it on the last pair of a component
    /// automatically (RFC 8955 §4.2.1.1 requires exactly one `e` bit).
    pub fn encode(&self, end_of_list: bool) -> u8 {
        let mut b = 0u8;
        if end_of_list {
            b |= 0x80; // e
        }
        if self.and_mask {
            b |= 0x40; // a
        }
        // len is log2(operand_len), encoded in bits 5-4.
        let len_bits = match self.operand_len {
            2 => 1u8,
            4 => 2,
            8 => 3,
            _ => 0,
        };
        b |= len_bits << 4;
        if self.mask || self.not {
            // Bitmask operator (RFC 8955 Figure 3): bits 3-2 reserved, set to
            // zero here, bit 1 = not, bit 0 = match.
            if self.not {
                b |= 0x02;
            }
            if self.mask {
                b |= 0x01;
            }
        } else {
            if self.less_than {
                b |= 0x04;
            }
            if self.greater_than {
                b |= 0x02;
            }
            if self.equality {
                b |= 0x01;
            }
        }
        b
    }

    /// Decode operator from a single octet. `bitmask` selects the Figure 3
    /// layout for component types 9 and 12; RFC 8955 §4.2.1 says the operator
    /// is chosen by component type, not by a reserved-bit discriminator.
    /// Returns `(operator, end_of_list)`.
    pub fn decode(b: u8, bitmask: bool) -> (Self, bool) {
        let end_of_list = b & 0x80 != 0;
        let len_bits = (b >> 4) & 0x03;
        let operand_len = 1u8 << len_bits;
        // Bits 3 and 2 are reserved (MUST be ignored on decode).
        let (not, mask, less_than, greater_than, equality) = if bitmask {
            (b & 0x02 != 0, b & 0x01 != 0, false, false, false)
        } else {
            (false, false, b & 0x04 != 0, b & 0x02 != 0, b & 0x01 != 0)
        };
        (
            FlowOperator {
                and_mask: b & 0x40 != 0,
                operand_len,
                equality,
                less_than,
                greater_than,
                not,
                mask,
            },
            end_of_list,
        )
    }
}

/// One `{operator, value}` pair inside a component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowPair {
    pub operator: FlowOperator,
    pub value: Vec<u8>,
}

/// FlowSpec component encoding
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowComponent {
    pub comp_type: FlowComponentType,
    /// Prefix components hold exactly one pair whose `value` is
    /// `[prefix_len_bits, ...prefix_octets]`; the operator is ignored.
    pub pairs: Vec<FlowPair>,
}

impl FlowComponent {
    /// Encode component to bytes.
    ///
    /// Prefix types (1, 2): `<type, prefix_len, prefix_octets>` — no operator
    /// octet and no length octet (RFC 8955 §4.2.2.1).
    ///
    /// All other types: `<type, {op, value}+>` where the `e` bit is set on the
    /// final pair only (RFC 8955 §4.2.1.1).
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = vec![self.comp_type as u8];
        if self.comp_type.is_prefix() {
            let pair = match self.pairs.first() {
                Some(p) => p,
                None => return buf,
            };
            buf.extend_from_slice(&pair.value);
            return buf;
        }
        let last = self.pairs.len().saturating_sub(1);
        for (i, pair) in self.pairs.iter().enumerate() {
            buf.push(pair.operator.encode(i == last));
            buf.extend_from_slice(&pair.value);
        }
        buf
    }

    /// Decode a component from `buf`. Returns `(component, bytes_consumed)`.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize)> {
        if buf.is_empty() {
            return Err(BgpError::TooShort {
                expected: 1,
                got: 0,
            });
        }
        let comp_type = match buf[0] {
            1 => FlowComponentType::DestPrefix,
            2 => FlowComponentType::SourcePrefix,
            3 => FlowComponentType::IpProtocol,
            4 => FlowComponentType::Port,
            5 => FlowComponentType::DestPort,
            6 => FlowComponentType::SourcePort,
            7 => FlowComponentType::IcmpType,
            8 => FlowComponentType::IcmpCode,
            9 => FlowComponentType::TcpFlags,
            10 => FlowComponentType::PacketLength,
            11 => FlowComponentType::Dscp,
            12 => FlowComponentType::Fragment,
            other => return Err(BgpError::BadComponent(other)),
        };

        if comp_type.is_prefix() {
            if buf.len() < 2 {
                return Err(BgpError::TooShort {
                    expected: 2,
                    got: buf.len(),
                });
            }
            let prefix_len = buf[1] as usize;
            if prefix_len > 32 {
                return Err(BgpError::InvalidPrefixLen(prefix_len as u8));
            }
            let octets = prefix_len.div_ceil(8);
            let end = 2 + octets;
            if buf.len() < end {
                return Err(BgpError::TooShort {
                    expected: end,
                    got: buf.len(),
                });
            }
            return Ok((
                FlowComponent {
                    comp_type,
                    pairs: vec![FlowPair {
                        operator: FlowOperator::default(),
                        value: buf[1..end].to_vec(),
                    }],
                },
                end,
            ));
        }

        let mut pairs = Vec::new();
        let mut offset = 1;
        loop {
            if buf.len() < offset + 1 {
                return Err(BgpError::TooShort {
                    expected: offset + 1,
                    got: buf.len(),
                });
            }
            let (operator, end_of_list) = FlowOperator::decode(buf[offset], comp_type.is_bitmask());
            offset += 1;
            let len = operator.operand_len as usize;
            if buf.len() < offset + len {
                return Err(BgpError::TooShort {
                    expected: offset + len,
                    got: buf.len(),
                });
            }
            pairs.push(FlowPair {
                operator,
                value: buf[offset..offset + len].to_vec(),
            });
            offset += len;
            if end_of_list {
                break;
            }
        }
        // RFC 8955 §4.2.1.1: the AND bit is always unset on the first pair.
        if let Some(first) = pairs.first_mut() {
            first.operator.and_mask = false;
        }
        Ok((FlowComponent { comp_type, pairs }, offset))
    }
}

/// FlowSpec NLRI value — list of components in strict type order
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowSpecNLRI {
    pub components: Vec<FlowComponent>,
}

/// Split an IPv4 prefix into `(prefix_len_bits, octets)`.
///
/// Accepts both `a.b.c.d` (implicit /32) and `a.b.c.d/len`. Host bits are
/// masked so the encoded NLRI is canonical (RFC 8955 §4.2.2.1).
fn v4_prefix(ip: &str) -> Result<(u8, Vec<u8>)> {
    let (addr, prefix_len) = match ip.split_once('/') {
        Some((addr, len)) => (
            addr,
            len.parse::<u8>()
                .map_err(|_| BgpError::Session("invalid FlowSpec prefix length".into()))?,
        ),
        None => (ip, 32),
    };
    if prefix_len > 32 {
        return Err(BgpError::InvalidPrefixLen(prefix_len));
    }
    let addr: std::net::IpAddr = addr.parse()?;
    match addr {
        std::net::IpAddr::V4(v4) => {
            let mut octets = v4.octets().to_vec();
            let full = (prefix_len / 8) as usize;
            let rem = prefix_len % 8;
            if rem != 0 {
                octets[full] &= 0xFFu8 << (8 - rem);
            }
            octets.truncate(prefix_len.div_ceil(8) as usize);
            Ok((prefix_len, octets.to_vec()))
        }
        std::net::IpAddr::V6(_) => Err(BgpError::InvalidPrefixLen(129)),
    }
}

impl FlowSpecNLRI {
    /// Encode NLRI to bytes: `<length, components>` per RFC 8955 section 4.
    /// Components MUST be in strict increasing type order.
    pub fn encode(&self) -> Result<Vec<u8>> {
        // Verify strict type ordering
        for i in 1..self.components.len() {
            if self.components[i].comp_type as u8 <= self.components[i - 1].comp_type as u8 {
                return Err(BgpError::ComponentOrdering);
            }
        }

        let mut components = Vec::new();
        for comp in &self.components {
            // RFC 8955 §4.2.1.1: the AND bit must be 0 on the first pair of
            // a sequence. §4.2.1: operand length must match the value length.
            if !comp.comp_type.is_prefix() {
                for (i, pair) in comp.pairs.iter().enumerate() {
                    if pair.operator.and_mask && i == 0 {
                        return Err(BgpError::Session(
                            "FlowSpec first pair must have AND bit 0".into(),
                        ));
                    }
                    if pair.value.len() != pair.operator.operand_len as usize {
                        return Err(BgpError::Session(format!(
                            "FlowSpec pair value len {} != operator {}",
                            pair.value.len(),
                            pair.operator.operand_len
                        )));
                    }
                }
            }
            components.extend_from_slice(&comp.encode());
        }
        let len = components.len();
        if len < 240 {
            let mut buf = Vec::with_capacity(len + 1);
            buf.push(len as u8);
            buf.extend_from_slice(&components);
            return Ok(buf);
        }
        let extended =
            u16::try_from(len).map_err(|_| BgpError::Session("FlowSpec NLRI too large".into()))?;
        if extended > 0x0FFF {
            return Err(BgpError::Session(
                "FlowSpec NLRI exceeds 4095 octets".into(),
            ));
        }
        let mut buf = Vec::with_capacity(len + 2);
        buf.extend_from_slice(&(0xF000u16 | extended).to_be_bytes());
        buf.extend_from_slice(&components);
        Ok(buf)
    }

    /// Decode NLRI from bytes. Returns `(nlri, bytes_consumed)`.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize)> {
        if buf.is_empty() {
            return Err(BgpError::TooShort {
                expected: 1,
                got: 0,
            });
        }
        let (nlri_len, header_len): (usize, usize) = if buf[0] < 0xF0 {
            (buf[0] as usize, 1)
        } else {
            if buf.len() < 2 {
                return Err(BgpError::TooShort {
                    expected: 2,
                    got: buf.len(),
                });
            }
            ((((buf[0] & 0x0F) as usize) << 8) | buf[1] as usize, 2)
        };
        let end = header_len
            .checked_add(nlri_len)
            .ok_or_else(|| BgpError::Session("FlowSpec NLRI length overflow".into()))?;
        if buf.len() < end {
            return Err(BgpError::TooShort {
                expected: end,
                got: buf.len(),
            });
        }
        let value = &buf[header_len..end];

        let mut components = Vec::new();
        let mut offset = 0;
        let mut previous = 0u8;
        while offset < value.len() {
            let (comp, used) = FlowComponent::decode(&value[offset..])?;
            offset += used;
            let current = comp.comp_type as u8;
            if current <= previous {
                return Err(BgpError::ComponentOrdering);
            }
            previous = current;
            components.push(comp);
        }
        Ok((FlowSpecNLRI { components }, end))
    }

    /// Create a UDP reflection filter (DNS/NTP/CLDAP/SSDP source port).
    pub fn udp_reflection(dest_prefix: &str, ports: &[u16]) -> Result<Self> {
        if ports.is_empty() {
            return Err(BgpError::InvalidPrefixLen(0));
        }
        let (prefix_len, prefix_octets) = v4_prefix(dest_prefix)?;
        let mut prefix_value = vec![prefix_len];
        prefix_value.extend_from_slice(&prefix_octets);

        let eq1 = FlowOperator {
            operand_len: 1,
            ..FlowOperator::default()
        };
        // Ports are 2-octet quantities; each gets its own pair so the `e` bit
        // lands on the last one (RFC 8955 §4.2.2.6).
        let eq2 = FlowOperator {
            operand_len: 2,
            ..FlowOperator::default()
        };
        let port_pairs: Vec<FlowPair> = ports
            .iter()
            .map(|p| FlowPair {
                operator: eq2,
                value: p.to_be_bytes().to_vec(),
            })
            .collect();

        Ok(FlowSpecNLRI {
            components: vec![
                FlowComponent {
                    comp_type: FlowComponentType::DestPrefix,
                    pairs: vec![FlowPair {
                        operator: FlowOperator::default(),
                        value: prefix_value,
                    }],
                },
                FlowComponent {
                    comp_type: FlowComponentType::IpProtocol,
                    pairs: vec![FlowPair {
                        operator: eq1,
                        value: vec![17], // UDP
                    }],
                },
                FlowComponent {
                    comp_type: FlowComponentType::SourcePort,
                    pairs: port_pairs,
                },
            ],
        })
    }

    /// Create RTBH FlowSpec (dest prefix only).
    pub fn rtbh_flowspec(dest_prefix: &str) -> Result<Self> {
        let (prefix_len, prefix_octets) = v4_prefix(dest_prefix)?;
        let mut prefix_value = vec![prefix_len];
        prefix_value.extend_from_slice(&prefix_octets);
        Ok(FlowSpecNLRI {
            components: vec![FlowComponent {
                comp_type: FlowComponentType::DestPrefix,
                pairs: vec![FlowPair {
                    operator: FlowOperator::default(),
                    value: prefix_value,
                }],
            }],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_operator_bit_layout_matches_rfc8955_figure2() {
        // e=1, a=0, len=01 (2 octets), lt=0, gt=1, eq=1  ->  "!= 2 octets"
        let op = FlowOperator {
            operand_len: 2,
            equality: true,
            greater_than: true,
            ..FlowOperator::default()
        };
        assert_eq!(op.encode(true), 0x80 | 0x10 | 0x02 | 0x01);
        assert_eq!(op.encode(false), 0x10 | 0x02 | 0x01);

        let (decoded, eol) = FlowOperator::decode(op.encode(true), false);
        assert!(eol);
        assert_eq!(decoded.operand_len, 2);
        assert!(decoded.equality && decoded.greater_than && !decoded.less_than);
    }

    #[test]
    fn bitmask_operator_sets_not_and_match_bits() {
        let op = FlowOperator {
            operand_len: 1,
            not: true,
            mask: true,
            ..FlowOperator::default()
        };
        let encoded = op.encode(false);
        // RFC 8955 Figure 3: not = bit 1, m = bit 0; bits 3-2 reserved zero.
        assert_eq!(encoded, 0x02 | 0x01);
        let (decoded, _) = FlowOperator::decode(encoded, true);
        assert!(decoded.not && decoded.mask);
    }

    #[test]
    fn prefix_nlri_carries_rfc8955_length_tuple() {
        // /24 198.51.100.0 -> <len=6><type=1, prefix_len=32, 4 octets>
        let fs = FlowSpecNLRI::rtbh_flowspec("198.51.100.1").unwrap();
        let encoded = fs.encode().unwrap();
        assert_eq!(encoded[0], 6, "NLRI value length");
        assert_eq!(encoded[1], 0x01, "type 1 DestPrefix");
        assert_eq!(encoded[2], 32, "/32 prefix length");
        assert_eq!(&encoded[3..7], &[198, 51, 100, 1]);
        assert_eq!(encoded.len(), 7, "length octet + component bytes");
    }

    #[test]
    fn encode_rejects_invalid_numeric_pair_encoding() {
        let nlri = |operator: FlowOperator, value: Vec<u8>| FlowSpecNLRI {
            components: vec![FlowComponent {
                comp_type: FlowComponentType::IpProtocol,
                pairs: vec![FlowPair { operator, value }],
            }],
        };
        assert!(nlri(FlowOperator::default(), vec![17]).encode().is_ok());
        assert!(
            nlri(
                FlowOperator {
                    and_mask: true,
                    ..FlowOperator::default()
                },
                vec![17]
            )
            .encode()
            .is_err()
        );
        assert!(
            nlri(
                FlowOperator {
                    operand_len: 2,
                    ..FlowOperator::default()
                },
                vec![17]
            )
            .encode()
            .is_err()
        );
    }

    #[test]
    fn cidr_prefix_is_encoded_with_length_and_masked() {
        let nlri = FlowSpecNLRI::udp_reflection("198.51.100.0/24", &[53]).unwrap();
        assert_eq!(nlri.components[0].pairs[0].value, vec![24, 198, 51, 100]);

        // Host bits are masked to keep the prefix canonical.
        let host = FlowSpecNLRI::udp_reflection("198.51.100.7/24", &[53]).unwrap();
        assert_eq!(host.components[0].pairs[0].value, vec![24, 198, 51, 100]);

        assert!(FlowSpecNLRI::udp_reflection("198.51.100.0/33", &[53]).is_err());
    }

    #[test]
    fn port_component_emits_one_pair_per_port_with_single_end_bit() {
        let fs = FlowSpecNLRI::udp_reflection("198.51.100.0", &[53, 123]).unwrap();
        let src = &fs.components[2];
        assert_eq!(src.pairs.len(), 2);
        let encoded = src.encode();
        // type(1) + [op(1)+val(2)] + [op(1)+val(2)]
        assert_eq!(encoded.len(), 7);
        // First pair has e=0, second has e=1.
        assert_eq!(encoded[1] & 0x80, 0x00);
        assert_eq!(encoded[4] & 0x80, 0x80);
    }

    #[test]
    fn decode_clears_and_bit_on_first_pair() {
        let (nlri, _) = FlowSpecNLRI::decode(&[0x03, 0x03, 0xC1, 17]).unwrap();
        assert!(!nlri.components[0].pairs[0].operator.and_mask);
    }

    #[test]
    fn decode_rejects_out_of_order_and_bad_types() {
        // Each input is prefixed with the RFC 8955 NLRI value length.
        assert!(FlowSpecNLRI::decode(&[0x03, 0x03, 0x80 | 0x01, 17]).is_ok());
        assert!(FlowSpecNLRI::decode(&[0x01, 0x63, 0x80]).is_err());
        // Type 5 followed by Type 1 violates RFC 8955 section 4.2 strict ordering.
        assert!(FlowSpecNLRI::decode(&[0x05, 0x05, 0x81, 0x01, 0x01, 0x00]).is_err());
    }

    #[test]
    fn roundtrip_survives_encode_decode() {
        let fs = FlowSpecNLRI::udp_reflection("198.51.100.0", &[53, 123]).unwrap();
        let encoded = fs.encode().unwrap();
        let (decoded, used) = FlowSpecNLRI::decode(&encoded).unwrap();
        assert_eq!(used, encoded.len());
        assert_eq!(decoded, fs);
    }
}
