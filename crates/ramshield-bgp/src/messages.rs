use crate::{BgpError, Result};

/// BGP message types per RFC 4271
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum BgpMsgType {
    Open = 1,
    Update = 2,
    Notification = 3,
    Keepalive = 4,
}

impl TryFrom<u8> for BgpMsgType {
    type Error = BgpError;
    fn try_from(v: u8) -> Result<Self> {
        match v {
            1 => Ok(BgpMsgType::Open),
            2 => Ok(BgpMsgType::Update),
            3 => Ok(BgpMsgType::Notification),
            4 => Ok(BgpMsgType::Keepalive),
            _ => Err(BgpError::BadType(v)),
        }
    }
}

/// Fixed BGP header (19 octets minimum)
#[derive(Debug, Clone)]
pub struct BgpHeader {
    /// 16 octets, all 1s for authentication (often omitted with TCP-AO)
    pub marker: [u8; 16],
    /// Message length in octets (19-4096), includes header
    pub length: u16,
    /// Message type: 1=OPEN, 2=UPDATE, 3=NOTIFICATION, 4=KEEPALIVE
    pub msg_type: BgpMsgType,
}

impl BgpHeader {
    /// Minimum length of a valid BGP header
    pub const MIN_LEN: usize = 19;

    /// Verify that the given byte slice is a valid BGP header
    pub fn verify(buf: &[u8]) -> Result<Self> {
        if buf.len() < Self::MIN_LEN {
            return Err(BgpError::TooShort {
                expected: Self::MIN_LEN,
                got: buf.len(),
            });
        }
        // Marker must be all 0xFF (16 octets) per RFC 4271
        if buf[0..16] != [0xFFu8; 16] {
            return Err(BgpError::BadMagic([0u8; 2]));
        }
        let marker = buf[0..16]
            .try_into()
            .map_err(|_| BgpError::BadMagic([0u8; 2]))?;
        let length = u16::from_be_bytes(
            buf[16..18]
                .try_into()
                .map_err(|_| BgpError::BadMagic([0u8; 2]))?,
        );
        let msg_type = BgpMsgType::try_from(buf[18])?;

        // Length must be at least 19 and at most 4096. Full-frame presence is
        // enforced where the body is read (session.rs) — verify() is also
        // called on a bare 19-octet header before the body is available.
        if !(19..=4096).contains(&length) {
            return Err(BgpError::TooShort {
                expected: length as usize,
                got: buf.len(),
            });
        }

        Ok(BgpHeader {
            marker,
            length,
            msg_type,
        })
    }
}

/// OPEN message — sent first after TCP connection
#[derive(Debug, Clone)]
pub struct BgpOpen {
    /// BGP version (must be 4)
    pub version: u8,
    /// My AS number
    pub my_asn: u16,
    /// Hold time in seconds (KEEPALIVE * 3 typical)
    pub hold_time: u16,
    /// BGP identifier (my router ID)
    pub bgp_id: u32,
    /// Optional parameters (CAPABILITIES, etc.)
    pub opt_params: Vec<BgpCapability>,
}

/// BGP capability advertisement
#[derive(Debug, Clone)]
pub struct BgpCapability {
    /// Capability code: 1 = Multiprotocol Extensions
    pub code: u8,
    /// Capability length (4 for multiprotocol)
    pub length: u8,
    /// Capability value: AFI (2) | Reserved (1) | SAFI (1)
    pub value: [u8; 4],
}

/// KEEPALIVE message — no body (19 octets)
#[derive(Debug, Clone, Copy)]
pub struct BgpKeepalive;

/// NOTIFICATION message — sent on error
#[derive(Debug, Clone)]
pub struct BgpNotification {
    /// Error code (see RFC 4271 Section 8)
    pub error_code: u8,
    /// Error subcode (specific to code)
    pub error_subcode: u8,
    /// Optional data (varies by subcode)
    pub data: Vec<u8>,
}

impl BgpKeepalive {
    /// Encode a KEEPALIVE message (exactly 19 octets)
    pub fn encode() -> Vec<u8> {
        // Marker (16 ones) + Length (19) + Type (4)
        let mut buf = Vec::with_capacity(19);
        buf.extend_from_slice(&[0xFFu8; 16]); // Marker
        buf.extend_from_slice(&19u16.to_be_bytes()); // Length
        buf.push(4); // KEEPALIVE type
        buf
    }
}

/// Decoded BGP message
#[derive(Debug, Clone)]
pub enum BgpMessage {
    Open(BgpOpen),
    Update(Vec<u8>), // Raw UPDATE body for now
    Notification(BgpNotification),
    Keepalive,
}

impl BgpMessage {
    pub fn from_parts(hdr: BgpHeader, body: Option<Vec<u8>>) -> Result<Self> {
        match hdr.msg_type {
            BgpMsgType::Open => {
                let body = body.ok_or(BgpError::TooShort {
                    expected: 10,
                    got: 0,
                })?;
                let open = decode_open(&body)?;
                Ok(BgpMessage::Open(open))
            }
            BgpMsgType::Update => {
                let body = body.unwrap_or_default();
                validate_update_body(&body)?;
                Ok(BgpMessage::Update(body))
            }
            BgpMsgType::Notification => {
                let body = body.ok_or(BgpError::TooShort {
                    expected: 2,
                    got: 0,
                })?;
                let notif = decode_notification(&body)?;
                Ok(BgpMessage::Notification(notif))
            }
            BgpMsgType::Keepalive => {
                if hdr.length != 19 || body.is_some() {
                    return Err(BgpError::Session(
                        "KEEPALIVE length must be exactly 19 octets".into(),
                    ));
                }
                Ok(BgpMessage::Keepalive)
            }
        }
    }
}

/// Structural validation for a received UPDATE body.
///
/// This is deliberately not a full RIB/decision process. It enforces the
/// framing invariants that prevent malformed lengths from reaching callers:
/// RFC 4271 §4.3 body length, IPv4 withdrawn-NLRI bounds, and path-attribute
/// length bounds (including the extended-length flag).
pub fn validate_update_body(body: &[u8]) -> Result<()> {
    if body.len() < 4 {
        return Err(BgpError::TooShort {
            expected: 4,
            got: body.len(),
        });
    }
    let withdrawn_len = u16::from_be_bytes([body[0], body[1]]) as usize;
    let attr_len = u16::from_be_bytes([body[2], body[3]]) as usize;
    let expected = 4usize
        .checked_add(withdrawn_len)
        .and_then(|n| n.checked_add(attr_len))
        .ok_or_else(|| BgpError::Session("UPDATE length overflow".into()))?;
    if body.len() != expected {
        return Err(BgpError::TooShort {
            expected,
            got: body.len(),
        });
    }

    let withdrawn = &body[4..4 + withdrawn_len];
    validate_ipv4_nlri(withdrawn)?;

    let attrs = &body[4 + withdrawn_len..];
    validate_path_attrs(attrs)?;
    Ok(())
}

fn validate_ipv4_nlri(buf: &[u8]) -> Result<()> {
    let mut pos = 0;
    while pos < buf.len() {
        let prefix_len = buf[pos] as usize;
        pos += 1;
        if prefix_len > 32 {
            return Err(BgpError::InvalidPrefixLen(prefix_len as u8));
        }
        let octets = prefix_len.div_ceil(8);
        if pos + octets > buf.len() {
            return Err(BgpError::TooShort {
                expected: pos + octets,
                got: buf.len(),
            });
        }
        pos += octets;
    }
    Ok(())
}

fn validate_well_known_flags(attr_type: u8, flags: u8) -> Result<()> {
    // RFC 4271 §5: well-known attributes have optional=0, transitive=1,
    // partial=0. Extended length (0x10) is allowed.
    let well_known = attr_type <= 7;
    if well_known && flags & 0xE0 != 0x40 {
        return Err(BgpError::Session(format!(
            "well-known attribute {attr_type} has invalid flags 0x{flags:02x}"
        )));
    }
    Ok(())
}

fn validate_path_attrs(buf: &[u8]) -> Result<()> {
    let mut pos = 0;
    while pos < buf.len() {
        if buf.len() - pos < 2 {
            return Err(BgpError::TooShort {
                expected: pos + 2,
                got: buf.len(),
            });
        }
        let flags = buf[pos];
        let attr_type = buf[pos + 1];
        validate_well_known_flags(attr_type, flags)?;
        pos += 1;
        pos += 1; // attribute type
        let len = if flags & 0x10 != 0 {
            if buf.len() - pos < 2 {
                return Err(BgpError::TooShort {
                    expected: pos + 2,
                    got: buf.len(),
                });
            }
            let len = u16::from_be_bytes([buf[pos], buf[pos + 1]]) as usize;
            pos += 2;
            len
        } else {
            let len = buf[pos] as usize;
            pos += 1;
            len
        };
        if pos + len > buf.len() {
            return Err(BgpError::TooShort {
                expected: pos + len,
                got: buf.len(),
            });
        }
        pos += len;
    }
    Ok(())
}

/// Decode OPEN message body
fn decode_open(buf: &[u8]) -> Result<BgpOpen> {
    if buf.len() < 10 {
        return Err(BgpError::TooShort {
            expected: 10,
            got: buf.len(),
        });
    }
    let version = buf[0];
    if version != 4 {
        return Err(BgpError::BadVersion(version));
    }
    let my_asn = u16::from_be_bytes([buf[1], buf[2]]);
    let hold_time = u16::from_be_bytes([buf[3], buf[4]]);
    let bgp_id = u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]]);
    // RFC 4271: opt_params_len is the total byte length of the optional
    // parameters field, NOT a count of individual parameters.
    let opt_params_len = buf[9] as usize;
    let mut opt_params = Vec::new();
    if opt_params_len > 0 {
        let params_start = 10;
        let params_end = params_start + opt_params_len;
        if params_end > buf.len() {
            return Err(BgpError::TooShort {
                expected: params_end,
                got: buf.len(),
            });
        }
        let mut pos = params_start;
        while pos + 2 <= params_end {
            let param_type = buf[pos];
            let param_len = buf[pos + 1] as usize;
            pos += 2;
            if pos + param_len > params_end {
                return Err(BgpError::TooShort {
                    expected: params_end,
                    got: buf.len(),
                });
            }
            // Only parse capability parameters (type 2). Unknown capabilities
            // are skipped, but MP-BGP (code 1) must be exactly 4 octets;
            // silently truncating it would downgrade the peer's SAFI set.
            if param_type == 2 {
                let cap_data = &buf[pos..pos + param_len];
                let mut cap_pos = 0;
                while cap_pos + 2 <= cap_data.len() {
                    let code = cap_data[cap_pos];
                    let length = cap_data[cap_pos + 1] as usize;
                    cap_pos += 2;
                    if cap_pos + length > cap_data.len() {
                        return Err(BgpError::TooShort {
                            expected: cap_data.len(),
                            got: cap_pos + length,
                        });
                    }
                    if code == 1 {
                        if length != 4 {
                            return Err(BgpError::Session(
                                "MP-BGP capability must be exactly 4 octets".into(),
                            ));
                        }
                        let mut value = [0u8; 4];
                        value.copy_from_slice(&cap_data[cap_pos..cap_pos + 4]);
                        opt_params.push(BgpCapability {
                            code,
                            length: 4,
                            value,
                        });
                    }
                    cap_pos += length;
                }
                if cap_pos != cap_data.len() {
                    return Err(BgpError::Session(
                        "truncated capability parameter list".into(),
                    ));
                }
            }
            pos += param_len;
        }
    }
    Ok(BgpOpen {
        version,
        my_asn,
        hold_time,
        bgp_id,
        opt_params,
    })
}

/// Decode NOTIFICATION message body
fn decode_notification(buf: &[u8]) -> Result<BgpNotification> {
    if buf.len() < 2 {
        return Err(BgpError::TooShort {
            expected: 2,
            got: buf.len(),
        });
    }
    let error_code = buf[0];
    let error_subcode = buf[1];
    let data = if buf.len() > 2 {
        buf[2..].to_vec()
    } else {
        Vec::new()
    };
    Ok(BgpNotification {
        error_code,
        error_subcode,
        data,
    })
}

/// Decode a KEEPALIVE message
pub fn decode_keepalive(buf: &[u8]) -> Result<()> {
    let hdr = BgpHeader::verify(buf)?;
    if hdr.msg_type != BgpMsgType::Keepalive {
        return Err(BgpError::BadType(hdr.msg_type as u8));
    }
    if hdr.length != 19 {
        return Err(BgpError::TooShort {
            expected: 19,
            got: buf.len(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod open_tests {
    use super::*;

    /// An OPEN carrying the MP-BGP SAFI 133 capability must decode to a
    /// capability list a peer can read back.
    #[test]
    fn multiprotocol_flowspec_capability_round_trips() {
        let cap = [1u8, 4, 0, 1, 0, 133]; // code, len, AFI=1, rsvd, SAFI=133
        let mut body = Vec::new();
        body.push(4); // version
        body.extend_from_slice(&65001u16.to_be_bytes()); // my ASN
        body.extend_from_slice(&90u16.to_be_bytes()); // hold time
        body.extend_from_slice(&0x0A00_0001u32.to_be_bytes()); // router id
        body.push(2 + cap.len() as u8); // optional params length
        body.push(2); // parameter type: capability
        body.push(cap.len() as u8);
        body.extend_from_slice(&cap);

        let open = decode_open(&body).unwrap();
        assert_eq!(open.version, 4);
        assert_eq!(open.my_asn, 65001);
        assert_eq!(open.hold_time, 90);
        assert_eq!(open.bgp_id, 0x0A00_0001);
        assert_eq!(open.opt_params.len(), 1, "one capability parsed");
        let c = &open.opt_params[0];
        assert_eq!(c.code, 1, "multiprotocol");
        assert_eq!(c.length, 4);
        assert_eq!(&c.value, &[0, 1, 0, 133], "AFI IPv4 + SAFI 133");
    }

    #[test]
    fn truncated_capability_is_rejected_not_panicked() {
        let mut body = vec![4, 0xFD, 0xE9, 0, 90, 10, 0, 0, 1];
        body.push(4); // opt params len claims 4 bytes
        body.extend_from_slice(&[2, 8, 1, 4]); // only 4 present, param needs 8
        assert!(decode_open(&body).is_err());
    }

    #[test]
    fn open_without_optional_params_still_decodes() {
        let mut body = vec![4, 0xFD, 0xE9, 0, 90, 10, 0, 0, 1];
        body.push(0); // no optional parameters
        let open = decode_open(&body).unwrap();
        assert!(open.opt_params.is_empty());
    }

    #[test]
    fn unknown_capability_is_skipped_without_truncation() {
        let mut body = vec![4, 0xFD, 0xE9, 0, 90, 10, 0, 0, 1];
        body.push(4); // opt params length
        body.extend_from_slice(&[2, 2, 65, 0]); // cap code 65, len 0
        let open = decode_open(&body).unwrap();
        assert!(open.opt_params.is_empty());
    }

    #[test]
    fn oversized_mp_capability_is_rejected() {
        let mut body = vec![4, 0xFD, 0xE9, 0, 90, 10, 0, 0, 1];
        body.push(5); // opt params length
        body.extend_from_slice(&[2, 3, 1, 5, 0]); // MP cap len 5
        assert!(decode_open(&body).is_err());
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;

    fn update_body(withdrawn: &[u8], attrs: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&(withdrawn.len() as u16).to_be_bytes());
        body.extend_from_slice(&(attrs.len() as u16).to_be_bytes());
        body.extend_from_slice(withdrawn);
        body.extend_from_slice(attrs);
        body
    }

    #[test]
    fn valid_update_with_withdrawn_and_origin_passes() {
        let body = update_body(&[24, 10, 0, 0], &[0x40, 1, 1, 0]);
        assert!(validate_update_body(&body).is_ok());

        let hdr = BgpHeader {
            marker: [0xFF; 16],
            length: (19 + body.len()) as u16,
            msg_type: BgpMsgType::Update,
        };
        assert!(matches!(
            BgpMessage::from_parts(hdr, Some(body)).unwrap(),
            BgpMessage::Update(_)
        ));
    }

    #[test]
    fn update_body_length_mismatch_is_rejected() {
        let mut body = update_body(&[24, 10, 0, 0], &[0x40, 1, 1, 0]);
        body.push(0);
        assert!(validate_update_body(&body).is_err());
    }

    #[test]
    fn invalid_withdrawn_prefix_length_is_rejected() {
        let body = update_body(&[33], &[]);
        assert!(matches!(
            validate_update_body(&body),
            Err(BgpError::InvalidPrefixLen(33))
        ));
    }

    #[test]
    fn truncated_path_attribute_is_rejected() {
        // attr flags/type say length 5, but only one value octet follows.
        let body = update_body(&[], &[0x40, 1, 5, 0]);
        assert!(validate_update_body(&body).is_err());
    }

    #[test]
    fn extended_length_attribute_bounds_are_enforced() {
        let attr = [0x50, 14, 0, 1, 0];
        let body = update_body(&[], &attr);
        assert!(validate_update_body(&body).is_ok());

        let bad = update_body(&[], &[0x50, 14, 0, 2, 0]);
        assert!(validate_update_body(&bad).is_err());
    }

    #[test]
    fn well_known_attribute_flags_are_enforced() {
        // ORIGIN (type 1) with well-known transitive flags 0x40.
        assert!(validate_update_body(&update_body(&[], &[0x40, 1, 1, 0])).is_ok());
        // Same attribute marked optional (0x80) must be rejected.
        assert!(validate_update_body(&update_body(&[], &[0x80, 1, 1, 0])).is_err());
        // Optional transitive COMMUNITIES (type 8) is allowed.
        assert!(validate_update_body(&update_body(&[], &[0xC0, 8, 1, 0])).is_ok());
    }
}
