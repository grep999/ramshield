#![recursion_limit = "512"]
//! RamShield library facade — thin re-export shell over workspace crates.
//! Domain logic lives in crates/ramshield-*; this module exists so the
//! binaries (main.rs, cli.rs) and glue (engine/, ipc/, dashboard/) keep
//! short import paths.

pub mod config {
    pub use ramshield_config::*;
}
pub mod dashboard;
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

/// Format a Unix timestamp (seconds since epoch) as RFC 3339 UTC.
// ponytail: std-only calendar math, replaces chrono (unmaintained per cargo audit).
fn rfc3339_from_unix(ts: i64) -> String {
    let (year, month, day, hour, min, sec) = civil_from_unix(ts);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, min, sec
    )
}

// ponytail: civil-from-days (Howard Hinnant's algorithm) for UTC breakdown.
fn civil_from_unix(ts: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400) as u32;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, secs / 3600, (secs % 3600) / 60, secs % 60)
}

/// Install panic hook that logs panics to stderr with ISO-8601 timestamp.
/// Call once near `main` entry point.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let timestamp = rfc3339_from_unix(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_from_unix_matches_known_instants() {
        assert_eq!(rfc3339_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_from_unix(1_700_000_000), "2023-11-14T22:13:20Z");
        // Leap year boundary.
        assert_eq!(rfc3339_from_unix(1_709_164_800), "2024-02-29T00:00:00Z");
    }
}
