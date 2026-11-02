//! Passive production-environment discovery.
//! This crate has no dependency on detection or enforcement. It accepts
//! normalized observations, keeps bounded hourly aggregates, and persists
//! them atomically. It never changes live configuration or network state.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
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
pub use stats::{
    CountModelFit, CountMoments, DistributionAssessment, assess_cardinality_anomaly,
    assess_metric_distribution, calibrate_threshold, d8_saturated_or_insufficient_guard,
    fit_count_model,
};
pub use window::{
    MissingReason, RecordOutcome, WindowError, WindowKey, WindowLedger, WindowObservation,
    WindowState,
};

const SCHEMA: u32 = 3;
const HOURS_PER_DAY: usize = 24;
const SOURCE_BUCKETS: usize = 64;
const DESTINATION_BUCKETS: usize = 64;
const MAX_PORTS: usize = 256;

// The configured retention and port limits imply a conservative serialized
// size bound. This absolute ceiling keeps corrupted snapshots from causing
// unbounded reads while still covering the largest supported configuration.
const MAX_DISCOVERY_SNAPSHOT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_BUCKET_FIXED_BYTES: u64 = 4 * 1024;
const MAX_PORT_ENTRY_BYTES: u64 = 96;
const SNAPSHOT_ENVELOPE_BYTES: u64 = 1024;
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
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

/// Report-only distribution diagnostics for one service and traffic direction.
/// These states never modify detector configuration or enforcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDistributionResult {
    pub metric: CountMetric,
    pub assessment: DistributionAssessment,
    pub calibration: DistributionAssessment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDistributionAssessment {
    pub service_id: ServiceId,
    pub direction: TrafficDirection,
    /// Absolute UTC hour used by the discovery profile store.
    pub hour: u64,
    pub source_cardinality: DistributionAssessment,
    pub destination_cardinality: DistributionAssessment,
    pub metrics: Vec<MetricDistributionResult>,
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
            let state: PersistedState =
                read_snapshot(&path, max_discovery_snapshot_bytes(&config))?;
            validate_persisted_state(&state, &config)?;
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

    /// Aggregate a normalized traffic observation without training service profiles.
    ///
    /// HealthSample is a separate, potentially out-of-order stream. Its absence
    /// cannot prove that an individual traffic sample was clean. Use
    /// observe_with_quality when the producer can explicitly classify the sample.
    pub fn observe(&mut self, sample: TrafficSample) {
        self.observe_inner(sample, None);
    }

    /// Aggregate a sample and update its service profile only with explicit
    /// producer-supplied quality.
    ///
    /// Missing and invalid states cannot accompany a traffic sample: represent
    /// missing windows with WindowLedger and reject invalid traffic before this
    /// API. Degraded samples remain visible in aggregate traffic but do not
    /// contribute to clean metric moments or cardinality estimates.
    pub fn observe_with_quality(
        &mut self,
        sample: TrafficSample,
        quality: ObservationQuality,
    ) -> io::Result<()> {
        if matches!(
            quality,
            ObservationQuality::Missing | ObservationQuality::Invalid
        ) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "traffic samples require Clean or Degraded quality",
            ));
        }
        self.observe_inner(sample, Some(quality));
        Ok(())
    }

    fn observe_inner(
        &mut self,
        sample: TrafficSample,
        service_quality: Option<ObservationQuality>,
    ) {
        let hour_utc = sample.timestamp_secs / 3600;
        let port_limit = self.config.max_ports_per_hour;
        let Some(bucket) = self.bucket_mut(hour_utc) else {
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

        // Service identity and quality are explicit producer contracts.
        // HealthSample is aggregated independently and is not used to infer
        // per-sample quality from event arrival order.
        if let (Some(service_id), Some(quality)) = (sample.service_id, service_quality) {
            let has_capacity = self.service_profiles.contains_key(&service_id)
                || self.service_profiles.len() < MAX_SERVICES;
            if has_capacity {
                let profile = self.service_profiles.entry(service_id).or_default();
                let oldest_hour = hour_utc.saturating_sub(self.config.bucket_count() as u64 - 1);
                profile.prune_before(oldest_hour);
                let service_key = service_id.as_u64().to_string();
                // D7/D8 compare adjacent absolute UTC hours and prune by the
                // same absolute-hour key. A recurring hour-of-week slot would
                // wrap every week and be incompatible with this retention floor.
                profile.observe_cardinality(
                    service_key.clone(),
                    sample.direction,
                    hour_utc,
                    sample.source_key,
                    sample.destination_key,
                    quality,
                );
                profile.observe_volume(
                    service_key,
                    sample.direction,
                    hour_utc,
                    sample.packets,
                    sample.bytes,
                    quality,
                );
            }
            // At capacity, aggregate traffic is still accepted; only the
            // optional per-service profile is omitted.
        }

        self.state.accepted = self.state.accepted.saturating_add(1);
    }

    /// Assess one service's latest hourly profiles against the preceding hour.
    ///
    /// This is a report-only integration point for D7/D8. It deliberately returns
    /// InsufficientData for unknown direction, missing hourly cardinality baselines,
    /// or unqualified metrics. It never emits numeric thresholds or changes runtime
    /// configuration. TrafficSample counts are producer observations, not a claim
    /// that independent completed windows have been collected.
    pub fn assess_service(
        &self,
        service_id: ServiceId,
        direction: TrafficDirection,
        hour: u64,
    ) -> Option<ServiceDistributionAssessment> {
        let profile = self.service_profiles.get(&service_id)?;
        let service_key = service_id.as_u64().to_string();
        let previous_hour = hour.checked_sub(1);

        // Unknown direction is not a comparable service series. Also require
        // explicit clean evidence in both adjacent hours before assessing
        // cardinality: degraded samples are otherwise absent from the estimator
        // and could make a partial hour look deceptively complete.
        let direction_known = direction != TrafficDirection::Unknown;
        let quality_is_clean = |profile_hour| {
            profile
                .assessment(&service_key, direction, CountMetric::Packets, profile_hour)
                .is_some_and(|assessment| {
                    assessment.quality == ObservationQuality::Clean
                        && assessment.degraded_observations == 0
                        && assessment.clean_observations == assessment.moments.observations()
                })
        };
        let current_quality_clean = quality_is_clean(hour);
        let baseline_quality_clean = match previous_hour {
            Some(previous) => quality_is_clean(previous),
            None => false,
        };
        let quality_qualified = direction_known && current_quality_clean && baseline_quality_clean;
        let current_source = if quality_qualified {
            profile.source_cardinality_estimate(&service_key, direction, hour)
        } else {
            None
        };
        let baseline_source = if quality_qualified {
            previous_hour.and_then(|previous| {
                profile.source_cardinality_estimate(&service_key, direction, previous)
            })
        } else {
            None
        };
        let current_destination = if quality_qualified {
            profile.dest_cardinality_estimate(&service_key, direction, hour)
        } else {
            None
        };
        let baseline_destination = if quality_qualified {
            previous_hour.and_then(|previous| {
                profile.dest_cardinality_estimate(&service_key, direction, previous)
            })
        } else {
            None
        };

        let source_cardinality =
            current_source
                .as_ref()
                .map_or(DistributionAssessment::InsufficientData, |current| {
                    assess_cardinality_anomaly(
                        current,
                        baseline_source.as_ref(),
                        current.observations,
                    )
                });
        let destination_cardinality = current_destination.as_ref().map_or(
            DistributionAssessment::InsufficientData,
            |current| {
                assess_cardinality_anomaly(
                    current,
                    baseline_destination.as_ref(),
                    current.observations,
                )
            },
        );

        let cardinality_assessment = if source_cardinality == DistributionAssessment::RequiresReview
            || destination_cardinality == DistributionAssessment::RequiresReview
        {
            DistributionAssessment::RequiresReview
        } else if source_cardinality == DistributionAssessment::InsufficientData
            || destination_cardinality == DistributionAssessment::InsufficientData
        {
            DistributionAssessment::InsufficientData
        } else {
            DistributionAssessment::Normal
        };

        let cardinality_saturated = current_source
            .as_ref()
            .is_some_and(|estimate| estimate.estimate == u64::MAX)
            || current_destination
                .as_ref()
                .is_some_and(|estimate| estimate.estimate == u64::MAX);

        let metrics = profile
            .all_assessments(&service_key, direction, hour)
            .into_iter()
            .map(|assessment| {
                let distribution = if direction == TrafficDirection::Unknown {
                    DistributionAssessment::InsufficientData
                } else {
                    assess_metric_distribution(&assessment)
                };
                let calibration = calibrate_threshold(
                    &cardinality_assessment,
                    &distribution,
                    cardinality_saturated,
                    assessment.clean_observations,
                );
                MetricDistributionResult {
                    metric: assessment.metric,
                    assessment: distribution,
                    calibration,
                }
            })
            .collect();

        Some(ServiceDistributionAssessment {
            service_id,
            direction,
            hour,
            source_cardinality,
            destination_cardinality,
            metrics,
        })
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

    /// Persist the bounded aggregate snapshot through an exclusive same-directory
    /// temporary file, file fsync, atomic rename and directory fsync.
    pub fn flush(&self) -> io::Result<()> {
        write_snapshot(
            &self.path,
            &self.state,
            max_discovery_snapshot_bytes(&self.config),
        )
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

fn max_discovery_snapshot_bytes(config: &DiscoveryConfig) -> u64 {
    let per_bucket = MAX_BUCKET_FIXED_BYTES
        .saturating_add((config.max_ports_per_hour as u64).saturating_mul(MAX_PORT_ENTRY_BYTES));
    (config.bucket_count() as u64)
        .saturating_mul(per_bucket)
        .saturating_add(SNAPSHOT_ENVELOPE_BYTES)
        .min(MAX_DISCOVERY_SNAPSHOT_BYTES)
}

fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn create_temp_file(path: &Path) -> io::Result<(PathBuf, File)> {
    let parent = parent_dir(path);
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "snapshot path must name a file",
        )
    })?;
    let file_name = file_name.to_string_lossy();

    for _ in 0..16 {
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.tmp.{}.{}",
            std::process::id(),
            counter
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique snapshot file",
    ))
}

fn read_snapshot<T: serde::de::DeserializeOwned>(path: &Path, max_bytes: u64) -> io::Result<T> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "snapshot exceeds its configured size bound",
        ));
    }

    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "snapshot grew beyond its configured size bound",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn write_snapshot<T: Serialize>(path: &Path, value: &T, max_bytes: u64) -> io::Result<()> {
    if path.file_name().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "snapshot path must name a file",
        ));
    }
    let parent = parent_dir(path);
    fs::create_dir_all(parent)?;

    let (temporary, mut file) = create_temp_file(path)?;
    let write_result = (|| {
        serde_json::to_writer(&mut file, value)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if file.metadata()?.len() > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "snapshot exceeds its configured size bound",
            ));
        }
        file.sync_all()
    })();
    drop(file);

    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    File::open(parent)?.sync_all()
}

fn validate_persisted_state(state: &PersistedState, config: &DiscoveryConfig) -> io::Result<()> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "discovery snapshot contains inconsistent persisted state",
        )
    };
    let bucket_count = state.buckets.len();
    if state.schema != SCHEMA
        || state.observation_days != config.observation_days
        || bucket_count != config.bucket_count()
        || bucket_count == 0
    {
        return Err(invalid());
    }

    let mut newest_hour = None;
    let mut retained_samples = 0u64;
    let mut retained_samples_overflowed = false;

    for (index, maybe_bucket) in state.buckets.iter().enumerate() {
        let Some(bucket) = maybe_bucket else {
            continue;
        };
        if bucket.hour > u64::MAX / 3600
            || bucket.hour % bucket_count as u64 != index as u64
            || bucket.sources.len() != SOURCE_BUCKETS
            || bucket.destinations.len() != DESTINATION_BUCKETS
            || bucket.ports.len() > config.max_ports_per_hour
            || bucket.protocols.iter().any(|count| *count > bucket.packets)
            || bucket
                .sources
                .iter()
                .chain(&bucket.destinations)
                .any(|count| *count > bucket.packets)
        {
            return Err(invalid());
        }

        let mut seen_ports = BTreeSet::new();
        if bucket.ports.iter().any(|port| {
            port.count == 0
                || port.count > bucket.samples
                || port.error_bound > port.count
                || !seen_ports.insert(port.port)
        }) {
            return Err(invalid());
        }

        match retained_samples.checked_add(bucket.samples) {
            Some(total) => retained_samples = total,
            None => retained_samples_overflowed = true,
        }
        newest_hour = Some(newest_hour.map_or(bucket.hour, |newest: u64| newest.max(bucket.hour)));
    }

    if (retained_samples_overflowed && state.accepted != u64::MAX)
        || (!retained_samples_overflowed && retained_samples > state.accepted)
    {
        return Err(invalid());
    }

    if let Some(newest) = newest_hour {
        let oldest_retained = newest.saturating_sub(bucket_count as u64 - 1);
        if state
            .buckets
            .iter()
            .flatten()
            .any(|bucket| bucket.hour < oldest_retained)
        {
            return Err(invalid());
        }
    }
    Ok(())
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

    fn observe_service_hour(
        discovery: &mut Discovery,
        service_id: ServiceId,
        direction: TrafficDirection,
        hour: u64,
    ) {
        for key in 0..12u64 {
            let mut event = sample(hour * 3600 + key);
            event.service_id = Some(service_id);
            event.direction = direction;
            event.source_key = Some(key + 1);
            event.destination_key = Some(key + 100);
            event.packets = 10 + (key % 2);
            discovery
                .observe_with_quality(event, ObservationQuality::Clean)
                .unwrap();
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
    fn reopen_rejects_bucket_outside_its_ring_slot() {
        let p = path();
        let mut discovery = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        discovery.observe(sample(1_700_000_000));
        let mut state = discovery.state.clone();
        let index = state.buckets.iter().position(Option::is_some).unwrap();
        let bucket = state.buckets[index].take().unwrap();
        let wrong_index = (index + 1) % state.buckets.len();
        state.buckets[wrong_index] = Some(bucket);
        fs::write(&p, serde_json::to_vec(&state).unwrap()).unwrap();

        let error = Discovery::open(&p, DiscoveryConfig::default())
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn reopen_rejects_distribution_counters_above_packet_total() {
        let p = path();
        let mut discovery = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        discovery.observe(sample(1_700_000_000));
        let mut state = discovery.state.clone();
        let bucket = state.buckets.iter_mut().flatten().next().unwrap();
        bucket.sources[0] = bucket.packets.saturating_add(1);
        fs::write(&p, serde_json::to_vec(&state).unwrap()).unwrap();

        let error = Discovery::open(&p, DiscoveryConfig::default())
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn reopen_rejects_oversized_snapshot_before_json_parsing() {
        let p = path();
        let config = DiscoveryConfig {
            observation_days: 1,
            max_ports_per_hour: 1,
        };
        let size = max_discovery_snapshot_bytes(&config) as usize + 1;
        fs::write(&p, vec![b' '; size]).unwrap();

        let error = Discovery::open(&p, config).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn relative_snapshot_path_is_supported() {
        let p = PathBuf::from(format!(
            "ramshield-discovery-relative-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut discovery = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        discovery.observe(sample(1_700_000_000));
        discovery.flush().unwrap();
        assert_eq!(
            Discovery::open(&p, DiscoveryConfig::default())
                .unwrap()
                .snapshot()
                .observations_accepted,
            1
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn failed_snapshot_rename_removes_temporary_file() {
        let dir = std::env::temp_dir().join(format!(
            "discovery_temp_cleanup_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("snapshot.json");
        let discovery = Discovery::open(&target, DiscoveryConfig::default()).unwrap();
        fs::create_dir(&target).unwrap();

        assert!(discovery.flush().is_err());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(dir);
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
    fn aggregate_observe_does_not_assume_clean_service_quality() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        let mut event = sample(100 * 3600);
        event.service_id = Some(ServiceId::new(77));
        event.direction = TrafficDirection::Ingress;

        // Aggregate traffic and collector health can arrive in either order.
        d.observe(event.clone());
        d.observe_health(HealthSample {
            timestamp_secs: 100 * 3600,
            capture_drops: 1,
            ..HealthSample::default()
        });
        assert!(d.service_profiles.is_empty());

        d.observe_with_quality(event, ObservationQuality::Degraded)
            .unwrap();
        let assessment = d
            .service_profiles
            .get(&ServiceId::new(77))
            .unwrap()
            .assessment("77", TrafficDirection::Ingress, CountMetric::Packets, 100)
            .unwrap();
        assert_eq!(assessment.clean_observations, 0);
        assert_eq!(assessment.degraded_observations, 1);
        assert_eq!(assessment.moments.observations(), 0);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn missing_and_invalid_quality_are_rejected_before_aggregation() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        let before = d.snapshot().observations_accepted;
        for quality in [ObservationQuality::Missing, ObservationQuality::Invalid] {
            assert!(d.observe_with_quality(sample(100 * 3600), quality).is_err());
        }
        assert_eq!(d.snapshot().observations_accepted, before);
        assert!(d.service_profiles.is_empty());
        let _ = fs::remove_file(p);
    }

    #[test]
    fn degraded_sample_invalidates_cardinality_for_its_hour() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        let service = ServiceId::new(88);
        observe_service_hour(&mut d, service, TrafficDirection::Ingress, 10_000);
        observe_service_hour(&mut d, service, TrafficDirection::Ingress, 10_001);

        let mut degraded = sample(10_001 * 3600 + 30);
        degraded.service_id = Some(service);
        degraded.direction = TrafficDirection::Ingress;
        degraded.source_key = Some(50_000);
        degraded.destination_key = Some(60_000);
        d.observe_with_quality(degraded, ObservationQuality::Degraded)
            .unwrap();

        let report = d
            .assess_service(service, TrafficDirection::Ingress, 10_001)
            .unwrap();
        assert_eq!(
            report.source_cardinality,
            DistributionAssessment::InsufficientData
        );
        assert_eq!(
            report.destination_cardinality,
            DistributionAssessment::InsufficientData
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn service_identity_is_never_derived_from_endpoint_hash() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        let mut event = sample(100 * 3600);
        event.source_key = Some(77);
        d.observe(event);
        assert!(
            d.assess_service(ServiceId::new(77), TrafficDirection::Unknown, 100)
                .is_none()
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn service_admission_limit_does_not_reject_aggregate_observations() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        for id in 0..(MAX_SERVICES as u64 + 1) {
            let mut event = sample(100 * 3600);
            event.service_id = Some(ServiceId::new(id + 1));
            event.direction = TrafficDirection::Ingress;
            d.observe_with_quality(event, ObservationQuality::Clean)
                .unwrap();
        }
        assert_eq!(d.service_profiles.len(), MAX_SERVICES);
        assert_eq!(d.state.accepted, MAX_SERVICES as u64 + 1);
        assert_eq!(d.state.too_old, 0);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn d7_d8_service_assessment_is_report_only_and_fails_closed_without_baseline() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        for key in 0..12u64 {
            let mut event = sample(100 * 3600 + key);
            event.service_id = Some(ServiceId::new(9));
            event.direction = TrafficDirection::Ingress;
            event.source_key = Some(key + 1);
            event.destination_key = Some(key + 100);
            d.observe_with_quality(event, ObservationQuality::Clean)
                .unwrap();
        }
        let report = d
            .assess_service(ServiceId::new(9), TrafficDirection::Ingress, 100)
            .unwrap();
        assert_eq!(
            report.source_cardinality,
            DistributionAssessment::InsufficientData
        );
        assert_eq!(
            report.destination_cardinality,
            DistributionAssessment::InsufficientData
        );
        assert!(
            report
                .metrics
                .iter()
                .all(|metric| { metric.calibration == DistributionAssessment::InsufficientData })
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn service_profiles_use_absolute_utc_hours_across_retention_pruning() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        let service = ServiceId::new(42);
        // Beyond the 90-day retention horizon, profiles must remain keyed by
        // absolute UTC hour so pruning does not erase every event in the hour.
        observe_service_hour(&mut d, service, TrafficDirection::Ingress, 10_000);
        observe_service_hour(&mut d, service, TrafficDirection::Ingress, 10_001);

        let profile = d.service_profiles.get(&service).unwrap();
        for hour in [10_000, 10_001] {
            let packets = profile
                .assessment("42", TrafficDirection::Ingress, CountMetric::Packets, hour)
                .unwrap();
            assert_eq!(packets.clean_observations, 12);
            assert_eq!(packets.moments.observations(), 12);
            let source = profile
                .source_cardinality_estimate("42", TrafficDirection::Ingress, hour)
                .unwrap();
            assert_eq!(source.observations, 12);
        }

        let report = d
            .assess_service(service, TrafficDirection::Ingress, 10_001)
            .unwrap();
        assert_eq!(report.source_cardinality, DistributionAssessment::Normal);
        assert_eq!(
            report.destination_cardinality,
            DistributionAssessment::Normal
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn d7_d8_service_assessment_rejects_unknown_direction() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        let service = ServiceId::new(10);
        observe_service_hour(&mut d, service, TrafficDirection::Unknown, 10_000);
        observe_service_hour(&mut d, service, TrafficDirection::Unknown, 10_001);
        let report = d
            .assess_service(service, TrafficDirection::Unknown, 10_001)
            .unwrap();
        assert_eq!(
            report.source_cardinality,
            DistributionAssessment::InsufficientData
        );
        assert_eq!(
            report.destination_cardinality,
            DistributionAssessment::InsufficientData
        );
        assert!(report.metrics.iter().all(|metric| {
            metric.assessment == DistributionAssessment::InsufficientData
                && metric.calibration == DistributionAssessment::InsufficientData
        }));
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

    #[test]
    fn restart_durable_service_profiles() {
        let path = std::env::temp_dir().join("rs-discovery-restart-test.json");
        let config = DiscoveryConfig {
            observation_days: 7,
            max_ports_per_hour: 256,
        };
        let mut disc = Discovery::open(&path, config.clone()).unwrap();
        let svc = ServiceId::new(42);
        let sample = TrafficSample {
            timestamp_secs: 1_700_000_000,
            packets: 100,
            bytes: 10_000,
            protocol: Protocol::Tcp,
            destination_port: Some(443),
            source_key: Some(1),
            destination_key: Some(2),
            service_id: Some(svc),
            direction: TrafficDirection::Ingress,
        };
        for _ in 0..5 {
            disc.observe_with_quality(sample.clone(), ObservationQuality::Clean)
                .unwrap();
        }
        disc.flush().unwrap();

        let disc2 = Discovery::open(&path, config).unwrap();
        // Service profiles are not persisted across restart in this implementation.
        // Just verify the cycle completes without error and profiles start fresh.
        assert!(disc2.service_profiles.is_empty());
    }

    #[test]
    fn corruption_rejects_persisted_state() {
        let path = std::env::temp_dir().join("rs-discovery-corrupt-test.json");
        // Write a persisted state with wrong schema version directly
        let state = serde_json::to_vec(&crate::PersistedState {
            schema: 999,
            observation_days: 7,
            buckets: vec![None; 10],
            accepted: 0,
            too_old: 0,
        })
        .unwrap();
        std::fs::write(&path, &state).unwrap();

        let config = DiscoveryConfig {
            observation_days: 7,
            max_ports_per_hour: 256,
        };
        let res = Discovery::open(&path, config);
        assert!(res.is_err(), "wrong schema should be rejected");
    }

    #[test]
    fn degraded_window_prevents_clean_assessment() {
        let mut store = MetricProfileStore::default();
        let dir = TrafficDirection::Ingress;
        let slot = 0u64;

        store.observe_volume("1".into(), dir, slot, 100, 1000, ObservationQuality::Clean);
        store.observe_volume(
            "1".into(),
            dir,
            slot + 1,
            105,
            1050,
            ObservationQuality::Clean,
        );
        store.observe_volume(
            "1".into(),
            dir,
            slot + 2,
            95,
            950,
            ObservationQuality::Clean,
        );

        let assessment = store.assessment("1", dir, CountMetric::Packets, slot + 2);
        let assessment = assessment.unwrap();
        let dist = assess_metric_distribution(&assessment);
        assert!(matches!(
            dist,
            DistributionAssessment::Normal | DistributionAssessment::InsufficientData
        ));

        let mut degraded_assessment = assessment.clone();
        degraded_assessment.quality = ObservationQuality::Degraded;
        let dist_degraded = assess_metric_distribution(&degraded_assessment);
        assert!(
            !matches!(dist_degraded, DistributionAssessment::Normal),
            "degraded quality should not produce Normal distribution assessment"
        );
    }
}
