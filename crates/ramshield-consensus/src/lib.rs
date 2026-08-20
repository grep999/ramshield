pub mod network;
pub mod raft_storage;
pub mod types;

use ramshield_storage::Store;
use std::sync::Arc;

pub struct ConsensusService {
    pub store: Arc<Store>,
}

impl ConsensusService {
    pub fn new(_node_id: u64, store: Arc<Store>) -> Self {
        Self { store }
    }
}
