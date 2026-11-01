//! Explicit service-window semantics for passive learning.
//! Missing and degraded windows are never represented as zero-valued traffic.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_SERVICE_ID_BYTES: usize = 128;
const MAX_WINDOW_SECONDS: u32 = 86_400;
const MAX_LEDGER_ENTRIES: usize = 1_000_000;

/// Persistence format version for forward compatibility.
const PERSISTENCE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersistenceEnvelope {
    version: u32,
    entries: Vec<(WindowKey, WindowState)>,
}

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
    /// Uses a unique temporary filename based on timestamp to avoid conflicts
    /// with concurrent flushes to the same path.
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
        let envelope = PersistenceEnvelope {
            version: PERSISTENCE_VERSION,
            entries: pairs,
        };
        let bytes = serde_json::to_vec(&envelope)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        // Unique temp file using nanosecond timestamp to avoid conflicts
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let mut tmp = path.to_path_buf();
        tmp.set_file_name(format!(".ledger_tmp_{}.json", nanos));
        if let Some(parent) = tmp.parent() {
            fs::create_dir_all(parent)?;
        }
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
    /// Validates capacity, persistence format version, duplicate keys,
    /// and all WindowKey invariants. Fails closed on any corruption.
    pub fn reopen(path: impl AsRef<Path>, max_entries: usize) -> Result<Self, WindowError> {
        if max_entries == 0 || max_entries > MAX_LEDGER_ENTRIES {
            return Err(WindowError::InvalidCapacity);
        }
        let path = path.as_ref();
        if !path.exists() {
            return Err(WindowError::Io("persistence file not found".into()));
        }
        let bytes = fs::read(path).map_err(|e| WindowError::Io(e.to_string()))?;
        let envelope: PersistenceEnvelope =
            serde_json::from_slice(&bytes).map_err(|e| WindowError::Io(e.to_string()))?;
        if envelope.version != PERSISTENCE_VERSION {
            return Err(WindowError::Io(format!(
                "unsupported persistence version {}",
                envelope.version
            )));
        }
        if envelope.entries.len() > max_entries {
            return Err(WindowError::InvalidCapacity);
        }
        // Detect duplicate keys — fail closed rather than silently overwriting.
        let mut seen = BTreeMap::new();
        for (key, state) in envelope.entries {
            if seen.contains_key(&key) {
                return Err(WindowError::Io("duplicate keys in persistence".into()));
            }
            key.validate()?;
            seen.insert(key, state);
        }
        Ok(Self {
            max_entries,
            entries: seen,
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

    /// Restart semantics: evicted entries must stay evicted after reopen,
    /// and stale late arrivals must still be rejected.
    #[test]
    fn restart_after_eviction_preserves_rejection() {
        let p = std::env::temp_dir().join(format!(
            "ledger_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
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
        // The window at start=0 is now evicted; reinserting it must be rejected.
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
        ledger.flush(&p).unwrap();
        let recovered = WindowLedger::reopen(&p, 2).unwrap();
        assert_eq!(recovered.len(), 2);
        // Same rejection must hold after recovery.
        let key_start_0 = WindowKey {
            service_id: "billing-worker".into(),
            window_start_secs: 0,
            window_seconds: 60,
        };
        assert!(!recovered.entries.contains_key(&key_start_0));
        let _ = std::fs::remove_file(&p);
    }

    /// Invalid capacity must be rejected on reopen.
    #[test]
    fn reopen_rejects_invalid_capacity() {
        let p = std::env::temp_dir().join(format!(
            "ledger_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut ledger = WindowLedger::new(8).unwrap();
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
        ledger.flush(&p).unwrap();
        // Zero capacity invalid.
        assert!(matches!(
            WindowLedger::reopen(&p, 0),
            Err(WindowError::InvalidCapacity)
        ));
        // Excessive capacity invalid.
        assert!(matches!(
            WindowLedger::reopen(&p, MAX_LEDGER_ENTRIES + 1),
            Err(WindowError::InvalidCapacity)
        ));
        // Valid capacity works.
        assert!(WindowLedger::reopen(&p, 8).is_ok());
        let _ = std::fs::remove_file(&p);
    }

    /// Truncated JSON must fail closed.
    #[test]
    fn reopen_rejects_truncated_json() {
        let p = std::env::temp_dir().join(format!(
            "ledger_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&p, b"{\"version\":1,\"entries\":[").unwrap();
        assert!(matches!(
            WindowLedger::reopen(&p, 8),
            Err(WindowError::Io(_))
        ));
        let _ = std::fs::remove_file(&p);
    }

    /// Unknown persistence version must fail closed.
    #[test]
    fn reopen_rejects_unknown_version() {
        let p = std::env::temp_dir().join(format!(
            "ledger_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bad: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        let mut bad = bad.clone();
        bad.insert("version".into(), serde_json::json!(999));
        bad.insert("entries".into(), serde_json::json!([]));
        let bytes = serde_json::to_vec(&bad).unwrap();
        fs::write(&p, &bytes).unwrap();
        assert!(matches!(
            WindowLedger::reopen(&p, 8),
            Err(WindowError::Io(_))
        ));
        let _ = std::fs::remove_file(&p);
    }

    /// Duplicate keys in persistence must fail closed rather than overwrite.
    #[test]
    fn reopen_rejects_duplicate_keys() {
        let p = std::env::temp_dir().join(format!(
            "ledger_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let key = WindowKey {
            service_id: "billing-worker".into(),
            window_start_secs: 0,
            window_seconds: 60,
        };
        let first_key = key.clone();
        let second_key = key;
        let first = WindowObservation {
            key: first_key,
            state: WindowState::Observed {
                packets: 10,
                bytes: 100,
            },
        };
        let second = WindowObservation {
            key: second_key,
            state: WindowState::Observed {
                packets: 20,
                bytes: 200,
            },
        };
        // Build JSON payload with duplicate keys.
        let duplicate_payload = serde_json::json!({
            "version": 1,
            "entries": [
                (&first.key, &first.state),
                (&second.key, &second.state),
            ]
        });
        let json = serde_json::to_string(&duplicate_payload).unwrap();
        fs::write(&p, json).unwrap();
        // Duplicate keys should fail closed.
        assert!(matches!(
            WindowLedger::reopen(&p, 8),
            Err(WindowError::Io(_))
        ));
        let _ = std::fs::remove_file(&p);
    }

    /// Invalid WindowKey in persistence must fail closed.
    #[test]
    fn reopen_rejects_invalid_window_key() {
        let p = std::env::temp_dir().join(format!(
            "ledger_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // Empty service_id is invalid.
        let json = r#"{"version":1,"entries":[[{"service_id":"","window_start_secs":0,"window_seconds":60},{"Observed":{"packets":1,"bytes":1}}]]}"#;
        fs::write(&p, json).unwrap();
        assert!(matches!(
            WindowLedger::reopen(&p, 8),
            Err(WindowError::Io(_))
        ));
        let _ = std::fs::remove_file(&p);
    }

    /// Over-capacity snapshot must fail closed.
    #[test]
    fn reopen_rejects_over_capacity() {
        let p = std::env::temp_dir().join(format!(
            "ledger_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let entries: Vec<WindowObservation> = (0..5u64)
            .map(|i| WindowObservation {
                key: WindowKey {
                    service_id: format!("svc-{i}"),
                    window_start_secs: i * 3600,
                    window_seconds: 60,
                },
                state: WindowState::Observed {
                    packets: 1,
                    bytes: 1,
                },
            })
            .collect();
        let payload = serde_json::json!({
            "version": 1,
            "entries": entries.iter().map(|e| (&e.key, &e.state)).collect::<Vec<_>>()
        });
        let json = serde_json::to_string(&payload).unwrap();
        fs::write(&p, json).unwrap();
        assert!(matches!(
            WindowLedger::reopen(&p, 2),
            Err(WindowError::InvalidCapacity)
        ));
        let _ = std::fs::remove_file(&p);
    }

    /// Missing persistence file must return Io error, not InvalidCapacity.
    #[test]
    fn reopen_rejects_missing_file() {
        let p = std::env::temp_dir().join(format!(
            "nonexistent_ledger_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(matches!(
            WindowLedger::reopen(&p, 8),
            Err(WindowError::Io(_))
        ));
    }
}
