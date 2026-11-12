#![recursion_limit = "512"]
//! RamShield library facade — thin re-export shell over workspace crates.
//! Domain logic lives in crates/ramshield-*; this module exists so the
//! binaries (main.rs, cli.rs) and glue (engine/, ipc/, dashboard/) keep
//! short import paths.

pub mod config {
    pub use ramshield_config::*;
}
pub mod dashboard;
pub mod doctor;
pub mod detection {
    pub use ramshield_detection::*;
}
pub mod engine;
pub mod enforcement {
    pub use ramshield_enforcement::*;
}
pub mod forecasting {
    pub use ramshield_forecasting::*;
}
pub mod ipc;
pub mod metrics {
    pub use ramshield_metrics::*;
}
pub mod storage {
    pub use ramshield_storage::*;
}

pub use engine::Engine;

pub use ramshield_config::{Config, ConfigHandle};
pub use ramshield_detection::DetectionEngine;
pub use ramshield_forecasting::Forecaster;
pub use ramshield_metrics::{
    BatchRecord, BlockRecord, DashboardSnapshot, Metrics, ModuleStats, SubnetRow,
};
pub use ramshield_storage::Store;
pub use ramshield_types::{
    BlockDecision, BlockReason, ConnectionEvent, EnforceAction, EnforceCommand, EnforceResult,
};

/// Install panic hook that logs panics to stderr with ISO-8601 timestamp.
/// Call once near `main` entry point.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let timestamp = chrono::Utc::now().to_rfc3339();
        // `panic!` with a literal payload is a `&str`; a formatted payload is a
        // `String`. Downcasting only to `&str` lost the message for every
        // format-style panic — which is most of them, and all of the ones this
        // codebase can actually hit.
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string payload>".to_string());
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".to_string());
        eprintln!("{timestamp} PANIC at {location}: {payload}");
    }));
}
