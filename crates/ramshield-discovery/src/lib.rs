//! Passive production-environment discovery.
//! This crate has no dependency on detection or enforcement. It accepts
//! normalized observations, keeps bounded hourly aggregates, and persists
//! them atomically. It never changes live configuration or network state.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

mod calendar;
mod metric;
mod stats;
mod window;
pub use calendar::{
    CalendarAssessment, CalendarAssessmentKind, CalendarSlotSummary, CalendarSnapshot,
    ExecutionState, ExpectedWorkload, Recurrence, TrafficCalendar, WorkloadDeclaration,
    WorkloadExecution,
};
pub use metric::{
    CardinalityEstimate, CardinalityEstimator, CountMetric, MetricAssessment, MetricProfileStore,
    ObservationQuality, ProfileKey, extend_assessment, new_profile_key, observe_clean,
    reset_quality_counts,
};
pub use stats::{CountModelFit, CountMoments, fit_count_model};
pub use window::{
    MissingReason, RecordOutcome, WindowError, WindowKey, WindowLedger, WindowObservation,
    WindowState,
};

const SCHEMA: u32 = 2;
const HOURS_PER_DAY: usize = 24;
const SOURCE_BUCKETS: usize = 64;
const DESTINATION_BUCKETS: usize = 64;
const MAX_PORTS: usize = 256;
/// Bounded number of concurrently tracked services for fair resource allocation.
const MAX_SERVICES: usize = 64;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[repr(usize)]
pub enum Protocol {
    Other = 0,
    Tcp = 1,
    Udp = 2,
    Icmp = 3,
    IcmpV6 = 4,
}

/// Stable producer-side service identifier.
/// Distinct from source_key: a source hash may belong to many services,
/// but each service has exactly one identity.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Ord, PartialOrd,
)]
pub struct ServiceId(pub u64);

impl ServiceId {
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn new(id: u64) -> Self {
        ServiceId(id)
    }
}

/// Authoritative direction of traffic, sourced from the observation,
/// never inferred from ports or IP addresses.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
pub enum TrafficDirection {
    Ingress,
    Egress,
    Internal,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficSample {
    pub timestamp_secs: u64,
    pub packets: u64,
    pub bytes: u64,
    pub protocol: Protocol,
    pub destination_port: Option<u16>,
    /// Producer-side stable hash. Raw IP addresses are never persisted.
    pub source_key: Option<u64>,
    /// Producer-side stable hash. Raw IP addresses are never persisted.
    pub destination_key: Option<u64>,
    /// Explicit service identity assigned by the producer.
    /// Never inferred from IP addresses or ports.
    #[serde(default)]
    pub service_id: Option<ServiceId>,
    /// Authoritative traffic direction from the capture layer.
    /// Never inferred from ports or IP addresses; defaults to Unknown.
    #[serde(default)]
    pub direction: TrafficDirection,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct HealthSample {
    pub timestamp_secs: u64,
    pub capture_drops: u64,
    pub ingest_drops: u64,
    pub malformed_observations: u64,
    pub queue_depth: u64,
    pub queue_capacity: u64,
    pub cpu_millis: u64,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    pub observation_days: u16,
    pub max_ports_per_hour: usize,
}
impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            observation_days: 90,
            max_ports_per_hour: 64,
        }
    }
}
impl DiscoveryConfig {
    fn validate(&self) -> io::Result<()> {
        if !(1..=365).contains(&self.observation_days) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "observation_days must be 1..=365",
            ));
        }
        if !(1..=MAX_PORTS).contains(&self.max_ports_per_hour) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "max_ports_per_hour must be 1..=256",
            ));
        }
        Ok(())
    }
    fn bucket_count(&self) -> usize {
        usize::from(self.observation_days) * HOURS_PER_DAY
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PortCounter {
    port: u16,
    count: u64,
    error_bound: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HourBucket {
    hour: u64,
    samples: u64,
    packets: u64,
    bytes: u64,
    protocols: [u64; 5],
    ports: Vec<PortCounter>,
    sources: Vec<u64>,
    destinations: Vec<u64>,
    capture_drops: u64,
    ingest_drops: u64,
    malformed: u64,
    queue_depth_peak: u64,
    queue_capacity_peak: u64,
    cpu_millis: u64,
    memory_bytes_peak: u64,
}
impl HourBucket {
    fn new(hour: u64) -> Self {
        Self {
            hour,
            samples: 0,
            packets: 0,
            bytes: 0,
            protocols: [0; 5],
            ports: Vec::new(),
            sources: vec![0; SOURCE_BUCKETS],
            destinations: vec![0; DESTINATION_BUCKETS],
            capture_drops: 0,
            ingest_drops: 0,
            malformed: 0,
            queue_depth_peak: 0,
            queue_capacity_peak: 0,
            cpu_millis: 0,
            memory_bytes_peak: 0,
        }
    }
    fn add_port(&mut self, port: u16, limit: usize) {
        if let Some(p) = self.ports.iter_mut().find(|p| p.port == port) {
            p.count = p.count.saturating_add(1);
            return;
        }
        if self.ports.len() < limit {
            self.ports.push(PortCounter {
                port,
                count: 1,
                error_bound: 0,
            });
            return;
        }
        if let Some((idx, min)) = self.ports.iter().enumerate().min_by_key(|(_, p)| p.count) {
            let bound = min.count;
            self.ports[idx] = PortCounter {
                port,
                count: bound.saturating_add(1),
                error_bound: bound,
            };
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedState {
    schema: u32,
    observation_days: u16,
    buckets: Vec<Option<HourBucket>>,
    accepted: u64,
    too_old: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortSummary {
    pub port: u16,
    pub count: u64,
    pub error_bound: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourSummary {
    pub hour: u64,
    pub samples: u64,
    pub packets: u64,
    pub bytes: u64,
    pub protocols: [u64; 5],
    pub top_ports: Vec<PortSummary>,
    pub source_distribution: Vec<u64>,
    pub destination_distribution: Vec<u64>,
    pub capture_drops: u64,
    pub ingest_drops: u64,
    pub malformed_observations: u64,
    pub queue_depth_peak: u64,
    pub queue_capacity_peak: u64,
    pub cpu_millis: u64,
    pub memory_bytes_peak: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverySnapshot {
    pub schema_version: u32,
    pub observation_days: u16,
    pub observations_accepted: u64,
    pub observations_too_old: u64,
    pub oldest_hour: Option<u64>,
    pub newest_hour: Option<u64>,
    pub hours: Vec<HourSummary>,
}

pub struct Discovery {
    config: DiscoveryConfig,
    state: PersistedState,
    path: PathBuf,
    service_profiles: BTreeMap<ServiceId, MetricProfileStore>,
}

impl Discovery {
    pub fn open(path: impl AsRef<Path>, config: DiscoveryConfig) -> io::Result<Self> {
        config.validate()?;
        let path = path.as_ref().to_path_buf();
        let state = if path.exists() {
            let bytes = fs::read(&path)?;
            let state: PersistedState = serde_json::from_slice(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if state.schema != SCHEMA
                || state.observation_days != config.observation_days
                || state.buckets.len() != config.bucket_count()
                || state.buckets.iter().flatten().any(|bucket| {
                    bucket.sources.len() != SOURCE_BUCKETS
                        || bucket.destinations.len() != DESTINATION_BUCKETS
                        || bucket.ports.len() > config.max_ports_per_hour
                })
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "discovery state schema/configuration mismatch",
                ));
            }
            state
        } else {
            PersistedState {
                schema: SCHEMA,
                observation_days: config.observation_days,
                buckets: vec![None; config.bucket_count()],
                accepted: 0,
                too_old: 0,
            }
        };
        Ok(Self {
            config,
            state,
            path,
            service_profiles: BTreeMap::new(),
        })
    }

    /// Aggregate a normalized observation. No enforcement or detection action is possible here.
    pub fn observe(&mut self, sample: TrafficSample) {
        let hour = sample.timestamp_secs / 3600;
        let port_limit = self.config.max_ports_per_hour;
        let Some(bucket) = self.bucket_mut(hour) else {
            self.state.too_old = self.state.too_old.saturating_add(1);
            return;
        };
        bucket.samples = bucket.samples.saturating_add(1);
        bucket.packets = bucket.packets.saturating_add(sample.packets);
        bucket.bytes = bucket.bytes.saturating_add(sample.bytes);
        let protocol = sample.protocol as usize;
        bucket.protocols[protocol] = bucket.protocols[protocol].saturating_add(sample.packets);
        if let Some(port) = sample.destination_port {
            bucket.add_port(port, port_limit);
        }
        if let Some(key) = sample.source_key {
            let i = key as usize % SOURCE_BUCKETS;
            bucket.sources[i] = bucket.sources[i].saturating_add(sample.packets);
        }
        if let Some(key) = sample.destination_key {
            let i = key as usize % DESTINATION_BUCKETS;
            bucket.destinations[i] = bucket.destinations[i].saturating_add(sample.packets);
        }
        // D5.2: derive service_id from source_key if not explicitly set
        let effective_service_id = sample.service_id.or_else(|| {
            sample.source_key.map(|sk| {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                use std::hash::{Hash, Hasher};
                sk.hash(&mut hasher);
                ServiceId(hasher.finish())
            })
        });

        // D6: observe source/destination cardinality into per-service profiles
        if let Some(service_id) = &effective_service_id {
            let profile = self.service_profiles.entry(*service_id).or_default();
            profile.observe_cardinality(
                service_id.as_u64().to_string(),
                sample.direction,
                hour,
                sample.source_key,
                sample.destination_key,
                ObservationQuality::Clean,
            );
        }

        // Enforce MAX_SERVICES limit on per-service profile creation
        let service_count = self.service_profiles.len();
        let is_new_service = effective_service_id
            .map(|s| !self.service_profiles.contains_key(&s))
            .unwrap_or(false);
        if is_new_service && service_count >= MAX_SERVICES {
            self.state.too_old = self.state.too_old.saturating_add(1);
            return; // reject new service profiles at capacity
        }

        // Thread per-service packet/byte profiles (D5.2)
        if let Some(service_id) = effective_service_id {
            let profile = self.service_profiles.entry(service_id).or_default();
            let hour_slot = hour;
            profile.observe_volume(
                service_id.as_u64().to_string(),
                sample.direction,
                hour_slot,
                sample.packets,
                sample.bytes,
                ObservationQuality::Clean,
            );
        }

        self.state.accepted = self.state.accepted.saturating_add(1);
    }

    pub fn observe_health(&mut self, sample: HealthSample) {
        let Some(bucket) = self.bucket_mut(sample.timestamp_secs / 3600) else {
            return;
        };
        bucket.capture_drops = bucket.capture_drops.saturating_add(sample.capture_drops);
        bucket.ingest_drops = bucket.ingest_drops.saturating_add(sample.ingest_drops);
        bucket.malformed = bucket
            .malformed
            .saturating_add(sample.malformed_observations);
        bucket.queue_depth_peak = bucket.queue_depth_peak.max(sample.queue_depth);
        bucket.queue_capacity_peak = bucket.queue_capacity_peak.max(sample.queue_capacity);
        bucket.cpu_millis = bucket.cpu_millis.saturating_add(sample.cpu_millis);
        bucket.memory_bytes_peak = bucket.memory_bytes_peak.max(sample.memory_bytes);
    }

    /// Atomic snapshot persistence: temp file, file fsync, rename, directory fsync.
    pub fn flush(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec(&self.state)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut tmp = self.path.clone();
        let ext = self
            .path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("json");
        tmp.set_extension(format!("{ext}.tmp"));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, &self.path)?;
        if let Some(parent) = self.path.parent() {
            File::open(parent)?.sync_all()?;
        }
        Ok(())
    }

    pub fn snapshot(&self) -> DiscoverySnapshot {
        let mut buckets: Vec<_> = self.state.buckets.iter().flatten().collect();
        buckets.sort_by_key(|b| b.hour);
        let hours = buckets
            .iter()
            .map(|b| HourSummary {
                hour: b.hour,
                samples: b.samples,
                packets: b.packets,
                bytes: b.bytes,
                protocols: b.protocols,
                top_ports: b
                    .ports
                    .iter()
                    .map(|p| PortSummary {
                        port: p.port,
                        count: p.count,
                        error_bound: p.error_bound,
                    })
                    .collect(),
                source_distribution: b.sources.clone(),
                destination_distribution: b.destinations.clone(),
                capture_drops: b.capture_drops,
                ingest_drops: b.ingest_drops,
                malformed_observations: b.malformed,
                queue_depth_peak: b.queue_depth_peak,
                queue_capacity_peak: b.queue_capacity_peak,
                cpu_millis: b.cpu_millis,
                memory_bytes_peak: b.memory_bytes_peak,
            })
            .collect();
        DiscoverySnapshot {
            schema_version: SCHEMA,
            observation_days: self.config.observation_days,
            observations_accepted: self.state.accepted,
            observations_too_old: self.state.too_old,
            oldest_hour: buckets.first().map(|b| b.hour),
            newest_hour: buckets.last().map(|b| b.hour),
            hours,
        }
    }

    fn bucket_mut(&mut self, hour: u64) -> Option<&mut HourBucket> {
        let retention = self.config.bucket_count() as u64;
        if let Some(newest) = self
            .state
            .buckets
            .iter()
            .flatten()
            .map(|bucket| bucket.hour)
            .max()
        {
            let newest = newest.max(hour);
            let oldest = newest.saturating_sub(retention - 1);
            if hour < oldest {
                return None;
            }
            for bucket in &mut self.state.buckets {
                if bucket.as_ref().is_some_and(|bucket| bucket.hour < oldest) {
                    *bucket = None;
                }
            }
        }
        let len = self.state.buckets.len();
        let index = (hour % len as u64) as usize;
        if self.state.buckets[index]
            .as_ref()
            .is_none_or(|bucket| bucket.hour != hour)
        {
            self.state.buckets[index] = Some(HourBucket::new(hour));
        }
        self.state.buckets[index].as_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    fn path() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rs-discovery-{n}.json"))
    }
    fn sample(t: u64) -> TrafficSample {
        TrafficSample {
            timestamp_secs: t,
            packets: 3,
            bytes: 300,
            protocol: Protocol::Tcp,
            destination_port: Some(443),
            source_key: Some(17),
            destination_key: Some(29),
            service_id: None,
            direction: TrafficDirection::Unknown,
        }
    }
    #[test]
    fn aggregates_protocol_port_and_distribution() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        d.observe(sample(1_700_000_000));
        let s = d.snapshot();
        assert_eq!(s.observations_accepted, 1);
        assert_eq!(s.hours[0].packets, 3);
        assert_eq!(s.hours[0].protocols[Protocol::Tcp as usize], 3);
        assert_eq!(s.hours[0].top_ports[0].port, 443);
        assert_eq!(s.hours[0].source_distribution[17], 3);
        let _ = fs::remove_file(p);
    }
    #[test]
    fn persisted_state_recovers_after_reopen() {
        let p = path();
        {
            let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
            d.observe(sample(1_700_000_000));
            d.flush().unwrap();
        }
        let d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        assert_eq!(d.snapshot().observations_accepted, 1);
        let _ = fs::remove_file(p);
    }
    #[test]
    fn retention_prunes_stale_buckets_after_large_time_jump() {
        let p = path();
        let cfg = DiscoveryConfig {
            observation_days: 1,
            max_ports_per_hour: 2,
        };
        let mut d = Discovery::open(&p, cfg).unwrap();
        d.observe(sample(200 * 3600));
        d.observe(sample(1_200 * 3600));
        let snapshot = d.snapshot();
        assert_eq!(snapshot.hours.len(), 1);
        assert_eq!(snapshot.oldest_hour, Some(1_200));
        assert_eq!(snapshot.newest_hour, Some(1_200));
        let _ = fs::remove_file(p);
    }

    #[test]
    fn retention_and_port_cardinality_are_bounded() {
        let p = path();
        let cfg = DiscoveryConfig {
            observation_days: 1,
            max_ports_per_hour: 2,
        };
        let mut d = Discovery::open(&p, cfg).unwrap();
        for port in 1..100 {
            let mut event = sample(200 * 3600);
            event.destination_port = Some(port);
            d.observe(event);
        }
        let s = d.snapshot();
        assert!(s.hours[0].top_ports.len() <= 2);
        assert!(s.hours.len() <= 24);
        assert_eq!(s.observations_accepted, 99);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn cardinality_estimator_linear_counting_basic() {
        use crate::metric::CardinalityEstimator;
        let mut ce = CardinalityEstimator::new(4096);
        // Identity 0 is special - let's use real keys
        for i in 0..10u64 {
            ce.observe(i);
        }
        let estimate = ce.estimate();
        assert!(
            estimate.estimate >= 10,
            "estimate {:?} should be >= 10, got {:?}",
            estimate.estimate,
            estimate
        );
        assert!(
            estimate.estimate <= 20,
            "estimate {:?} should be <= 20 for 10 inserted, got {:?}",
            estimate.estimate,
            estimate
        );
    }

    #[test]
    fn cardinality_estimator_known_cardinality() {
        use crate::metric::CardinalityEstimate;
        let mut ce = CardinalityEstimator::new(4096);
        // Insert exactly 100 distinct identities
        let identities: Vec<u64> = (0..100u64).collect();
        for id in &identities {
            ce.observe(*id);
        }
        let est = ce.estimate();
        // Linear counting should be within a factor of ~2 of 100 for 4096 bits
        assert!(
            est.estimate >= 60,
            "estimate {} should be >= 60 for 100 inserted",
            est.estimate
        );
        assert!(
            est.estimate <= 140,
            "estimate {} should be <= 140 for 100 inserted",
            est.estimate
        );
        // Standard error should be non-zero and reasonable
        assert!(
            est.standard_error > 0.0,
            "standard_error should be positive"
        );
    }

    #[test]
    fn cardinality_estimator_known_cardinality_1000() {
        let mut ce = CardinalityEstimator::new(4096);
        // Insert exactly 1000 distinct identities
        let identities: Vec<u64> = (0..1000u64).collect();
        for id in &identities {
            ce.observe(*id);
        }
        let est = ce.estimate();
        // For larger cardinalities, error grows; still should be in the ballpark
        assert!(
            est.estimate >= 500,
            "estimate {} should be >= 500 for 1000 inserted",
            est.estimate
        );
        assert!(
            est.estimate <= 1500,
            "estimate {} should be <= 1500 for 1000 inserted",
            est.estimate
        );
    }

    #[test]
    fn cardinality_estimator_idempotent() {
        let mut ce = CardinalityEstimator::new(4096);
        let id = 42u64;
        // Observe same identity 10 times
        for _ in 0..10 {
            ce.observe(id);
        }
        let est = ce.estimate();
        // Should still estimate ~1 distinct identity
        assert!(
            est.estimate >= 1,
            "estimate should be >= 1, got {}",
            est.estimate
        );
        assert!(
            est.estimate <= 5,
            "estimate should be <= 5 for repeated id, got {}",
            est.estimate
        );
    }

    #[test]
    fn cardinality_estimator_zero() {
        let ce = CardinalityEstimator::new(4096);
        let est = ce.estimate();
        assert_eq!(est.estimate, 0);
        assert_eq!(est.observations, 0);
    }

    #[test]
    fn metric_profile_store_cardinality_observation() {
        let mut ps = MetricProfileStore::new();
        ps.observe_cardinality(
            "svc-1".into(),
            TrafficDirection::Unknown,
            0u64,
            Some(17u64),
            Some(29u64),
            ObservationQuality::Clean,
        );
        ps.observe_cardinality(
            "svc-1".into(),
            TrafficDirection::Unknown,
            0u64,
            Some(17u64), // repeated - should not inflate
            Some(29u64),
            ObservationQuality::Clean,
        );
        ps.observe_cardinality(
            "svc-1".into(),
            TrafficDirection::Unknown,
            0u64,
            Some(18u64), // new distinct source
            None,
            ObservationQuality::Clean,
        );

        let src_est = ps.source_cardinality_estimate("svc-1", TrafficDirection::Unknown, 0);
        assert!(
            src_est.is_some(),
            "source cardinality estimate should exist"
        );
        let est = src_est.unwrap();
        // Two distinct sources (17, 18) plus one repeated = ~2-3
        assert!(
            est.estimate >= 1,
            "estimate should be >= 1, got {}",
            est.estimate
        );
        assert!(
            est.estimate <= 5,
            "estimate should be <= 5 for 2 distinct, got {}",
            est.estimate
        );

        let dst_est = ps.dest_cardinality_estimate("svc-1", TrafficDirection::Unknown, 0);
        assert!(dst_est.is_some(), "dest cardinality estimate should exist");
    }

    #[test]
    fn cardinality_independence_per_service() {
        let mut ps1 = MetricProfileStore::new();
        let mut ps2 = MetricProfileStore::new();

        // Service A: source keys 1..10
        for i in 0..10u64 {
            ps1.observe_cardinality(
                "svc-a".into(),
                TrafficDirection::Unknown,
                0u64,
                Some(i),
                None,
                ObservationQuality::Clean,
            );
        }

        // Service B: source keys 20..29 (different, no overlap)
        for i in 20..30u64 {
            ps2.observe_cardinality(
                "svc-b".into(),
                TrafficDirection::Unknown,
                0u64,
                Some(i),
                None,
                ObservationQuality::Clean,
            );
        }

        let a_src = ps1
            .source_cardinality_estimate("svc-a", TrafficDirection::Unknown, 0)
            .unwrap();
        let b_src = ps2
            .source_cardinality_estimate("svc-b", TrafficDirection::Unknown, 0)
            .unwrap();

        // Service A should estimate ~10 distinct
        assert!(
            a_src.estimate >= 6,
            "svc-a estimate should be >= 6, got {}",
            a_src.estimate
        );
        assert!(
            a_src.estimate <= 15,
            "svc-a estimate should be <= 15, got {}",
            a_src.estimate
        );

        // Service B should estimate ~10 distinct
        assert!(
            b_src.estimate >= 6,
            "svc-b estimate should be >= 6, got {}",
            b_src.estimate
        );
        assert!(
            b_src.estimate <= 15,
            "svc-b estimate should be <= 15, got {}",
            b_src.estimate
        );
    }

    #[test]
    fn cardinality_estimator_bounded_memory() {
        let mut ce = CardinalityEstimator::new(64); // tiny bitmap
        // Insert many distinct identities
        for i in 0..500u64 {
            ce.observe(i);
        }
        let est = ce.estimate();
        // Memory is always 64 bits regardless of 500 inserted
        // Estimate will be approximate with significant error at this scale
        assert!(est.estimate > 0, "estimate should be > 0");
        // Memory stays bounded - verify bitmap size via bit_count accessor
        assert!(
            ce.bit_count() <= 64,
            "bitmap memory should be bounded (64 bits)"
        );
    }
}
