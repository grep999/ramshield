use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;
use crate::{Store, Entry};

pub async fn run_ttl_wheel(
    _store: Arc<Store>,
    _command_tx: mpsc::Sender<Entry>,
) {
    info!("TTL wheel running.");
}
