use crate::message::Message;
pub use crate::message::PROTOCOL_VERSION;
use std::io::{self, Cursor, Read};

/// Wire magic bytes: `RSHP` (0x52 0x53 0x48 0x50).
pub const MAGIC: u32 = 0x5253_4850;

/// Maximum allowed payload size per frame.
pub const MAX_PAYLOAD_LEN: usize = 1 << 20; // 1 MiB

/// Decode error for the IPC wire protocol.
#[derive(Debug, Clone, PartialEq)]
pub enum DecodeError {
    /// Not enough bytes for a complete frame header.
    IncompleteHeader,
    /// Magic bytes mismatch.
    InvalidMagic(u32),
    /// Protocol version not supported by this implementation.
    UnsupportedVersion(u16),
    /// Payload length exceeds the configured maximum.
    PayloadTooLarge(usize),
    /// CRC checksum mismatch.
    CrcMismatch { expected: u32, actual: u32 },
    /// I/O error while reading.
    Io(io::ErrorKind),
    /// Deserialization error of the message body.
    Deserialize(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::IncompleteHeader => write!(f, "incomplete frame header"),
            DecodeError::InvalidMagic(got) => write!(f, "invalid magic: 0x{got:08x}"),
            DecodeError::UnsupportedVersion(v) => write!(f, "unsupported version: {v}"),
            DecodeError::PayloadTooLarge(len) => write!(f, "payload too large: {len} bytes"),
            DecodeError::CrcMismatch { expected, actual } => {
                write!(
                    f,
                    "CRC mismatch: expected 0x{expected:08x}, got 0x{actual:08x}"
                )
            }
            DecodeError::Io(kind) => write!(f, "I/O error: {kind:?}"),
            DecodeError::Deserialize(e) => write!(f, "deserialize error: {e}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Encode a [`Message`] into a length-delimited, CRC-protected binary frame.
///
/// Returns `[magic: u32 | version: u16 | len: u32 | payload | crc: u32]`.
pub fn encode(msg: &Message) -> Result<Vec<u8>, DecodeError> {
    // ponytail: JSON payload (not bincode) — internally-tagged enums need
    // deserialize_any; also human-debuggable on the wire. Swap to bincode+variant
    // enums only if frame size ever matters.
    let payload = serde_json::to_vec(msg).map_err(|e| DecodeError::Deserialize(e.to_string()))?;

    let payload_len = payload.len() as u32;
    if payload_len as usize > MAX_PAYLOAD_LEN {
        return Err(DecodeError::PayloadTooLarge(payload_len as usize));
    }

    // Build frame: magic + version + length + payload + crc
    let mut frame = Vec::with_capacity(4 + 2 + 4 + payload.len() + 4);
    frame.extend_from_slice(&MAGIC.to_le_bytes());
    frame.extend_from_slice(&msg.version.to_le_bytes());
    frame.extend_from_slice(&payload_len.to_le_bytes());
    frame.extend_from_slice(&payload);

    // CRC over everything except the CRC field itself (payload only).
    let crc = crc32fast::hash(&payload);
    frame.extend_from_slice(&crc.to_le_bytes());

    Ok(frame)
}

/// Decode a complete binary frame into a [`Message`].
///
/// Expects to consume exactly one frame from the provided `&[u8]`.
/// Returns `(message, bytes_consumed)` — caller advances their buffer by the
/// returned count and calls again for the next frame.
pub fn decode(buf: &[u8]) -> Result<(Message, usize), DecodeError> {
    // Minimum: 4 magic + 2 version + 4 length + 0 payload + 4 crc = 14 bytes
    if buf.len() < 14 {
        return Err(DecodeError::IncompleteHeader);
    }

    let mut cursor = Cursor::new(buf);

    // Read magic
    let mut magic_bytes = [0u8; 4];
    cursor
        .read_exact(&mut magic_bytes)
        .map_err(|_| DecodeError::Io(io::ErrorKind::UnexpectedEof))?;
    let magic = u32::from_le_bytes(magic_bytes);
    if magic != MAGIC {
        return Err(DecodeError::InvalidMagic(magic));
    }

    // Read version
    let mut version_bytes = [0u8; 2];
    cursor
        .read_exact(&mut version_bytes)
        .map_err(|_| DecodeError::Io(io::ErrorKind::UnexpectedEof))?;
    let version = u16::from_le_bytes(version_bytes);
    if version != PROTOCOL_VERSION {
        return Err(DecodeError::UnsupportedVersion(version));
    }

    // Read payload length
    let mut len_bytes = [0u8; 4];
    cursor
        .read_exact(&mut len_bytes)
        .map_err(|_| DecodeError::Io(io::ErrorKind::UnexpectedEof))?;
    let payload_len = u32::from_le_bytes(len_bytes) as usize;
    if payload_len > MAX_PAYLOAD_LEN {
        return Err(DecodeError::PayloadTooLarge(payload_len));
    }

    // Check if we have enough data for payload + CRC
    let total_needed = 4 + 2 + 4 + payload_len + 4;
    if buf.len() < total_needed {
        return Err(DecodeError::IncompleteHeader);
    }

    // Read payload
    let payload_start = cursor.position() as usize;
    let payload_end = payload_start + payload_len;
    let payload = &buf[payload_start..payload_end];

    // Read and verify CRC
    let crc_start = payload_end;
    let crc_bytes = &buf[crc_start..crc_start + 4];
    let expected_crc = crc32fast::hash(payload);
    let actual_crc = u32::from_le_bytes(crc_bytes.try_into().unwrap());
    if expected_crc != actual_crc {
        return Err(DecodeError::CrcMismatch {
            expected: expected_crc,
            actual: actual_crc,
        });
    }

    // Deserialize payload
    let msg: Message =
        serde_json::from_slice(payload).map_err(|e| DecodeError::Deserialize(e.to_string()))?;

    Ok((msg, total_needed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Request, Response};

    #[test]
    fn roundtrip_get_status() {
        let msg = Message::request(Request::GetStatus);
        let frame = encode(&msg).unwrap();
        let (decoded, consumed) = decode(&frame).unwrap();
        assert_eq!(decoded, msg);
        assert_eq!(consumed, frame.len());
    }

    #[test]
    fn roundtrip_block_ip() {
        let msg = Message::request(Request::BlockIp {
            ip: "10.0.0.1".into(),
            reason: "syn_flood".into(),
            ttl_secs: Some(3600),
        });
        let frame = encode(&msg).unwrap();
        let (decoded, consumed) = decode(&frame).unwrap();
        assert_eq!(decoded, msg);
        assert_eq!(consumed, frame.len());
    }

    #[test]
    fn roundtrip_response_ok() {
        let msg = Message::response(Response::Ok {
            message: "ok".into(),
            state: None,
        });
        let frame = encode(&msg).unwrap();
        let (decoded, consumed) = decode(&frame).unwrap();
        assert_eq!(decoded, msg);
        assert_eq!(consumed, frame.len());
    }

    #[test]
    fn roundtrip_batch_response() {
        let msg = Message::response(Response::BatchOk {
            accepted: 500,
            rejected: 2,
        });
        let frame = encode(&msg).unwrap();
        let (decoded, consumed) = decode(&frame).unwrap();
        assert_eq!(decoded, msg);
        assert_eq!(consumed, frame.len());
    }

    #[test]
    fn rejects_bad_magic() {
        let msg = Message::request(Request::GetStatus);
        let mut frame = encode(&msg).unwrap();
        frame[0] = 0x00;
        let err = decode(&frame).unwrap_err();
        assert!(matches!(err, DecodeError::InvalidMagic(_)));
    }

    #[test]
    fn rejects_bad_crc() {
        let frame = encode(&Message::request(Request::GetStatus)).unwrap();
        let last_idx = frame.len() - 1;
        let mut corrupted = frame.clone();
        corrupted[last_idx] ^= 0xFF;
        let err = decode(&corrupted).unwrap_err();
        assert!(matches!(err, DecodeError::CrcMismatch { .. }));
    }

    #[test]
    fn rejects_bad_version() {
        let frame = encode(&Message::request(Request::GetStatus)).unwrap();
        let mut corrupted = frame.clone();
        // Corrupt version byte (bytes 4-5)
        corrupted[4] = 0xFF;
        let err = decode(&corrupted).unwrap_err();
        assert!(matches!(err, DecodeError::UnsupportedVersion(_)));
    }

    #[test]
    fn incomplete_header() {
        let err = decode(&[0x52, 0x53, 0x48]).unwrap_err();
        assert!(matches!(err, DecodeError::IncompleteHeader));
    }

    #[test]
    fn double_frame_in_buffer() {
        let msg1 = Message::request(Request::GetStatus);
        let msg2 = Message::request(Request::GetStats);
        let frame1 = encode(&msg1).unwrap();
        let frame2 = encode(&msg2).unwrap();
        let mut buf = frame1.clone();
        buf.extend_from_slice(&frame2);
        let (decoded1, consumed1) = decode(&buf).unwrap();
        assert_eq!(decoded1, msg1);
        let remaining = &buf[consumed1..];
        let (decoded2, consumed2) = decode(remaining).unwrap();
        assert_eq!(decoded2, msg2);
        assert_eq!(consumed2, remaining.len());
    }
}
