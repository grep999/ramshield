//! Ingestion bridge: ConnectionEvent → WindowObservation → WindowLedger.
//! Authoritative, idempotent, crash-recoverable.

use crate::ServiceId;
use crate::window::{
    MissingReason, RecordOutcome, WindowError, WindowKey, WindowLedger, WindowObservation,
    WindowState,
};
use ramshield_types::ConnectionEvent;
use std::collections::HashMap;

/// Default window size in seconds (1 minute for granular learning).
const DEFAULT_WINDOW_SECONDS: u32 = 60;

/// Ingestion configuration.
#[derive(Debug, Clone, Copy)]
pub struct IngestConfig {
    /// Window size in seconds.
    pub window_seconds: u32,
    /// Maximum services to track (bounds memory).
    pub max_services: usize,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            window_seconds: DEFAULT_WINDOW_SECONDS,
            max_services: 64,
        }
    }
}

/// Per-service, per-window accumulator.
#[derive(Debug, Default)]
#[allow(dead_code)]
struct WindowAccumulator {
    packets: u64,
    bytes_captured: u64,
    bytes_wire: u64,
    truncations: u64,
    capture_drops: u64,
    ingest_drops: u64,
    malformed: u64,
}

/// Ingestion engine: batches ConnectionEvents into WindowObservations and records them.
pub struct IngestionEngine {
    config: IngestConfig,
    ledger: WindowLedger,
    /// Pending accumulators keyed by (ServiceId, window_start_secs).
    pending: HashMap<(ServiceId, u64), WindowAccumulator>,
    /// Count of events since last flush.
    events_since_flush: usize,
    /// Collector health counters.
    pub collector_health: CollectorHealth,
}

/// Collector health snapshot for WindowState::Degraded.
#[derive(Debug, Clone, Copy, Default)]
pub struct CollectorHealth {
    pub capture_drops: u64,
    pub ingest_drops: u64,
    pub malformed: u64,
}

impl IngestionEngine {
    /// Create a new ingestion engine with the given ledger capacity.
    pub fn new(ledger_max_entries: usize, config: IngestConfig) -> Result<Self, WindowError> {
        Ok(Self {
            config,
            ledger: WindowLedger::new(ledger_max_entries)?,
            pending: HashMap::new(),
            events_since_flush: 0,
            collector_health: CollectorHealth::default(),
        })
    }

    /// Record a batch of ConnectionEvents into pending accumulators.
    pub fn record_batch(&mut self, events: &[ConnectionEvent]) -> Vec<RecordOutcome> {
        for ev in events {
            let service_id = ServiceId::from_ip(ev.ip);
            let ts_secs = ev.timestamp_ns / 1_000_000_000;
            let window_start = if self.config.window_seconds == 0 {
                ts_secs
            } else {
                (ts_secs / self.config.window_seconds as u64) * self.config.window_seconds as u64
            };

            let key = (service_id, window_start);
            let acc = self.pending.entry(key).or_default();

            acc.packets = acc.packets.saturating_add(1);
            acc.bytes_captured = acc.bytes_captured.saturating_add(ev.captured_bytes);
            acc.bytes_wire = acc.bytes_wire.saturating_add(ev.wire_bytes);

            if ev.truncated {
                acc.truncations = acc.truncations.saturating_add(1);
            }
        }

        self.events_since_flush = self.events_since_flush.saturating_add(events.len());
        Vec::new()
    }

    /// Flush all pending accumulators to the ledger.
    pub fn flush_pending(&mut self) -> Result<Vec<RecordOutcome>, WindowError> {
        let mut outcomes = Vec::new();

        for ((service_id, window_start), acc) in self.pending.drain() {
            let key = WindowKey {
                service_id: service_id.as_string(),
                window_start_secs: window_start,
                window_seconds: self.config.window_seconds,
            };

            // Determine window state based on data quality.
            let state = if acc.packets == 0 {
                WindowState::Missing {
                    reason: MissingReason::NoTelemetry,
                }
            } else if acc.truncations > 0
                || self.collector_health.capture_drops > 0
                || self.collector_health.ingest_drops > 0
                || self.collector_health.malformed > 0
            {
                WindowState::Degraded {
                    packets: acc.packets,
                    bytes: acc.bytes_captured,
                    capture_drops: self.collector_health.capture_drops,
                    ingest_drops: self.collector_health.ingest_drops,
                    malformed: self.collector_health.malformed,
                }
            } else {
                WindowState::Observed {
                    packets: acc.packets,
                    bytes: acc.bytes_captured,
                }
            };

            let obs = WindowObservation { key, state };
            let outcome = self.ledger.record(obs)?;
            outcomes.push(outcome);
        }

        // Remove flushed entries (already drained).
        self.events_since_flush = 0;
        Ok(outcomes)
    }

    /// Persist the ledger atomically (temp file → fsync → rename → dir fsync).
    pub fn persist(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        self.ledger.flush(path)
    }

    /// Recover a ledger from disk and create a new engine.
    pub fn reopen(
        path: impl AsRef<std::path::Path>,
        max_entries: usize,
        config: IngestConfig,
    ) -> Result<Self, WindowError> {
        let ledger = WindowLedger::reopen(path, max_entries)?;
        Ok(Self {
            config,
            ledger,
            pending: HashMap::new(),
            events_since_flush: 0,
            collector_health: CollectorHealth::default(),
        })
    }

    /// Get a reference to the underlying ledger for inspection.
    pub fn ledger(&self) -> &WindowLedger {
        &self.ledger
    }

    /// Get mutable reference to the ledger for advanced operations.
    pub fn ledger_mut(&mut self) -> &mut WindowLedger {
        &mut self.ledger
    }

    /// Record collector health drop counters.
    pub fn record_capture_drops(&mut self, count: u64) {
        self.collector_health.capture_drops =
            self.collector_health.capture_drops.saturating_add(count);
    }

    pub fn record_ingest_drops(&mut self, count: u64) {
        self.collector_health.ingest_drops =
            self.collector_health.ingest_drops.saturating_add(count);
    }

    pub fn record_malformed(&mut self, count: u64) {
        self.collector_health.malformed = self.collector_health.malformed.saturating_add(count);
    }

    /// Number of pending windows not yet flushed.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ramshield_types::ConnectionEvent;
    use std::net::{IpAddr, Ipv4Addr};
    use tempfile::tempdir;

    fn sample_event(
        ip: IpAddr,
        ts_ns: u64,
        captured: u64,
        wire: u64,
        truncated: bool,
    ) -> ConnectionEvent {
        ConnectionEvent {
            ip,
            timestamp_ns: ts_ns,
            wire_bytes: wire,
            captured_bytes: captured,
            truncated,
            status_code: 200,
            proto_fingerprint: 1,
            l7: None,
        }
    }

    #[test]
    fn events_aggregate_into_windows_and_flush_to_ledger() {
        let _dir = tempdir().unwrap();
        let mut eng = IngestionEngine::new(100, IngestConfig::default()).unwrap();

        let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 1).into();
        let events = vec![
            sample_event(ip, 1_000_000_000, 100, 100, false),
            sample_event(ip, 1_000_000_500, 200, 200, false),
            sample_event(ip, 1_000_000_999, 50, 50, false),
        ];
        eng.record_batch(&events);
        let outcomes = eng.flush_pending().unwrap();

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0], RecordOutcome::Inserted);

        // Verify ledger content
        let key = WindowKey {
            service_id: ServiceId::from_ip(ip).as_string(),
            window_start_secs: 0, // (1 / 60) * 60 = 0 for ts_secs=1
            window_seconds: 60,
        };
        let state = eng
            .ledger()
            .get(&key)
            .expect("window should exist after flush");
        assert!(matches!(
            state,
            WindowState::Observed {
                packets: 3,
                bytes: 350
            }
        ));
    }

    #[test]
    fn truncation_marks_window_degraded() {
        let _dir = tempdir().unwrap();
        let mut eng = IngestionEngine::new(100, IngestConfig::default()).unwrap();

        let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 2).into();
        let events = vec![
            sample_event(ip, 1_000_000_000, 100, 150, true), // truncated
        ];
        eng.record_batch(&events);
        eng.flush_pending().unwrap();
        // No need to capture flush outcomes for this test - just verify window state
        let key = WindowKey {
            service_id: ServiceId::from_ip(ip).as_string(),
            window_start_secs: 0, // (1 / 60) * 60 = 0 for ts_secs=1
            window_seconds: 60,
        };
        let state = eng
            .ledger()
            .get(&key)
            .expect("degraded window should exist after flush");
        assert!(matches!(state, WindowState::Degraded { .. }));
    }

    #[test]
    fn idempotent_duplicate_windows_are_detected() {
        let _dir = tempdir().unwrap();
        let mut eng = IngestionEngine::new(100, IngestConfig::default()).unwrap();

        let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 3).into();
        let events = vec![sample_event(ip, 1_000_000_000, 100, 100, false)];
        eng.record_batch(&events);
        eng.flush_pending().unwrap();

        // Flush same window again
        eng.record_batch(&events);
        let outcomes = eng.flush_pending().unwrap();

        assert!(matches!(outcomes[0], RecordOutcome::Duplicate));
    }

    #[test]
    fn conflicting_duplicate_is_visible() {
        let _dir = tempdir().unwrap();
        let mut eng = IngestionEngine::new(100, IngestConfig::default()).unwrap();

        let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 4).into();
        let events = vec![sample_event(ip, 1_000_000_000, 100, 100, false)];
        eng.record_batch(&events);
        eng.flush_pending().unwrap();

        // Different data for same window
        let conflict = vec![sample_event(ip, 1_000_000_000, 200, 200, false)];
        eng.record_batch(&conflict);
        let outcomes = eng.flush_pending().unwrap();

        assert!(matches!(outcomes[0], RecordOutcome::ConflictingDuplicate));
    }

    #[test]
    fn persist_and_reopen_recovers_state() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ledger.json");

        {
            let mut eng = IngestionEngine::new(100, IngestConfig::default()).unwrap();
            let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 5).into();
            let events = vec![sample_event(ip, 1_000_000_000, 100, 100, false)];
            eng.record_batch(&events);
            eng.flush_pending().unwrap();
            eng.persist(&path).unwrap();
        }

        // Reopen and verify
        let recovered = IngestionEngine::reopen(&path, 100, IngestConfig::default()).unwrap();
        let key = WindowKey {
            service_id: ServiceId::from_ip(Ipv4Addr::new(10, 0, 0, 5).into()).as_string(),
            window_start_secs: 0, // (1 / 60) * 60 = 0 for ts_secs=1
            window_seconds: 60,
        };
        let state = recovered
            .ledger()
            .get(&key)
            .expect("recovered window should exist after reopen");
        assert!(matches!(
            state,
            WindowState::Observed {
                packets: 1,
                bytes: 100
            }
        ));
    }

    #[test]
    fn collector_health_feeds_into_degraded_state() {
        let mut eng = IngestionEngine::new(100, IngestConfig::default()).unwrap();

        let ip: IpAddr = Ipv4Addr::new(10, 0, 0, 6).into();
        eng.record_capture_drops(5);
        eng.record_ingest_drops(3);
        eng.record_malformed(2);

        let events = vec![sample_event(ip, 1_000_000_000, 100, 100, false)];
        eng.record_batch(&events);
        eng.flush_pending().unwrap();

        let key = WindowKey {
            service_id: ServiceId::from_ip(ip).as_string(),
            window_start_secs: 0, // (1 / 60) * 60 = 0 for ts_secs=1
            window_seconds: 60,
        };
        let state = eng
            .ledger()
            .get(&key)
            .expect("window with collector health should exist after flush");
        if let WindowState::Degraded {
            capture_drops,
            ingest_drops,
            malformed,
            ..
        } = state
        {
            assert_eq!(*capture_drops, 5);
            assert_eq!(*ingest_drops, 3);
            assert_eq!(*malformed, 2);
        } else {
            panic!("expected Degraded state");
        }
    }

    #[test]
    fn bounded_ledger_evicts_oldest() {
        let mut eng = IngestionEngine::new(2, IngestConfig::default()).unwrap();

        let ip1: IpAddr = Ipv4Addr::new(10, 0, 0, 10).into();
        let ip2: IpAddr = Ipv4Addr::new(10, 0, 0, 11).into();

        // Fill to capacity
        for i in 0..2 {
            let events = vec![sample_event(
                ip1,
                i as u64 * 60_000_000_000,
                100,
                100,
                false,
            )];
            eng.record_batch(&events);
            eng.flush_pending().unwrap();
        }
        assert_eq!(eng.ledger().len(), 2);

        // Add third window -> evicts oldest
        let events = vec![sample_event(ip2, 120_000_000_000, 100, 100, false)];
        eng.record_batch(&events);
        eng.flush_pending().unwrap();

        assert_eq!(eng.ledger().len(), 2);
    }
}
