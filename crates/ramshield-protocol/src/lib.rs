//! # ramshield-protocol
//!
//! Shared wire protocol for RamShield IPC.
//!
//! Every IPC peer (server, CLI, any future client) uses this crate so that the
//! on-the-wire format can never drift between them. The transport is
//! newline-delimited JSON (`serde_json`) over TCP; the message schema lives in
//! [`message`] (`Request`/`Response`, internally-tagged by `"type"`,
//! `deny_unknown_fields` — field-name typos fail loudly).
//!
//! Frame authentication (HMAC-SHA256, optional per `[ipc] auth_keys`) lives in
//! [`auth`]: senders wrap frames as
//! `{"auth":{"key_id","ts_ms","sig"},"type":...}`; the server verifies,
//! strips the envelope, then parses the inner `Request`.

pub mod auth;
pub mod message;

pub use message::PROTOCOL_VERSION;
pub use message::*;
