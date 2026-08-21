//! # ramshield-protocol
//!
//! Shared, versioned wire protocol for RamShield IPC.
//!
//! Every IPC peer (server, CLI, any future client) uses this crate so that the
//! on-the-wire format can never drift between them. The format is a
//! length-delimited, CRCs, version-tagged binary codec — no newline JSON, no
//! raw bincode.
//!
//! ## Wire layout (little-endian)
//!
//! ```text
//! ┌────────────────┬───────────────┬───────────────────────────────┐
//! │ magic: u32     │ version: u8   │ payload_len: u32               │
//! │ 0x5253_4850    │               │ (bytes of `payload` only)      │
//! │ "RSHP"         │               │                               │
//! ├────────────────┴───────────────┼───────────────────────────────┤
//! │ payload: bincode(Envelope<T>)  │ crc32: u32                     │
//! │ (length == payload_len)        │ crc32 over the whole payload   │
//! └────────────────────────────────┴───────────────────────────────┘
//! ```
//!
//! Total frame size = 4 (magic) + 1 (version) + 4 (len) + payload_len + 4 (crc).
//!
//! # Compatibility
//!
//! `PROTOCOL_VERSION` is negotiated implicitly by the version byte in the
//! frame header. A reader rejects a frame whose version byte is unknown with
//! [`DecodeError::UnsupportedVersion`], so a newer client talking to an older
//! server fails loudly instead of silently misparsing. The message body is
//! `bincode`-encoded — versioned independently of the frame header, but kept in
//! lockstep with it for now (see `ponytail:` notes).

pub mod codec;
pub mod message;

pub use codec::{DecodeError, MAGIC, MAX_PAYLOAD_LEN, decode, encode};
pub use message::PROTOCOL_VERSION;
pub use message::*;
