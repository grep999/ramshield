//! Fleet federation: AWORSet CRDT + authenticated TCP transport.
pub mod aworset;
/// Per-peer condition-driven anti-entropy scheduling used by the mesh transport.
pub mod condition_sync;
pub mod hlc;
pub mod transport;
pub use transport::{MeshHandle, MeshMessage, MeshPeer};
