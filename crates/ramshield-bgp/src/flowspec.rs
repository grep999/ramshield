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

/// Operator encoding for FlowSpec components (RFC 8955 Section 4.2.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowOperator {
    /// Bit 0: 1 = equality, 0 = inequality
    pub equality: bool,
    /// Bit 1: 1 = AND mask, 0 = OR mask
    pub and_mask: bool,
    /// Bits 2-4: operand length (0-7)
    pub operand_len: u8,
    /// Bit 5: NOT match
    pub not: bool,
    /// Bit 6: mask operation
    pub mask: bool,
    /// Bit 7: reserved (must be 0)
    pub reserved: bool,
}

impl FlowOperator {
    /// Encode operator to single byte
    pub fn encode(&self) -> u8 {
        let mut b = 0u8;
        if self.equality {
            b |= 0x01;
        }
        if self.and_mask {
            b |= 0x02;
        }
        b |= self.operand_len << 2; // bits 2-4
        if self.not {
            b |= 0x20;
        } // bit 5
        if self.mask {
            b |= 0x40;
        } // bit 6
        // bit 7 reserved, must be 0
        b
    }

    /// Decode operator from single byte
    pub fn decode(b: u8) -> Self {
        FlowOperator {
            equality: (b & 0x01) != 0,
            and_mask: (b & 0x02) != 0,
            operand_len: (b >> 2) & 0x07,
            not: (b & 0x20) != 0,
            mask: (b & 0x40) != 0,
            reserved: false,
        }
    }
}

/// FlowSpec component encoding
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowComponent {
    pub comp_type: FlowComponentType,
    pub operator: FlowOperator,
    pub value: Vec<u8>,
}

impl FlowComponent {
    /// Encode component to bytes: <type(1), length(1), op(1), value(var)>
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(self.comp_type as u8);
        buf.push((1 + self.value.len()) as u8); // operator + value
        buf.push(self.operator.encode());
        buf.extend_from_slice(&self.value);
        buf
    }
}

/// FlowSpec NLRI value — list of components in strict type order
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowSpecNLRI {
    pub components: Vec<FlowComponent>,
}

impl FlowSpecNLRI {
    /// Encode NLRI to bytes. Components MUST be in strict increasing type order.
    pub fn encode(&self) -> Result<Vec<u8>> {
        // Verify strict type ordering
        for i in 1..self.components.len() {
            if self.components[i].comp_type as u8 <= self.components[i - 1].comp_type as u8 {
                return Err(BgpError::ComponentOrdering);
            }
        }

        let mut buf = Vec::new();
        for comp in &self.components {
            buf.extend_from_slice(&comp.encode());
        }
        Ok(buf)
    }

    /// Create a UDP reflection filter (DNS/NTP/CLDAP/SSDP source port)
    pub fn udp_reflection(dest_prefix: &str, ports: &[u16]) -> Result<Self> {
        let mut components: Vec<FlowComponent> = Vec::new();

        // Type 1: Destination Prefix
        let ip: std::net::IpAddr = dest_prefix.parse()?;
        match ip {
            std::net::IpAddr::V4(v4) => {
                let prefix_bytes = v4.octets().to_vec();
                components.push(FlowComponent {
                    comp_type: FlowComponentType::DestPrefix,
                    operator: FlowOperator {
                        equality: true,
                        and_mask: false,
                        operand_len: 0,
                        not: false,
                        mask: false,
                        reserved: false,
                    },
                    value: prefix_bytes,
                });
            }
            std::net::IpAddr::V6(_) => return Err(BgpError::InvalidPrefixLen(129)), // IPv6 Flowspec in RFC 8956
        }

        // Type 3: IP Protocol = UDP (17)
        components.push(FlowComponent {
            comp_type: FlowComponentType::IpProtocol,
            operator: FlowOperator {
                equality: true,
                and_mask: false,
                operand_len: 0,
                not: false,
                mask: false,
                reserved: false,
            },
            value: vec![17],
        });

        // Type 6: Source Port — for multiple ports we'd need multiple components
        for port in ports {
            components.push(FlowComponent {
                comp_type: FlowComponentType::SourcePort,
                operator: FlowOperator {
                    equality: true,
                    and_mask: false,
                    operand_len: 0,
                    not: false,
                    mask: false,
                    reserved: false,
                },
                value: port.to_be_bytes().to_vec(),
            });
        }

        Ok(FlowSpecNLRI { components })
    }

    /// Create RTBH FlowSpec (dest prefix + protocol equality match-all)
    pub fn rtbh_flowspec(dest_prefix: &str) -> Result<Self> {
        let ip: std::net::IpAddr = dest_prefix.parse()?;
        match ip {
            std::net::IpAddr::V4(v4) => {
                let prefix_bytes = v4.octets().to_vec();
                Ok(FlowSpecNLRI {
                    components: vec![FlowComponent {
                        comp_type: FlowComponentType::DestPrefix,
                        operator: FlowOperator {
                            equality: true,
                            and_mask: false,
                            operand_len: 0,
                            not: false,
                            mask: false,
                            reserved: false,
                        },
                        value: prefix_bytes,
                    }],
                })
            }
            std::net::IpAddr::V6(_) => Err(BgpError::InvalidPrefixLen(129)),
        }
    }
}

/// FlowSpec action communities (Transitive Extended Community Type 0x07)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowSpecAction {
    pub sub_type: u8,   // 0x01-0x09
    pub value: Vec<u8>, // action-specific value
}

impl FlowSpecAction {
    /// traffic-rate action: 8 octets (rate bytes/sec + burst bytes/sec)
    pub fn traffic_rate(rate: u32, burst: u32) -> Self {
        FlowSpecAction {
            sub_type: crate::attributes::flowspec_communities::TRAFFIC_RATE,
            value: {
                let mut v = rate.to_be_bytes().to_vec();
                v.extend_from_slice(&burst.to_be_bytes());
                v
            },
        }
    }

    /// traffic-action: drop = 0x01
    pub fn traffic_action_drop() -> Self {
        FlowSpecAction {
            sub_type: crate::attributes::flowspec_communities::TRAFFIC_ACTION,
            value: vec![0x01],
        }
    }
}
