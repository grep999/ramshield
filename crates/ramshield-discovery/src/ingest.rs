//! Bounded ingestion bridge from captured packets to completed service windows.
//!
//! Service identity and direction are explicit caller inputs. Source IPs are
//! never promoted to service identity, and incomplete windows are not sealed
//! as clean observations.
use crate::window::{
    MissingReason, RecordOutcome, WindowError, WindowKey, WindowLedger, WindowObservation,
    WindowState,
};
use crate::{ServiceId, TrafficDirection};
use ramshield_types::ConnectionEvent;
use std::collections::{BTreeMap, BTreeSet};

/// Default completed-window size (one hour).
const DEFAULT_WINDOW_SECONDS: u32 = 3_600;

#[derive(Debug, Clone, Copy)]
pub struct IngestConfig {
    pub window_seconds: u32,
    pub max_services: usize,
    pub max_pending_windows: usize,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            window_seconds: DEFAULT_WINDOW_SECONDS,
            max_services: 64,
            max_pending_windows: 4_096,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CollectorHealth {
    pub capture_drops: u64,
    pub ingest_drops: u64,
    pub malformed: u64,
}

impl CollectorHealth {
    fn is_clean(self) -> bool {
        self.capture_drops == 0 && self.ingest_drops == 0 && self.malformed == 0
    }
}

#[derive(Debug, Clone, Default)]
struct WindowAccumulator {
    packets: u64,
    captured_bytes: u64,
    wire_bytes: u64,
    truncated_packets: u64,
    health: CollectorHealth,
    zero_traffic_complete: bool,
}

impl WindowAccumulator {
    fn absorb(&mut self, event: &ConnectionEvent) {
        self.packets = self.packets.saturating_add(1);
        self.captured_bytes = self.captured_bytes.saturating_add(event.captured_bytes);
        self.wire_bytes = self.wire_bytes.saturating_add(event.wire_bytes);
        if event.truncated {
            self.truncated_packets = self.truncated_packets.saturating_add(1);
        }
    }

    fn merge(&mut self, other: Self) {
        self.packets = self.packets.saturating_add(other.packets);
        self.captured_bytes = self.captured_bytes.saturating_add(other.captured_bytes);
        self.wire_bytes = self.wire_bytes.saturating_add(other.wire_bytes);
        self.truncated_packets = self
            .truncated_packets
            .saturating_add(other.truncated_packets);
        self.health.capture_drops = self
            .health
            .capture_drops
            .saturating_add(other.health.capture_drops);
        self.health.ingest_drops = self
            .health
            .ingest_drops
            .saturating_add(other.health.ingest_drops);
        self.health.malformed = self.health.malformed.saturating_add(other.health.malformed);
        self.zero_traffic_complete |= other.zero_traffic_complete;
    }
}

/// In-memory, bounded accumulator backed by an idempotent persistent ledger.
pub struct IngestionEngine {
    config: IngestConfig,
    ledger: WindowLedger,
    pending: BTreeMap<(ServiceId, TrafficDirection, u64), WindowAccumulator>,
}

impl IngestionEngine {
    pub fn new(ledger_max_entries: usize, config: IngestConfig) -> Result<Self, WindowError> {
        if config.window_seconds == 0 || config.window_seconds > 86_400 {
            return Err(WindowError::InvalidWindow);
        }
        if config.max_services == 0 || config.max_pending_windows == 0 {
            return Err(WindowError::InvalidCapacity);
        }
        Ok(Self {
            config,
            ledger: WindowLedger::new(ledger_max_entries)?,
            pending: BTreeMap::new(),
        })
    }

    /// Add packet events to an explicitly identified service and direction.
    /// The batch is admitted atomically with respect to pending-capacity limits.
    pub fn record_batch(
        &mut self,
        service_id: ServiceId,
        direction: TrafficDirection,
        events: &[ConnectionEvent],
    ) -> Result<(), WindowError> {
        let mut staged: BTreeMap<(ServiceId, TrafficDirection, u64), WindowAccumulator> =
            BTreeMap::new();
        // Reject events with timestamps too far in the future to prevent
        // pending-capacity exhaustion from a single far-future sample.
        // Use a 1-hour tolerance measured in seconds.
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let max_future_secs = now_secs + 3_600; // 1 hour tolerance
        for event in events {
            let timestamp_secs = event.timestamp_ns / 1_000_000_000;
            if timestamp_secs > max_future_secs {
                return Err(WindowError::InvalidObservation);
            }
            if event.captured_bytes > event.wire_bytes
                || event.truncated != (event.wire_bytes > event.captured_bytes)
            {
                return Err(WindowError::InvalidObservation);
            }
            let timestamp_secs = event.timestamp_ns / 1_000_000_000;
            let window_seconds = u64::from(self.config.window_seconds);
            let window_start = (timestamp_secs / window_seconds) * window_seconds;
            staged
                .entry((service_id, direction, window_start))
                .or_default()
                .absorb(event);
        }
        self.check_capacity(staged.keys())?;
        if staged.keys().any(|key| {
            self.pending
                .get(key)
                .is_some_and(|pending| pending.zero_traffic_complete)
        }) {
            return Err(WindowError::InvalidObservation);
        }
        for (key, accumulator) in staged {
            self.pending.entry(key).or_default().merge(accumulator);
        }
        Ok(())
    }

    /// Attach loss/parser counters to one specific service-direction window.
    /// Global lifetime counters must not contaminate every future window.
    pub fn record_window_health(
        &mut self,
        service_id: ServiceId,
        direction: TrafficDirection,
        window_start_secs: u64,
        health: CollectorHealth,
    ) -> Result<(), WindowError> {
        if !window_start_secs.is_multiple_of(u64::from(self.config.window_seconds)) {
            return Err(WindowError::InvalidWindow);
        }
        let key = (service_id, direction, window_start_secs);
        self.check_capacity(std::iter::once(&key))?;
        let accumulator = self.pending.entry(key).or_default();
        accumulator.health.capture_drops = accumulator
            .health
            .capture_drops
            .saturating_add(health.capture_drops);
        accumulator.health.ingest_drops = accumulator
            .health
            .ingest_drops
            .saturating_add(health.ingest_drops);
        accumulator.health.malformed = accumulator
            .health
            .malformed
            .saturating_add(health.malformed);
        Ok(())
    }

    /// Mark a service-direction window as completely observed with zero traffic.
    /// This is distinct from an absent window, which remains Missing.
    pub fn record_empty_window(
        &mut self,
        service_id: ServiceId,
        direction: TrafficDirection,
        window_start_secs: u64,
    ) -> Result<(), WindowError> {
        if !window_start_secs.is_multiple_of(u64::from(self.config.window_seconds)) {
            return Err(WindowError::InvalidWindow);
        }
        let key = (service_id, direction, window_start_secs);
        self.check_capacity(std::iter::once(&key))?;
        let accumulator = self.pending.entry(key).or_default();
        if accumulator.packets > 0 {
            return Err(WindowError::InvalidObservation);
        }
        accumulator.zero_traffic_complete = true;
        Ok(())
    }

    fn check_capacity<'a>(
        &self,
        incoming: impl Iterator<Item = &'a (ServiceId, TrafficDirection, u64)>,
    ) -> Result<(), WindowError> {
        let mut new_keys = BTreeSet::new();
        let mut services: BTreeSet<ServiceId> = self.pending.keys().map(|(id, _, _)| *id).collect();
        for key in incoming {
            services.insert(key.0);
            if !self.pending.contains_key(key) {
                new_keys.insert(*key);
            }
        }
        if services.len() > self.config.max_services
            || self.pending.len().saturating_add(new_keys.len()) > self.config.max_pending_windows
        {
            return Err(WindowError::InvalidCapacity);
        }
        Ok(())
    }

    /// Seal only windows whose end time is at or before now_secs.
    pub fn seal_completed(&mut self, now_secs: u64) -> Result<Vec<RecordOutcome>, WindowError> {
        let duration = u64::from(self.config.window_seconds);
        let completed: Vec<_> = self
            .pending
            .keys()
            .filter(|(_, _, start)| {
                start
                    .checked_add(duration)
                    .is_some_and(|end| end <= now_secs)
            })
            .copied()
            .collect();
        let mut outcomes = Vec::with_capacity(completed.len());
        for (service_id, direction, window_start_secs) in completed {
            let Some(accumulator) =
                self.pending
                    .remove(&(service_id, direction, window_start_secs))
            else {
                continue;
            };
            let key = WindowKey {
                service_id: service_id.as_string(),
                direction,
                window_start_secs,
                window_seconds: self.config.window_seconds,
            };
            let state = if accumulator.packets == 0
                && accumulator.truncated_packets == 0
                && accumulator.health.is_clean()
                && !accumulator.zero_traffic_complete
            {
                WindowState::Missing {
                    reason: MissingReason::NoTelemetry,
                }
            } else if accumulator.truncated_packets > 0 || !accumulator.health.is_clean() {
                WindowState::Degraded {
                    packets: accumulator.packets,
                    captured_bytes: accumulator.captured_bytes,
                    wire_bytes: accumulator.wire_bytes,
                    truncated_packets: accumulator.truncated_packets,
                    capture_drops: accumulator.health.capture_drops,
                    ingest_drops: accumulator.health.ingest_drops,
                    malformed: accumulator.health.malformed,
                }
            } else {
                WindowState::Observed {
                    packets: accumulator.packets,
                    captured_bytes: accumulator.captured_bytes,
                    wire_bytes: accumulator.wire_bytes,
                }
            };
            outcomes.push(self.ledger.record(WindowObservation { key, state })?);
        }
        Ok(outcomes)
    }

    /// Refuse to persist while unsealed windows remain, rather than silently
    /// discarding them across restart.
    pub fn persist(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        if !self.pending.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "cannot persist with unsealed windows pending",
            ));
        }
        self.ledger.flush(path)
    }

    pub fn reopen(
        path: impl AsRef<std::path::Path>,
        max_entries: usize,
        config: IngestConfig,
    ) -> Result<Self, WindowError> {
        if config.window_seconds == 0 || config.window_seconds > 86_400 {
            return Err(WindowError::InvalidWindow);
        }
        if config.max_services == 0 || config.max_pending_windows == 0 {
            return Err(WindowError::InvalidCapacity);
        }
        Ok(Self {
            config,
            ledger: WindowLedger::reopen(path, max_entries)?,
            pending: BTreeMap::new(),
        })
    }

    pub fn ledger(&self) -> &WindowLedger {
        &self.ledger
    }

    pub fn ledger_mut(&mut self) -> &mut WindowLedger {
        &mut self.ledger
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::cloned_ref_to_slice_refs)]
    use super::*;
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

    fn engine(config: IngestConfig) -> IngestionEngine {
        IngestionEngine::new(100, config).unwrap()
    }

    fn key(service_id: ServiceId, direction: TrafficDirection) -> WindowKey {
        WindowKey {
            service_id: service_id.as_string(),
            direction,
            window_start_secs: 0,
            window_seconds: 3_600,
        }
    }

    #[test]
    fn explicit_service_and_direction_are_the_window_identity() {
        let mut eng = engine(IngestConfig::default());
        let service = ServiceId::new(7);
        let event = sample_event(
            Ipv4Addr::new(10, 0, 0, 1).into(),
            1_000_000_000,
            150,
            150,
            false,
        );
        eng.record_batch(service, TrafficDirection::Ingress, &[event.clone()])
            .unwrap();
        eng.record_batch(service, TrafficDirection::Egress, &[event])
            .unwrap();
        assert_eq!(eng.seal_completed(3_600).unwrap().len(), 2);
        assert!(matches!(
            eng.ledger().get(&key(service, TrafficDirection::Ingress)),
            Some(WindowState::Observed {
                packets: 1,
                captured_bytes: 150,
                wire_bytes: 150
            })
        ));
        assert!(matches!(
            eng.ledger().get(&key(service, TrafficDirection::Egress)),
            Some(WindowState::Observed { packets: 1, .. })
        ));
    }

    #[test]
    fn future_timestamp_rejected() {
        let mut eng = engine(IngestConfig::default());
        let service = ServiceId::new(7);
        // Record a valid event first
        let valid_event = sample_event(
            Ipv4Addr::new(10, 0, 0, 1).into(),
            1_000_000_000,
            150,
            150,
            false,
        );
        eng.record_batch(service, TrafficDirection::Ingress, &[valid_event]).unwrap();
        // Now reject an event with a timestamp beyond the 1-hour tolerance
        let far_future_event = sample_event(
            Ipv4Addr::new(10, 0, 0, 1).into(),
            u64::MAX, // far beyond any plausible wall time
            150,
            150,
            false,
        );
        assert!(eng.record_batch(service, TrafficDirection::Ingress, &[far_future_event]).is_err());
    }

    #[test]
    fn partial_windows_are_not_sealed_as_clean() {
        let mut eng = engine(IngestConfig::default());
        let service = ServiceId::new(8);
        let event = sample_event(
            Ipv4Addr::new(10, 0, 0, 2).into(),
            1_000_000_000,
            100,
            100,
            false,
        );
        eng.record_batch(service, TrafficDirection::Unknown, &[event])
            .unwrap();
        assert!(eng.seal_completed(3_599).unwrap().is_empty());
        assert_eq!(eng.pending_count(), 1);
        assert_eq!(
            eng.seal_completed(3_600).unwrap(),
            vec![RecordOutcome::Inserted]
        );
    }

    #[test]
    fn inconsistent_capture_lengths_are_rejected_without_partial_admission() {
        let mut eng = engine(IngestConfig::default());
        let service = ServiceId::new(16);
        let ip = Ipv4Addr::new(10, 0, 0, 16).into();
        let good = sample_event(ip, 1_000_000_000, 100, 100, false);
        let bad = sample_event(ip, 1_000_000_500, 150, 100, false);
        assert_eq!(
            eng.record_batch(service, TrafficDirection::Ingress, &[good, bad]),
            Err(WindowError::InvalidObservation)
        );
        assert_eq!(eng.pending_count(), 0);
    }

    #[test]
    fn packet_events_cannot_be_added_after_declaring_empty_window() {
        let mut eng = engine(IngestConfig::default());
        let service = ServiceId::new(17);
        eng.record_empty_window(service, TrafficDirection::Ingress, 0)
            .unwrap();
        let event = sample_event(
            Ipv4Addr::new(10, 0, 0, 17).into(),
            1_000_000_000,
            100,
            100,
            false,
        );
        assert_eq!(
            eng.record_batch(service, TrafficDirection::Ingress, &[event]),
            Err(WindowError::InvalidObservation)
        );
    }

    #[test]
    fn explicit_zero_traffic_is_observed_not_missing() {
        let mut eng = engine(IngestConfig::default());
        let service = ServiceId::new(15);
        eng.record_empty_window(service, TrafficDirection::Ingress, 0)
            .unwrap();
        assert_eq!(
            eng.seal_completed(3_600).unwrap(),
            vec![RecordOutcome::Inserted]
        );
        assert!(matches!(
            eng.ledger().get(&key(service, TrafficDirection::Ingress)),
            Some(WindowState::Observed {
                packets: 0,
                captured_bytes: 0,
                wire_bytes: 0,
            })
        ));
    }

    #[test]
    fn truncation_preserves_wire_and_captured_bytes_and_degrades_window() {
        let mut eng = engine(IngestConfig::default());
        let service = ServiceId::new(9);
        let event = sample_event(
            Ipv4Addr::new(10, 0, 0, 3).into(),
            1_000_000_000,
            100,
            150,
            true,
        );
        eng.record_batch(service, TrafficDirection::Ingress, &[event])
            .unwrap();
        eng.seal_completed(3_600).unwrap();
        assert!(matches!(
            eng.ledger().get(&key(service, TrafficDirection::Ingress)),
            Some(WindowState::Degraded {
                packets: 1,
                captured_bytes: 100,
                wire_bytes: 150,
                truncated_packets: 1,
                ..
            })
        ));
    }

    #[test]
    fn collector_health_is_scoped_to_one_service_window() {
        let mut eng = engine(IngestConfig::default());
        let a = ServiceId::new(10);
        let b = ServiceId::new(11);
        let event = sample_event(
            Ipv4Addr::new(10, 0, 0, 4).into(),
            1_000_000_000,
            100,
            100,
            false,
        );
        eng.record_window_health(
            a,
            TrafficDirection::Ingress,
            0,
            CollectorHealth {
                capture_drops: 5,
                ingest_drops: 3,
                malformed: 2,
            },
        )
        .unwrap();
        eng.record_batch(a, TrafficDirection::Ingress, &[event.clone()])
            .unwrap();
        eng.record_batch(b, TrafficDirection::Ingress, &[event])
            .unwrap();
        eng.seal_completed(3_600).unwrap();
        assert!(matches!(
            eng.ledger().get(&key(a, TrafficDirection::Ingress)),
            Some(WindowState::Degraded {
                capture_drops: 5,
                ingest_drops: 3,
                malformed: 2,
                ..
            })
        ));
        assert!(matches!(
            eng.ledger().get(&key(b, TrafficDirection::Ingress)),
            Some(WindowState::Observed { .. })
        ));
    }

    #[test]
    fn duplicate_and_conflicting_completed_windows_are_reported() {
        let mut eng = engine(IngestConfig::default());
        let service = ServiceId::new(12);
        let ip = Ipv4Addr::new(10, 0, 0, 5).into();
        let event = sample_event(ip, 1_000_000_000, 100, 100, false);
        eng.record_batch(service, TrafficDirection::Ingress, &[event.clone()])
            .unwrap();
        assert_eq!(
            eng.seal_completed(3_600).unwrap(),
            vec![RecordOutcome::Inserted]
        );
        eng.record_batch(service, TrafficDirection::Ingress, &[event])
            .unwrap();
        assert_eq!(
            eng.seal_completed(3_600).unwrap(),
            vec![RecordOutcome::Duplicate]
        );
        let conflict = sample_event(ip, 1_000_000_000, 200, 200, false);
        eng.record_batch(service, TrafficDirection::Ingress, &[conflict])
            .unwrap();
        assert_eq!(
            eng.seal_completed(3_600).unwrap(),
            vec![RecordOutcome::ConflictingDuplicate]
        );
    }

    #[test]
    fn pending_capacity_is_bounded_and_batch_admission_is_atomic() {
        let config = IngestConfig {
            max_pending_windows: 1,
            ..IngestConfig::default()
        };
        let mut eng = engine(config);
        let service = ServiceId::new(13);
        let events = [
            sample_event(
                Ipv4Addr::new(10, 0, 0, 6).into(),
                1_000_000_000,
                10,
                10,
                false,
            ),
            sample_event(
                Ipv4Addr::new(10, 0, 0, 6).into(),
                4_000_000_000_000,
                10,
                10,
                false,
            ),
        ];
        assert_eq!(
            eng.record_batch(service, TrafficDirection::Ingress, &events),
            Err(WindowError::InvalidCapacity)
        );
        assert_eq!(eng.pending_count(), 0);
    }

    #[test]
    fn persistence_refuses_unsealed_data_and_recovers_sealed_ledger() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ledger.json");
        let mut eng = engine(IngestConfig::default());
        let service = ServiceId::new(14);
        let event = sample_event(
            Ipv4Addr::new(10, 0, 0, 7).into(),
            1_000_000_000,
            120,
            120,
            false,
        );
        eng.record_batch(service, TrafficDirection::Ingress, &[event])
            .unwrap();
        assert_eq!(
            eng.persist(&path).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        eng.seal_completed(3_600).unwrap();
        eng.persist(&path).unwrap();
        let recovered = IngestionEngine::reopen(&path, 100, IngestConfig::default()).unwrap();
        assert!(matches!(
            recovered
                .ledger()
                .get(&key(service, TrafficDirection::Ingress)),
            Some(WindowState::Observed {
                wire_bytes: 120,
                captured_bytes: 120,
                ..
            })
        ));
    }
}
