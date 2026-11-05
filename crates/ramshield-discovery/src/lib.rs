//! Passive production-environment discovery.
//! This crate has no dependency on detection or enforcement. It accepts
//! normalized observations, keeps bounded hourly aggregates, and persists
//! them atomically. It never changes live configuration or network state.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

mod calendar;
mod ingest;
mod metric;
mod stats;
mod window;
pub use calendar::{
    CalendarAssessment, CalendarAssessmentKind, CalendarSlotSummary, CalendarSnapshot,
    ExecutionState, ExpectedWorkload, Recurrence, TrafficCalendar, WorkloadDeclaration,
    WorkloadExecution,
};
pub use ingest::{CollectorHealth, IngestConfig, IngestionEngine};
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

    pub fn as_string(&self) -> String {
        format!("svc_{:016x}", self.0)
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
    service_profiles: Option<Vec<u8>>, // serialized MetricProfileStore
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
    /// Tracks (service_id, direction, hour_utc) windows already trained into
    /// profiles. Prevents double-training from duplicate record_completed_window()
    /// calls within the same Discovery instance. Starts empty on reopen (runtime only).
    /// Grouped by hour so retention pruning removes whole expired hour buckets.
    recorded_windows: BTreeMap<u64, BTreeSet<(ServiceId, TrafficDirection)>>,
}

/// Context for recording a completed window into service profiles.
#[derive(Debug, Clone)]
pub struct RecordCompletedWindowCtx {
    pub service_id: ServiceId,
    pub direction: TrafficDirection,
    pub hour_utc: u64,
    pub packets: u64,
    pub bytes: u64,
    pub quality: ObservationQuality,
    pub source_key: Option<u64>,
    pub dest_key: Option<u64>,
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
            state // return raw state first
        } else {
            PersistedState {
                schema: SCHEMA,
                observation_days: config.observation_days,
                buckets: vec![None; config.bucket_count()],
                accepted: 0,
                too_old: 0,
                service_profiles: None,
            }
        };
        // Rehydrate service profiles stored on disk (if any).
        let service_profiles: BTreeMap<ServiceId, MetricProfileStore> =
            match &state.service_profiles {
                Some(bytes) => serde_json::from_slice(bytes).map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("corrupt service profiles: {e}"),
                    )
                })?,
                None => BTreeMap::new(),
            };
        let recorded_windows: BTreeMap<u64, BTreeSet<(ServiceId, TrafficDirection)>> =
            BTreeMap::new();
        Ok(Self {
            config,
            state,
            path,
            service_profiles,
            recorded_windows,
        })
    }

    /// Aggregate a normalized observation. No enforcement or detection action is possible here.
    pub fn observe(&mut self, sample: TrafficSample) {
        let hour_utc = sample.timestamp_secs / 3600;
        let _calendar_slot = hour_utc; // absolute UTC hour; matches assess_service lookup
        let port_limit = self.config.max_ports_per_hour;
        let Some(bucket) = self.bucket_mut(hour_utc) else {
            self.state.too_old = self.state.too_old.saturating_add(1);
            return;
        };
        let _quality =
            if bucket.capture_drops > 0 || bucket.ingest_drops > 0 || bucket.malformed > 0 {
                ObservationQuality::Degraded
            } else {
                ObservationQuality::Clean
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
        // Service profiles are intentionally NOT updated here from raw events.
        // Training service profiles from individual observations before window
        // completeness is known risks contaminating baselines with degraded data.
        // Use record_completed_window() to feed service profiles from sealed,
        // quality-qualified completed windows.
        self.state.accepted = self.state.accepted.saturating_add(1);
    }

    /// Record a completed, quality-qualified window into service profiles.
    /// Only sealed windows whose completeness has been independently determined
    /// train baseline state, preventing contaminated data from entering assessments.
    /// Duplicate calls with the same (service_id, direction, hour_utc)
    /// are idempotent within one Discovery instance — the window is only
    /// trained once, even if record_completed_window() is called multiple
    /// times with the same ctx. The deduplication ledger is runtime-only and
    /// resets on reopen; replaying a window after a restart retrains the
    /// persisted profile.
    /// PRECONDITION: The caller MUST ensure ctx represents a window that has been
    /// independently verified as sealed and quality-qualified (e.g., via
    /// IngestionEngine::flush() which only seals windows whose end time has passed
    /// and whose capture health is known). This API does not cryptographically
    /// prove provenance — it relies on the caller's correct usage.
    pub fn record_completed_window(&mut self, ctx: RecordCompletedWindowCtx) {
        // Idempotency guard: skip if this (service_id, direction, hour_utc)
        // was already trained.
        if self
            .recorded_windows
            .get(&ctx.hour_utc)
            .is_some_and(|set| set.contains(&(ctx.service_id, ctx.direction)))
        {
            return;
        }
        // Service capacity gate: reject new service IDs before touching the map.
        let has_capacity = self.service_profiles.contains_key(&ctx.service_id)
            || self.service_profiles.len() < MAX_SERVICES;
        if !has_capacity {
            return;
        }
        // Monotonic retention horizon: newest hour is the last key in the
        // ordered map, so no full-map scan is needed.
        let newest_hour = self
            .recorded_windows
            .last_key_value()
            .map(|(&hour, _)| hour)
            .unwrap_or(ctx.hour_utc)
            .max(ctx.hour_utc);
        let retention_hour = newest_hour.saturating_sub(self.config.bucket_count() as u64 - 1);
        // Drop entire expired hour buckets (O(expired buckets), not O(all keys)).
        while self
            .recorded_windows
            .first_key_value()
            .is_some_and(|(&hour, _)| hour < retention_hour)
        {
            self.recorded_windows.pop_first();
        }
        // Reject windows older than the retention horizon entirely so an
        // out-of-order old event cannot re-enter after being pruned.
        if ctx.hour_utc < retention_hour {
            return;
        }
        if !self
            .recorded_windows
            .entry(ctx.hour_utc)
            .or_default()
            .insert((ctx.service_id, ctx.direction))
        {
            return;
        }

        let profile = self.service_profiles.entry(ctx.service_id).or_default();
        let oldest_hour = ctx
            .hour_utc
            .saturating_sub(self.config.bucket_count() as u64 - 1);
        profile.prune_before(oldest_hour);
        let service_key = ctx.service_id.as_u64().to_string();
        profile.observe_volume(
            service_key.clone(),
            ctx.direction,
            ctx.hour_utc,
            ctx.packets,
            ctx.bytes,
            ctx.quality,
        );
        profile.observe_cardinality(
            service_key,
            ctx.direction,
            ctx.hour_utc,
            ctx.source_key,
            ctx.dest_key,
            ctx.quality,
        );
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

        let current_source = profile.source_cardinality_estimate(&service_key, direction, hour);
        let baseline_source = previous_hour.and_then(|previous| {
            profile.source_cardinality_estimate(&service_key, direction, previous)
        });
        let current_destination = profile.dest_cardinality_estimate(&service_key, direction, hour);
        let baseline_destination = previous_hour.and_then(|previous| {
            profile.dest_cardinality_estimate(&service_key, direction, previous)
        });

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

    /// Atomic snapshot persistence: temp file, file fsync, rename, directory fsync.
    pub fn flush(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut state = self.state.clone();
        state.service_profiles = Some(
            serde_json::to_vec(&self.service_profiles)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
        );
        let bytes = serde_json::to_vec(&state)
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
        // Cap the observed hour at wall time plus a 1-hour skew tolerance so that
        // a single future timestamp cannot advance the retention horizon and evict
        // valid historical buckets. The tolerance accommodates legitimate clock skew.
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // `hour` is an hour index; convert wall time to hours and allow 1h skew.
        let max_hour = (now_secs / 3600).saturating_add(1);
        let effective_hour = hour.min(max_hour);
        let retention = self.config.bucket_count() as u64;
        if let Some(newest) = self
            .state
            .buckets
            .iter()
            .flatten()
            .map(|bucket| bucket.hour)
            .max()
        {
            let newest = newest.max(effective_hour);
            let oldest = newest.saturating_sub(retention - 1);
            if effective_hour < oldest {
                return None;
            }
            for bucket in &mut self.state.buckets {
                if bucket.as_ref().is_some_and(|bucket| bucket.hour < oldest) {
                    *bucket = None;
                }
            }
        }
        let len = self.state.buckets.len();
        let index = (effective_hour % len as u64) as usize;
        if self.state.buckets[index]
            .as_ref()
            .is_none_or(|bucket| bucket.hour != effective_hour)
        {
            self.state.buckets[index] = Some(HourBucket::new(effective_hour));
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
        d.record_completed_window(crate::RecordCompletedWindowCtx {
            service_id: ServiceId::new(1),
            direction: TrafficDirection::Ingress,
            hour_utc: 100,
            packets: 3,
            bytes: 64,
            quality: crate::ObservationQuality::Clean,
            source_key: None,
            dest_key: None,
        });
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
        d.record_completed_window(crate::RecordCompletedWindowCtx {
            service_id: ServiceId::new(1),
            direction: TrafficDirection::Ingress,
            hour_utc: 200,
            packets: 99,
            bytes: 6400,
            quality: crate::ObservationQuality::Clean,
            source_key: None,
            dest_key: None,
        });
        let s = d.snapshot();
        assert!(s.hours[0].top_ports.len() <= 2);
        assert!(s.hours.len() <= 24);
        assert_eq!(s.observations_accepted, 99);
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
            d.observe(event);
        }
        // Record completed windows for first MAX_SERVICES
        for id in 0..MAX_SERVICES {
            d.record_completed_window(crate::RecordCompletedWindowCtx {
                service_id: ServiceId::new((id + 1) as u64),
                direction: TrafficDirection::Ingress,
                hour_utc: 100,
                packets: 1,
                bytes: 64,
                quality: crate::ObservationQuality::Clean,
                source_key: None,
                dest_key: None,
            });
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
            d.record_completed_window(crate::RecordCompletedWindowCtx {
                service_id: ServiceId::new(9),
                direction: TrafficDirection::Ingress,
                hour_utc: 100,
                packets: 10,
                bytes: 640,
                quality: crate::ObservationQuality::Clean,
                source_key: Some(key + 1),
                dest_key: Some(key + 100),
            });
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
    fn d7_d8_service_assessment_rejects_unknown_direction() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        for key in 0..12u64 {
            d.record_completed_window(crate::RecordCompletedWindowCtx {
                service_id: ServiceId::new(10),
                direction: TrafficDirection::Unknown,
                hour_utc: 100,
                packets: 10,
                bytes: 640,
                quality: crate::ObservationQuality::Clean,
                source_key: Some(key + 1),
                dest_key: Some(key + 100),
            });
        }
        let report = d
            .assess_service(ServiceId::new(10), TrafficDirection::Unknown, 100)
            .unwrap();
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
            disc.observe(sample.clone());
        }
        disc.record_completed_window(crate::RecordCompletedWindowCtx {
            service_id: svc,
            direction: TrafficDirection::Ingress,
            hour_utc: sample.timestamp_secs / 3600,
            packets: 500,
            bytes: 50_000,
            quality: crate::ObservationQuality::Clean,
            source_key: Some(1),
            dest_key: Some(2),
        });
        disc.flush().unwrap();

        let disc2 = Discovery::open(&path, config).unwrap();
        // Service profiles must survive restart (durability).
        assert!(
            !disc2.service_profiles.is_empty(),
            "profiles must persist across restart"
        );
        let slot = sample.timestamp_secs / 3600;
        let svc = disc2.service_profiles.get(&ServiceId::new(42)).unwrap();
        assert!(
            !svc.all_assessments("42", TrafficDirection::Ingress, slot)
                .is_empty()
        );
    }

    #[test]
    fn future_timestamp_does_not_advance_retention_horizon() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        let now_secs = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let now_hour = now_secs / 3600;
        let recent_hour = now_hour - 10;
        // Normal recent observation trains successfully.
        d.observe(sample(recent_hour * 3600));
        assert!(
            d.state
                .buckets
                .iter()
                .flatten()
                .any(|b| b.hour == recent_hour)
        );
        // Far-future observation must not advance the retention horizon.
        let future_hour = now_hour + 100_000;
        d.observe(sample(future_hour * 3600));
        let newest_hour = d
            .state
            .buckets
            .iter()
            .flatten()
            .map(|b| b.hour)
            .max()
            .unwrap_or(0);
        assert!(
            newest_hour < future_hour,
            "future hour {future_hour} must not advance retention horizon"
        );
        assert!(
            d.state
                .buckets
                .iter()
                .flatten()
                .any(|b| b.hour == recent_hour),
            "recent bucket must survive future timestamp"
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn recorded_windows_map_stays_bounded() {
        let p = path();
        // 1-day retention = 24 hourly buckets. Record 200 hours so the map
        // must prune, not merely stay under a large default.
        let config = DiscoveryConfig {
            observation_days: 1,
            max_ports_per_hour: 256,
        };
        let mut d = Discovery::open(&p, config).unwrap();
        let svc = ServiceId::new(1);
        for hour in 0..200u64 {
            d.record_completed_window(crate::RecordCompletedWindowCtx {
                service_id: svc,
                direction: TrafficDirection::Ingress,
                hour_utc: 10_000 + hour,
                packets: 1,
                bytes: 64,
                quality: crate::ObservationQuality::Clean,
                source_key: None,
                dest_key: None,
            });
        }
        // The map must be bounded by the 24-hour retention window, not 200.
        let retention = d.config.bucket_count();
        assert_eq!(retention, 24);
        // Hour-bucket count stays bounded by retention (each bucket holds
        // all services/directions seen in that hour).
        assert!(
            d.recorded_windows.len() <= retention,
            "hour buckets len {} exceeds retention {}",
            d.recorded_windows.len(),
            retention
        );
        // The most recent window must still be present (idempotency intact).
        assert!(
            d.recorded_windows
                .get(&10_199)
                .is_some_and(|set| set.contains(&(svc, TrafficDirection::Ingress)))
        );
        // The oldest window must have been pruned.
        assert!(!d.recorded_windows.contains_key(&10_000));
        let _ = fs::remove_file(p);
    }

    #[test]
    fn recorded_windows_respects_max_services() {
        let p = path();
        let config = DiscoveryConfig {
            observation_days: 1,
            max_ports_per_hour: 256,
        };
        let mut d = Discovery::open(&p, config).unwrap();
        // Try to record for more distinct services than MAX_SERVICES (64).
        for i in 0..70u64 {
            let svc = ServiceId::new(i);
            d.record_completed_window(crate::RecordCompletedWindowCtx {
                service_id: svc,
                direction: TrafficDirection::Ingress,
                hour_utc: 10_000,
                packets: 1,
                bytes: 64,
                quality: crate::ObservationQuality::Clean,
                source_key: None,
                dest_key: None,
            });
        }
        // Only 64 services should be admitted; the rest silently dropped.
        let recorded_count: usize = d.recorded_windows.values().map(|set| set.len()).sum();
        assert_eq!(
            recorded_count, MAX_SERVICES,
            "recorded_windows count {} exceeds max services {}",
            recorded_count, MAX_SERVICES
        );
        // First 64 services must have profile entries.
        for i in 0..MAX_SERVICES as u64 {
            let svc = ServiceId::new(i);
            assert!(
                d.service_profiles.contains_key(&svc),
                "service {} must have profile entry",
                i
            );
        }
        // 65th service must NOT have a profile.
        assert!(
            !d.service_profiles.contains_key(&ServiceId::new(64)),
            "service 65 must be rejected due to capacity"
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn recorded_windows_old_after_new_does_not_move_horizon() {
        let p = path();
        let config = DiscoveryConfig {
            observation_days: 1,
            max_ports_per_hour: 256,
        };
        let mut d = Discovery::open(&p, config).unwrap();
        let svc = ServiceId::new(1);
        // Record a new window at hour 100.
        d.record_completed_window(crate::RecordCompletedWindowCtx {
            service_id: svc,
            direction: TrafficDirection::Ingress,
            hour_utc: 100,
            packets: 1,
            bytes: 64,
            quality: crate::ObservationQuality::Clean,
            source_key: None,
            dest_key: None,
        });
        // Record a very old window (hour 0) — should NOT move the retention
        // horizon backwards, so hour 100 must stay.
        d.record_completed_window(crate::RecordCompletedWindowCtx {
            service_id: svc,
            direction: TrafficDirection::Ingress,
            hour_utc: 0,
            packets: 1,
            bytes: 64,
            quality: crate::ObservationQuality::Clean,
            source_key: None,
            dest_key: None,
        });
        // The retention horizon is based on the newest hour (100), so 100
        // must still be present.
        assert!(
            d.recorded_windows
                .get(&100)
                .is_some_and(|set| set.contains(&(svc, TrafficDirection::Ingress)))
        );
        // Hour 0 should be pruned because it's outside the 24-hour horizon
        // from the newest hour (100).
        assert!(!d.recorded_windows.contains_key(&0));
        let _ = fs::remove_file(p);
    }

    fn mk_ctx(
        svc: u64,
        direction: TrafficDirection,
        hour: u64,
        packets: u64,
    ) -> crate::RecordCompletedWindowCtx {
        crate::RecordCompletedWindowCtx {
            service_id: ServiceId::new(svc),
            direction,
            hour_utc: hour,
            packets,
            bytes: packets * 64,
            quality: crate::ObservationQuality::Clean,
            source_key: None,
            dest_key: None,
        }
    }

    fn total_recorded(d: &Discovery) -> usize {
        d.recorded_windows.values().map(|set| set.len()).sum()
    }

    /// Count trained assessments for a service/direction/hour. Zero means the
    /// window never trained a profile — the behavioural signal these tests need.
    fn assessment_count(d: &Discovery, svc: u64, dir: TrafficDirection, hour: u64) -> usize {
        d.service_profiles
            .get(&ServiceId::new(svc))
            .map(|s| s.all_assessments(&svc.to_string(), dir, hour).len())
            .unwrap_or(0)
    }

    #[test]
    fn recorded_windows_duplicate_is_idempotent() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        let ctx = mk_ctx(1, TrafficDirection::Ingress, 100, 10);
        d.record_completed_window(ctx.clone());
        let after_first = d
            .service_profiles
            .get(&ServiceId::new(1))
            .and_then(|s| {
                s.all_assessments("1", TrafficDirection::Ingress, 100)
                    .into_iter()
                    .next()
            })
            .map(|a| (a.observed_packets, a.clean_observations));
        d.record_completed_window(ctx);
        let after_second = d
            .service_profiles
            .get(&ServiceId::new(1))
            .and_then(|s| {
                s.all_assessments("1", TrafficDirection::Ingress, 100)
                    .into_iter()
                    .next()
            })
            .map(|a| (a.observed_packets, a.clean_observations));
        assert!(
            after_first.is_some(),
            "first call must actually train a profile"
        );
        assert_eq!(after_first, after_second, "duplicate must not re-train");
        assert_eq!(total_recorded(&d), 1);
        assert_eq!(assessment_count(&d, 1, TrafficDirection::Ingress, 100), 2);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn recorded_windows_allows_retention_boundary() {
        let p = path();
        let config = DiscoveryConfig {
            observation_days: 1, // 24 buckets
            max_ports_per_hour: 256,
        };
        let mut d = Discovery::open(&p, config).unwrap();
        // Newest hour 100, retention 24 => horizon = 100 - 23 = 77 (inclusive).
        d.record_completed_window(mk_ctx(1, TrafficDirection::Ingress, 100, 1));
        d.record_completed_window(mk_ctx(1, TrafficDirection::Ingress, 77, 1));
        assert!(
            d.recorded_windows
                .get(&77)
                .is_some_and(|s| s.contains(&(ServiceId::new(1), TrafficDirection::Ingress))),
            "hour at the exact retention boundary must be accepted"
        );
        assert!(
            assessment_count(&d, 1, TrafficDirection::Ingress, 77) > 0,
            "boundary window must train a profile, not just a dedup key"
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn recorded_windows_rejects_before_retention_boundary() {
        let p = path();
        let config = DiscoveryConfig {
            observation_days: 1,
            max_ports_per_hour: 256,
        };
        let mut d = Discovery::open(&p, config).unwrap();
        d.record_completed_window(mk_ctx(1, TrafficDirection::Ingress, 100, 1));
        // 76 is one hour before the horizon (77) and must be rejected.
        d.record_completed_window(mk_ctx(1, TrafficDirection::Ingress, 76, 1));
        assert!(!d.recorded_windows.contains_key(&76));
        assert_eq!(
            assessment_count(&d, 1, TrafficDirection::Ingress, 76),
            0,
            "rejected window must not train a profile"
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn recorded_windows_prunes_multiple_hour_buckets() {
        let p = path();
        let config = DiscoveryConfig {
            observation_days: 1, // 24 buckets
            max_ports_per_hour: 256,
        };
        let mut d = Discovery::open(&p, config).unwrap();
        // Insert at hour 0, then jump forward far enough that many buckets expire.
        d.record_completed_window(mk_ctx(1, TrafficDirection::Ingress, 0, 1));
        assert!(d.recorded_windows.contains_key(&0));
        d.record_completed_window(mk_ctx(1, TrafficDirection::Ingress, 1000, 1));
        // All buckets older than 1000 - 23 = 977 are gone.
        assert_eq!(d.recorded_windows.len(), 1);
        assert!(d.recorded_windows.contains_key(&1000));
        assert_eq!(
            assessment_count(&d, 1, TrafficDirection::Ingress, 0),
            0,
            "expired hour profile must be pruned"
        );
        assert!(
            assessment_count(&d, 1, TrafficDirection::Ingress, 1000) > 0,
            "new hour profile must be trained"
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn recorded_windows_separates_directions() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        d.record_completed_window(mk_ctx(1, TrafficDirection::Ingress, 100, 10));
        d.record_completed_window(mk_ctx(1, TrafficDirection::Egress, 100, 20));
        let set = d.recorded_windows.get(&100).unwrap();
        assert!(set.contains(&(ServiceId::new(1), TrafficDirection::Ingress)));
        assert!(set.contains(&(ServiceId::new(1), TrafficDirection::Egress)));
        assert_eq!(total_recorded(&d), 2);
        assert!(
            assessment_count(&d, 1, TrafficDirection::Ingress, 100) > 0,
            "ingress profile must be trained"
        );
        assert!(
            assessment_count(&d, 1, TrafficDirection::Egress, 100) > 0,
            "egress profile must be trained independently"
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn recorded_windows_reopens_with_empty_runtime_dedup() {
        let p = path();
        let config = DiscoveryConfig::default();
        {
            let mut d = Discovery::open(&p, config.clone()).unwrap();
            d.record_completed_window(mk_ctx(1, TrafficDirection::Ingress, 100, 10));
            d.flush().unwrap();
            assert_eq!(total_recorded(&d), 1);
        }
        // Reopen: runtime-only idempotency state must be empty...
        let d2 = Discovery::open(&p, config).unwrap();
        assert_eq!(total_recorded(&d2), 0);
        // ...but the persisted profile state must survive.
        assert!(
            assessment_count(&d2, 1, TrafficDirection::Ingress, 100) > 0,
            "profile must persist across reopen even though dedup state resets"
        );
        let _ = fs::remove_file(p);
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
            service_profiles: None,
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

    #[test]
    fn record_completed_window_idempotency() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        let ctx = crate::RecordCompletedWindowCtx {
            service_id: ServiceId::new(1),
            direction: TrafficDirection::Ingress,
            hour_utc: 100,
            packets: 10,
            bytes: 64,
            quality: crate::ObservationQuality::Clean,
            source_key: None,
            dest_key: None,
        };
        // Capture profile state before any training (empty)
        let prof_before = d.service_profiles.get(&ctx.service_id).and_then(|s| {
            s.all_assessments(
                &ctx.service_id.as_u64().to_string(),
                ctx.direction,
                ctx.hour_utc,
            )
            .into_iter()
            .next()
            .map(|a| {
                (
                    a.observed_packets,
                    a.clean_observations,
                    a.degraded_observations,
                )
            })
        });
        // First call trains the profile
        d.record_completed_window(ctx.clone());
        let prof_after_first = d.service_profiles.get(&ctx.service_id).and_then(|s| {
            s.all_assessments(
                &ctx.service_id.as_u64().to_string(),
                ctx.direction,
                ctx.hour_utc,
            )
            .into_iter()
            .next()
            .map(|a| {
                (
                    a.observed_packets,
                    a.clean_observations,
                    a.degraded_observations,
                )
            })
        });
        // Second call must not change profile state
        d.record_completed_window(ctx.clone());
        let prof_after_second = d.service_profiles.get(&ctx.service_id).and_then(|s| {
            s.all_assessments(
                &ctx.service_id.as_u64().to_string(),
                ctx.direction,
                ctx.hour_utc,
            )
            .into_iter()
            .next()
            .map(|a| {
                (
                    a.observed_packets,
                    a.clean_observations,
                    a.degraded_observations,
                )
            })
        });
        // Second call must not change profile state
        assert_eq!(prof_after_first, prof_after_second);
        // First call must have trained (different from empty)
        assert_ne!(prof_before, prof_after_first);
        // Only one map entry
        assert_eq!(d.service_profiles.len(), 1);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn record_completed_window_direction_isolation() {
        let p = path();
        let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        let ingress_ctx = crate::RecordCompletedWindowCtx {
            service_id: ServiceId::new(1),
            direction: TrafficDirection::Ingress,
            hour_utc: 100,
            packets: 10,
            bytes: 64,
            quality: crate::ObservationQuality::Clean,
            source_key: None,
            dest_key: None,
        };
        let egress_ctx = crate::RecordCompletedWindowCtx {
            service_id: ServiceId::new(1),
            direction: TrafficDirection::Egress,
            hour_utc: 100,
            packets: 10,
            bytes: 64,
            quality: crate::ObservationQuality::Clean,
            source_key: None,
            dest_key: None,
        };

        d.record_completed_window(ingress_ctx.clone());
        d.record_completed_window(egress_ctx.clone());

        // Both ingress and egress profiles must be populated for the same
        // service + hour.
        assert_eq!(d.service_profiles.len(), 1);
        {
            let svc = d.service_profiles.get(&ServiceId::new(1)).unwrap();
            assert!(
                !svc.all_assessments("1", TrafficDirection::Ingress, 100)
                    .is_empty()
            );
            assert!(
                !svc.all_assessments("1", TrafficDirection::Egress, 100)
                    .is_empty()
            );
        }

        // Duplicate calls per direction must remain idempotent.
        let (ing_before, egr_before) = {
            let svc = d.service_profiles.get(&ServiceId::new(1)).unwrap();
            (
                svc.all_assessments("1", TrafficDirection::Ingress, 100)
                    .len(),
                svc.all_assessments("1", TrafficDirection::Egress, 100)
                    .len(),
            )
        };
        d.record_completed_window(ingress_ctx.clone());
        d.record_completed_window(egress_ctx.clone());
        {
            let svc = d.service_profiles.get(&ServiceId::new(1)).unwrap();
            assert_eq!(
                svc.all_assessments("1", TrafficDirection::Ingress, 100)
                    .len(),
                ing_before
            );
            assert_eq!(
                svc.all_assessments("1", TrafficDirection::Egress, 100)
                    .len(),
                egr_before
            );
        }
        let _ = fs::remove_file(p);
    }
}
