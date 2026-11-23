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

        // Length must be at least 19 and at most 4096
        if !(19..=4096).contains(&length) || buf.len() < length as usize {
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
            BgpMsgType::Update => Ok(BgpMessage::Update(body.unwrap_or_default())),
            BgpMsgType::Notification => {
                let body = body.ok_or(BgpError::TooShort {
                    expected: 2,
                    got: 0,
                })?;
                let notif = decode_notification(&body)?;
                Ok(BgpMessage::Notification(notif))
            }
            BgpMsgType::Keepalive => Ok(BgpMessage::Keepalive),
        }
    }
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
            // Only parse capability parameters (type 2)
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
                    let mut value = [0u8; 4];
                    let copy_len = std::cmp::min(4, length);
                    value[..copy_len].copy_from_slice(&cap_data[cap_pos..cap_pos + copy_len]);
                    opt_params.push(BgpCapability {
                        code,
                        length: length as u8,
                        value,
                    });
                    cap_pos += length;
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
