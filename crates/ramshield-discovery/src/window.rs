//! Explicit service-window semantics for passive learning.
//! Missing and degraded windows are never represented as zero-valued traffic.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
};

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
        if self.service_id.is_empty()
            || self.service_id.len() > MAX_SERVICE_ID_BYTES
            || !self
                .service_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(WindowError::InvalidServiceId);
        }
        if self.window_seconds == 0
            || self.window_seconds > MAX_WINDOW_SECONDS
            || !self
                .window_start_secs
                .is_multiple_of(u64::from(self.window_seconds))
        {
            return Err(WindowError::InvalidWindow);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissingReason {
    CollectorUnavailable,
    NoTelemetry,
    ClockSkew,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WindowState {
    Observed {
        packets: u64,
        bytes: u64,
    },
    Missing {
        reason: MissingReason,
    },
    Degraded {
        packets: u64,
        bytes: u64,
        capture_drops: u64,
        ingest_drops: u64,
        malformed: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowObservation {
    pub key: WindowKey,
    pub state: WindowState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome {
    Inserted,
    Duplicate,
    ConflictingDuplicate,
    InsertedAfterEviction,
    RejectedTooOld,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowError {
    InvalidServiceId,
    InvalidWindow,
    InvalidCapacity,
    Io(String),
}

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
        Ok(Self {
            max_entries,
            entries: BTreeMap::new(),
        })
    }

    pub fn record(&mut self, observation: WindowObservation) -> Result<RecordOutcome, WindowError> {
        observation.key.validate()?;
        if let Some(existing) = self.entries.get(&observation.key) {
            return Ok(if existing == &observation.state {
                RecordOutcome::Duplicate
            } else {
                RecordOutcome::ConflictingDuplicate
            });
        }
        let mut outcome = RecordOutcome::Inserted;
        if self.entries.len() >= self.max_entries {
            let oldest = self
                .entries
                .keys()
                .min_by_key(|key| key.window_start_secs)
                .cloned();
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

    pub fn get(&self, key: &WindowKey) -> Option<&WindowState> {
        self.entries.get(key)
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Atomic snapshot persistence: temp file, file fsync, rename, directory fsync.
    pub fn flush(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let pairs: Vec<(WindowKey, WindowState)> = self
            .entries
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let bytes = serde_json::to_vec(&pairs)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut tmp = path.to_path_buf();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("json");
        tmp.set_extension(format!("{ext}.tmp"));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)?;
        if let Some(parent) = path.parent() {
            File::open(parent)?.sync_all()?;
        }
        Ok(())
    }

    /// Reconstruct a WindowLedger from a persisted snapshot.
    pub fn reopen(path: impl AsRef<Path>, max_entries: usize) -> Result<Self, WindowError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(WindowError::InvalidCapacity);
        }
        let bytes = fs::read(path).map_err(|e| WindowError::Io(e.to_string()))?;
        let pairs: Vec<(WindowKey, WindowState)> =
            serde_json::from_slice(&bytes).map_err(|e| WindowError::Io(e.to_string()))?;
        let entries: BTreeMap<WindowKey, WindowState> = pairs.into_iter().collect();
        if entries.len() > max_entries {
            return Err(WindowError::InvalidCapacity);
        }
        for key in entries.keys() {
            key.validate()?;
        }
        Ok(Self {
            max_entries,
            entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn observation(state: WindowState, start: u64) -> WindowObservation {
        WindowObservation {
            key: WindowKey {
                service_id: "billing-worker".into(),
                window_start_secs: start,
                window_seconds: 60,
            },
            state,
        }
    }

    #[test]
    fn zero_traffic_is_distinct_from_missing_and_degraded() {
        let mut ledger = WindowLedger::new(8).unwrap();
        let zero = observation(
            WindowState::Observed {
                packets: 0,
                bytes: 0,
            },
            0,
        );
        let missing = observation(
            WindowState::Missing {
                reason: MissingReason::CollectorUnavailable,
            },
            60,
        );
        let degraded = observation(
            WindowState::Degraded {
                packets: 0,
                bytes: 0,
                capture_drops: 4,
                ingest_drops: 0,
                malformed: 0,
            },
            120,
        );
        assert_eq!(ledger.record(zero).unwrap(), RecordOutcome::Inserted);
        assert_eq!(ledger.record(missing).unwrap(), RecordOutcome::Inserted);
        assert_eq!(ledger.record(degraded).unwrap(), RecordOutcome::Inserted);
        assert_eq!(ledger.len(), 3);
    }

    #[test]
    fn duplicate_window_is_idempotent_and_conflict_is_visible() {
        let mut ledger = WindowLedger::new(8).unwrap();
        let first = observation(
            WindowState::Observed {
                packets: 10,
                bytes: 100,
            },
            0,
        );
        assert_eq!(
            ledger.record(first.clone()).unwrap(),
            RecordOutcome::Inserted
        );
        assert_eq!(ledger.record(first).unwrap(), RecordOutcome::Duplicate);
        let conflict = observation(
            WindowState::Observed {
                packets: 11,
                bytes: 100,
            },
            0,
        );
        assert_eq!(
            ledger.record(conflict).unwrap(),
            RecordOutcome::ConflictingDuplicate
        );
    }

    #[test]
    fn bounded_ledger_evicts_oldest_and_rejects_older_late_data() {
        let mut ledger = WindowLedger::new(2).unwrap();
        assert_eq!(
            ledger
                .record(observation(
                    WindowState::Observed {
                        packets: 1,
                        bytes: 1
                    },
                    0
                ))
                .unwrap(),
            RecordOutcome::Inserted
        );
        assert_eq!(
            ledger
                .record(observation(
                    WindowState::Observed {
                        packets: 1,
                        bytes: 1
                    },
                    60
                ))
                .unwrap(),
            RecordOutcome::Inserted
        );
        assert_eq!(
            ledger
                .record(observation(
                    WindowState::Observed {
                        packets: 1,
                        bytes: 1
                    },
                    120
                ))
                .unwrap(),
            RecordOutcome::InsertedAfterEviction
        );
        assert_eq!(
            ledger
                .record(observation(
                    WindowState::Observed {
                        packets: 1,
                        bytes: 1
                    },
                    0
                ))
                .unwrap(),
            RecordOutcome::RejectedTooOld
        );
        assert_eq!(ledger.len(), 2);
    }

    #[test]
    fn persisted_state_recovers_after_reopen() {
        let p = std::env::temp_dir().join(format!(
            "ledger_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut ledger = WindowLedger::new(8).unwrap();
        let zero = observation(
            WindowState::Observed {
                packets: 42,
                bytes: 1000,
            },
            0,
        );
        assert_eq!(ledger.record(zero).unwrap(), RecordOutcome::Inserted);
        ledger.flush(&p).unwrap();
        let recovered = WindowLedger::reopen(&p, 8).unwrap();
        assert_eq!(recovered.len(), 1);
        let key = WindowKey {
            service_id: "billing-worker".into(),
            window_start_secs: 0,
            window_seconds: 60,
        };
        assert_eq!(
            recovered.entries[&key],
            WindowState::Observed {
                packets: 42,
                bytes: 1000
            }
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn persistence_evicts_exceeds_max_entries() {
        let p = std::env::temp_dir().join(format!(
            "ledger_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut ledger = WindowLedger::new(2).unwrap();
        let a = observation(
            WindowState::Observed {
                packets: 1,
                bytes: 1,
            },
            0,
        );
        let b = observation(
            WindowState::Observed {
                packets: 2,
                bytes: 2,
            },
            60,
        );
        let c = observation(
            WindowState::Observed {
                packets: 3,
                bytes: 3,
            },
            120,
        );
        assert_eq!(ledger.record(a).unwrap(), RecordOutcome::Inserted);
        assert_eq!(ledger.record(b).unwrap(), RecordOutcome::Inserted);
        assert_eq!(
            ledger.record(c).unwrap(),
            RecordOutcome::InsertedAfterEviction
        );
        ledger.flush(&p).unwrap();
        let recovered = WindowLedger::reopen(&p, 2).unwrap();
        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered.entries.len(), 2);
        let _ = std::fs::remove_file(&p);
    }
}
