pub mod cache;
pub mod config;
pub mod dashboard;
pub mod detection;
pub mod dns;
pub mod engine;
pub mod error;
pub mod forecasting;
pub mod ipc;
pub mod learning;
pub mod metrics;

pub mod storage;
pub mod util;

pub use crate::util::BoundedVecDeque;
pub use config::Config;
pub use detection::{BlockDecision, ConnectionEvent, DetectionEngine};
pub use engine::Engine;
pub use error::RsError;

/// Install panic hook that logs panics to stderr with ISO-8601 timestamp.
/// Call once near `main` entry point.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .unwrap_or(&"<non-string payload>");
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".to_string());
        eprintln!("{timestamp} PANIC at {location}: {payload}");
    }));
}
