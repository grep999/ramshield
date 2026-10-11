//! Fleet federation: AWORSet CRDT + authenticated TCP transport.
pub mod aworset;
pub mod hlc;
pub mod transport;
pub use transport::{MeshHandle, MeshMessage, MeshPeer};
