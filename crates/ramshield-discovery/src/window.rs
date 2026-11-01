//! Explicit service-window semantics for passive learning.
//! Missing and degraded windows are never represented as zero-valued traffic.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MAX_SERVICE_ID_BYTES: usize = 128;
const MAX_WINDOW_SECONDS: u32 = 86_400;
const MAX_LEDGER_ENTRIES: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WindowKey {
    pub service_id: String,
    pub window_start_secs: u64,
    pub window_seconds: u32,
}

impl WindowKey {
    pub fn validate(&self) -> Result<(), WindowError> {
        if self.service_id.is_empty() || self.service_id.len() > MAX_SERVICE_ID_BYTES
            || !self.service_id.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) {
            return Err(WindowError::InvalidServiceId);
        }
        if self.window_seconds == 0 || self.window_seconds > MAX_WINDOW_SECONDS
            || !self.window_start_secs.is_multiple_of(u64::from(self.window_seconds)) {
            return Err(WindowError::InvalidWindow);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissingReason { CollectorUnavailable, NoTelemetry, ClockSkew, Unknown }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WindowState {
    Observed { packets: u64, bytes: u64 },
    Missing { reason: MissingReason },
    Degraded { packets: u64, bytes: u64, capture_drops: u64, ingest_drops: u64, malformed: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowObservation { pub key: WindowKey, pub state: WindowState }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome { Inserted, Duplicate, ConflictingDuplicate, InsertedAfterEviction, RejectedTooOld }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowError { InvalidServiceId, InvalidWindow, InvalidCapacity }

/// Bounded in-memory idempotency ledger. Persistence and runtime feeding are
/// deliberately owned by the caller, not by the packet capture hot path.
pub struct WindowLedger {
    max_entries: usize,
    entries: BTreeMap<WindowKey, WindowState>,
}

impl WindowLedger {
    pub fn new(max_entries: usize) -> Result<Self, WindowError> {
        if max_entries == 0 || max_entries > MAX_LEDGER_ENTRIES {
            return Err(WindowError::InvalidCapacity);
        }
        Ok(Self { max_entries, entries: BTreeMap::new() })
    }

    pub fn record(&mut self, observation: WindowObservation) -> Result<RecordOutcome, WindowError> {
        observation.key.validate()?;
        if let Some(existing) = self.entries.get(&observation.key) {
            return Ok(if existing == &observation.state { RecordOutcome::Duplicate }
                else { RecordOutcome::ConflictingDuplicate });
        }
        let mut outcome = RecordOutcome::Inserted;
        if self.entries.len() >= self.max_entries {
            let oldest = self.entries.keys().min_by_key(|key| key.window_start_secs).cloned();
            if let Some(oldest_key) = oldest {
                let oldest_start = oldest_key.window_start_secs;
                if observation.key.window_start_secs < oldest_start {
                    return Ok(RecordOutcome::RejectedTooOld);
                }
                self.entries.remove(&oldest_key);
                outcome = RecordOutcome::InsertedAfterEviction;
            }
        }
        self.entries.insert(observation.key, observation.state);
        Ok(outcome)
    }

    pub fn get(&self, key: &WindowKey) -> Option<&WindowState> { self.entries.get(key) }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn observation(state: WindowState, start: u64) -> WindowObservation {
        WindowObservation {
            key: WindowKey { service_id: "billing-worker".into(), window_start_secs: start, window_seconds: 60 },
            state,
        }
    }

    #[test]
    fn zero_traffic_is_distinct_from_missing_and_degraded() {
        let mut ledger = WindowLedger::new(8).unwrap();
        let zero = observation(WindowState::Observed { packets: 0, bytes: 0 }, 0);
        let missing = observation(WindowState::Missing { reason: MissingReason::CollectorUnavailable }, 60);
        let degraded = observation(WindowState::Degraded { packets: 0, bytes: 0, capture_drops: 4, ingest_drops: 0, malformed: 0 }, 120);
        assert_eq!(ledger.record(zero).unwrap(), RecordOutcome::Inserted);
        assert_eq!(ledger.record(missing).unwrap(), RecordOutcome::Inserted);
        assert_eq!(ledger.record(degraded).unwrap(), RecordOutcome::Inserted);
        assert_eq!(ledger.len(), 3);
    }

    #[test]
    fn duplicate_window_is_idempotent_and_conflict_is_visible() {
        let mut ledger = WindowLedger::new(8).unwrap();
        let first = observation(WindowState::Observed { packets: 10, bytes: 100 }, 0);
        assert_eq!(ledger.record(first.clone()).unwrap(), RecordOutcome::Inserted);
        assert_eq!(ledger.record(first).unwrap(), RecordOutcome::Duplicate);
        let conflict = observation(WindowState::Observed { packets: 11, bytes: 100 }, 0);
        assert_eq!(ledger.record(conflict).unwrap(), RecordOutcome::ConflictingDuplicate);
    }

    #[test]
    fn bounded_ledger_evicts_oldest_and_rejects_older_late_data() {
        let mut ledger = WindowLedger::new(2).unwrap();
        assert_eq!(ledger.record(observation(WindowState::Observed { packets: 1, bytes: 1 }, 0)).unwrap(), RecordOutcome::Inserted);
        assert_eq!(ledger.record(observation(WindowState::Observed { packets: 1, bytes: 1 }, 60)).unwrap(), RecordOutcome::Inserted);
        assert_eq!(ledger.record(observation(WindowState::Observed { packets: 1, bytes: 1 }, 120)).unwrap(), RecordOutcome::InsertedAfterEviction);
        assert_eq!(ledger.record(observation(WindowState::Observed { packets: 1, bytes: 1 }, 30)).unwrap(), RecordOutcome::RejectedTooOld);
        assert_eq!(ledger.len(), 2);
    }
}