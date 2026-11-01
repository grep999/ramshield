//! Bounded, direction-aware service-window ledger for passive learning.
//! Missing and degraded windows are never represented as zero-valued traffic.
//! Eviction uses a deterministic event-time key and persists a replay watermark.
use super::TrafficDirection;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
const MAX_PERSISTED_ENTRY_BYTES: u64 = 512;
const MAX_PERSISTED_ENVELOPE_BYTES: u64 = 512;

const MAX_SERVICE_ID_BYTES: usize = 128;
const MAX_WINDOW_SECONDS: u32 = 86_400;
const MAX_LEDGER_ENTRIES: usize = 1_000_000;

/// Persistence format version for forward compatibility.
const PERSISTENCE_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersistenceEnvelope {
    version: u32,
    entries: Vec<(WindowKey, WindowState)>,
    eviction_watermark: Option<WindowKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
/// The field order defines deterministic eviction order: event time, service,
/// window width, then direction.
pub struct WindowKey {
    pub window_start_secs: u64,
    pub service_id: String,
    pub window_seconds: u32,
    pub direction: TrafficDirection,
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

/// Bounded in-memory idempotency ledger. Snapshot persistence and runtime
/// feeding are deliberately owned by the caller, not by the packet capture hot path.
/// The watermark rejects any key at or below the greatest evicted key, including
/// equal-timestamp replays from another service or direction.
pub struct WindowLedger {
    max_entries: usize,
    entries: BTreeMap<WindowKey, WindowState>,
    eviction_watermark: Option<WindowKey>,
}

impl WindowLedger {
    pub fn new(max_entries: usize) -> Result<Self, WindowError> {
        if max_entries == 0 || max_entries > MAX_LEDGER_ENTRIES {
            return Err(WindowError::InvalidCapacity);
        }
        Ok(Self {
            max_entries,
            entries: BTreeMap::new(),
            eviction_watermark: None,
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
        if self
            .eviction_watermark
            .as_ref()
            .is_some_and(|watermark| &observation.key <= watermark)
        {
            return Ok(RecordOutcome::RejectedTooOld);
        }

        let mut outcome = RecordOutcome::Inserted;
        if self.entries.len() >= self.max_entries {
            let Some(oldest_key) = self.entries.keys().next().cloned() else {
                return Err(WindowError::Io(
                    "ledger is at capacity but has no oldest entry".into(),
                ));
            };
            if observation.key <= oldest_key {
                return Ok(RecordOutcome::RejectedTooOld);
            }
            self.entries.remove(&oldest_key);
            self.eviction_watermark = Some(oldest_key);
            outcome = RecordOutcome::InsertedAfterEviction;
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

    /// Atomic snapshot persistence: exclusive temp file, file fsync, rename,
    /// then directory fsync. Callers must serialize concurrent writers to one path.
    pub fn flush(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if path.file_name().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ledger snapshot path must name a file",
            ));
        }
        let parent = parent_dir(path);
        fs::create_dir_all(parent)?;

        let pairs: Vec<(WindowKey, WindowState)> = self
            .entries
            .iter()
            .map(|(key, state)| (key.clone(), state.clone()))
            .collect();
        let envelope = PersistenceEnvelope {
            version: PERSISTENCE_VERSION,
            entries: pairs,
            eviction_watermark: self.eviction_watermark.clone(),
        };
        let bytes = serde_json::to_vec(&envelope)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if bytes.len() as u64 > max_snapshot_bytes(self.max_entries) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ledger snapshot exceeds configured capacity bound",
            ));
        }

        let (tmp, mut file) = create_temp_file(path)?;
        let write_result = file.write_all(&bytes).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = write_result {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        if let Err(error) = fs::rename(&tmp, path) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        File::open(parent)?.sync_all()
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
        let metadata = fs::metadata(path).map_err(|error| WindowError::Io(error.to_string()))?;
        if metadata.len() > max_snapshot_bytes(max_entries) {
            return Err(WindowError::Io(
                "persistence file exceeds configured capacity bound".into(),
            ));
        }
        let bytes = fs::read(path).map_err(|error| WindowError::Io(error.to_string()))?;
        let envelope: PersistenceEnvelope =
            serde_json::from_slice(&bytes).map_err(|error| WindowError::Io(error.to_string()))?;
        if envelope.version != PERSISTENCE_VERSION {
            return Err(WindowError::Io(format!(
                "unsupported persistence version {}",
                envelope.version
            )));
        }
        if envelope.entries.len() > max_entries {
            return Err(WindowError::InvalidCapacity);
        }
        if let Some(watermark) = envelope.eviction_watermark.as_ref() {
            watermark.validate().map_err(|error| {
                WindowError::Io(format!("invalid eviction watermark: {error:?}"))
            })?;
        }

        let mut seen = BTreeMap::new();
        for (key, state) in envelope.entries {
            if seen.contains_key(&key) {
                return Err(WindowError::Io("duplicate keys in persistence".into()));
            }
            key.validate().map_err(|error| {
                WindowError::Io(format!("invalid window key in persistence: {error:?}"))
            })?;
            if envelope
                .eviction_watermark
                .as_ref()
                .is_some_and(|watermark| &key <= watermark)
            {
                return Err(WindowError::Io(
                    "retained entry is at or below eviction watermark".into(),
                ));
            }
            seen.insert(key, state);
        }
        Ok(Self {
            max_entries,
            entries: seen,
            eviction_watermark: envelope.eviction_watermark,
        })
    }
}

fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn max_snapshot_bytes(max_entries: usize) -> u64 {
    (max_entries as u64)
        .saturating_mul(MAX_PERSISTED_ENTRY_BYTES)
        .saturating_add(MAX_PERSISTED_ENVELOPE_BYTES)
}

fn create_temp_file(path: &Path) -> io::Result<(PathBuf, File)> {
    let parent = parent_dir(path);
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "ledger snapshot path must name a file",
        )
    })?;
    let file_name = file_name.to_string_lossy();

    for _ in 0..16 {
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = parent.join(format!(
            ".{file_name}.tmp.{}.{}",
            std::process::id(),
            counter
        ));
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => return Ok((tmp, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique ledger snapshot file",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn observation(state: WindowState, start: u64) -> WindowObservation {
        observation_for("billing-worker", TrafficDirection::Unknown, state, start)
    }

    fn observation_for(
        service_id: &str,
        direction: TrafficDirection,
        state: WindowState,
        start: u64,
    ) -> WindowObservation {
        WindowObservation {
            key: WindowKey {
                window_start_secs: start,
                service_id: service_id.into(),
                window_seconds: 60,
                direction,
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
            direction: TrafficDirection::Unknown,
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
    /// Equal-timestamp eviction remains replay-safe after a restart.
    #[test]
    fn restart_after_equal_timestamp_eviction_rejects_replay() {
        let p = std::env::temp_dir().join(format!(
            "ledger_equal_timestamp_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut ledger = WindowLedger::new(2).unwrap();
        let evicted = observation_for(
            "a-service",
            TrafficDirection::Ingress,
            WindowState::Observed {
                packets: 1,
                bytes: 1,
            },
            60,
        );
        let retained = observation_for(
            "b-service",
            TrafficDirection::Ingress,
            WindowState::Observed {
                packets: 2,
                bytes: 2,
            },
            60,
        );
        let incoming = observation_for(
            "c-service",
            TrafficDirection::Ingress,
            WindowState::Observed {
                packets: 3,
                bytes: 3,
            },
            120,
        );
        assert_eq!(
            ledger.record(evicted.clone()).unwrap(),
            RecordOutcome::Inserted
        );
        assert_eq!(
            ledger.record(retained.clone()).unwrap(),
            RecordOutcome::Inserted
        );
        assert_eq!(
            ledger.record(incoming).unwrap(),
            RecordOutcome::InsertedAfterEviction
        );
        assert!(ledger.get(&evicted.key).is_none());
        assert_eq!(ledger.get(&retained.key), Some(&retained.state));

        ledger.flush(&p).unwrap();
        let mut recovered = WindowLedger::reopen(&p, 2).unwrap();
        assert_eq!(
            recovered.record(evicted).unwrap(),
            RecordOutcome::RejectedTooOld
        );
        assert_eq!(recovered.get(&retained.key), Some(&retained.state));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn ingress_and_egress_are_distinct_window_identities() {
        let mut ledger = WindowLedger::new(2).unwrap();
        let ingress = observation_for(
            "billing-worker",
            TrafficDirection::Ingress,
            WindowState::Observed {
                packets: 1,
                bytes: 10,
            },
            60,
        );
        let egress = observation_for(
            "billing-worker",
            TrafficDirection::Egress,
            WindowState::Observed {
                packets: 2,
                bytes: 20,
            },
            60,
        );
        assert_eq!(ledger.record(ingress).unwrap(), RecordOutcome::Inserted);
        assert_eq!(ledger.record(egress).unwrap(), RecordOutcome::Inserted);
        assert_eq!(ledger.len(), 2);
    }

    #[test]
    fn relative_snapshot_path_is_supported() {
        let p = PathBuf::from(format!(
            "ledger_relative_{}_{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut ledger = WindowLedger::new(2).unwrap();
        let sample = observation(
            WindowState::Observed {
                packets: 7,
                bytes: 70,
            },
            0,
        );
        assert_eq!(ledger.record(sample).unwrap(), RecordOutcome::Inserted);
        ledger.flush(&p).unwrap();
        assert_eq!(WindowLedger::reopen(&p, 2).unwrap().len(), 1);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn failed_rename_removes_temporary_snapshot() {
        let dir = std::env::temp_dir().join(format!(
            "ledger_temp_cleanup_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("snapshot.json");
        fs::create_dir(&target).unwrap();
        let ledger = WindowLedger::new(2).unwrap();
        assert!(ledger.flush(&target).is_err());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reopen_rejects_retained_entry_at_or_below_watermark() {
        let p = std::env::temp_dir().join(format!(
            "ledger_bad_watermark_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let retained = observation(
            WindowState::Observed {
                packets: 1,
                bytes: 1,
            },
            60,
        );
        let payload = PersistenceEnvelope {
            version: PERSISTENCE_VERSION,
            entries: vec![(retained.key.clone(), retained.state)],
            eviction_watermark: Some(retained.key),
        };
        fs::write(&p, serde_json::to_vec(&payload).unwrap()).unwrap();
        assert!(matches!(
            WindowLedger::reopen(&p, 2),
            Err(WindowError::Io(_))
        ));
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn reopen_rejects_snapshot_larger_than_configured_bound() {
        let p = std::env::temp_dir().join(format!(
            "ledger_oversized_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let oversized = vec![b' '; max_snapshot_bytes(1) as usize + 1];
        fs::write(&p, oversized).unwrap();
        assert!(matches!(
            WindowLedger::reopen(&p, 1),
            Err(WindowError::Io(_))
        ));
        let _ = fs::remove_file(&p);
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
        bad.insert("eviction_watermark".into(), serde_json::Value::Null);
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
            direction: TrafficDirection::Unknown,
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
            "version": 2,
            "entries": [
                (&first.key, &first.state),
                (&second.key, &second.state),
            ],
            "eviction_watermark": null
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
        let json = r#"{"version":2,"entries":[[{"window_start_secs":0,"service_id":"","window_seconds":60,"direction":"Unknown"},{"state":"observed","packets":1,"bytes":1}]],"eviction_watermark":null}"#;
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
                    direction: TrafficDirection::Unknown,
                },
                state: WindowState::Observed {
                    packets: 1,
                    bytes: 1,
                },
            })
            .collect();
        let payload = serde_json::json!({
            "version": 2,
            "entries": entries.iter().map(|e| (&e.key, &e.state)).collect::<Vec<_>>(),
            "eviction_watermark": null
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
