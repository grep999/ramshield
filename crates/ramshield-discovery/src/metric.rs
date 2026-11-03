//! Metric identity and assessment for RamShield discovery.
//! Extends the existing CountMoments layer with metric-level profiles.
//! Every metric entering the statistical engine must carry a CountMetric identity.
//! This prevents mixing units (e.g. packets vs bytes) in the same model.

use crate::{CountMoments, TrafficDirection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable metric identity used as part of a profile key.
/// The calendar_slot is an absolute UTC hour since epoch, not a
/// hour-of-week slot. This ensures observe() and assess_service()
/// use the same key domain, enabling correct retention and
/// previous-hour comparison.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum CountMetric {
    /// Raw packet count.
    Packets,
    /// Raw byte count.
    Bytes,
    /// Flow count (when source supplies real flow telemetry).
    Flows,
    /// TCP-specific packet count.
    TcpPackets,
    /// UDP-specific packet count.
    UdpPackets,
    /// ICMP-specific packet count.
    IcmpPackets,
    /// ICMPv6-specific packet count.
    IcmpV6Packets,
    /// Catches any other protocol-specific packet count.
    OtherPackets,
}

/// A profile scoped to service, direction, metric and calendar slot.
/// The statistical engine maintains one independent CountMoments per profile key.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProfileKey {
    pub service_id: String,
    pub direction: TrafficDirection,
    pub metric: CountMetric,
    pub calendar_slot: u64, // hour-of-week = weekday * 24 + hour_of_day
}

impl ProfileKey {
    pub fn new(
        service_id: String,
        direction: TrafficDirection,
        metric: CountMetric,
        hour_utc: u64,
    ) -> Self {
        ProfileKey {
            service_id,
            direction,
            metric,
            calendar_slot: hour_utc, // absolute UTC hour
        }
    }
}

/// Observation quality metadata: whether a sample is clean, missing, degraded,
/// or invalid. Only clean completed observations feed CountMoments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservationQuality {
    /// Fully observed, normal window.
    Clean,
    /// Window had no telemetry at all (e.g. collector was down).
    Missing,
    /// Window had some telemetry but with known gaps (drops, malformed).
    Degraded,
    /// Observations cannot be validated (e.g. corrupted parser output).
    Invalid,
}

/// A metric-level assessment that reuses the existing statistical primitives
/// but applies observation-quality gating.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricAssessment {
    pub metric: CountMetric,
    pub observed_packets: u64,
    pub quality: ObservationQuality,
    pub moments: CountMoments,
    pub clean_observations: u64,
    pub degraded_observations: u64,
    pub baseline_source: Option<String>,
    pub last_updated_secs: u64,
}

impl Default for MetricAssessment {
    fn default() -> Self {
        MetricAssessment {
            metric: CountMetric::Packets,
            observed_packets: 0,
            quality: ObservationQuality::Clean,
            moments: CountMoments::default(),
            clean_observations: 0,
            degraded_observations: 0,
            baseline_source: None,
            last_updated_secs: 0,
        }
    }
}

/// Initialise a fresh metric profile for a service/direction/metric/calendar slot.
/// All counters start at zero; moments default to CountMoments::default().
pub fn new_profile_key(
    service_id: String,
    direction: TrafficDirection,
    metric: CountMetric,
    hour_utc: u64,
) -> ProfileKey {
    ProfileKey::new(service_id, direction, metric, hour_utc)
}

/// Apply a clean observed value to a CountMoments instance.
/// Quality is asserted Clean before pushing. Missing/Degraded/Invalid must
/// not be passed through this path.
pub fn observe_clean(moments: &mut CountMoments, value: u64) {
    moments.push(value);
}

/// Extend an existing MetricAssessment with a clean observation.
pub fn extend_assessment(assessment: &mut MetricAssessment, value: u64) {
    // Quality must already be Clean; invalid transitions are silently rejected
    // to keep this function simple and avoid surprise data-loss.
    if assessment.quality == ObservationQuality::Clean {
        observe_clean(&mut assessment.moments, value);
        assessment.observed_packets = assessment.observed_packets.saturating_add(value);
        assessment.clean_observations = assessment.clean_observations.saturating_add(1);
    }
}

/// Reset quality counts to zero. Retains the moments state.
pub fn reset_quality_counts(assessment: &mut MetricAssessment) {
    assessment.clean_observations = 0;
    assessment.degraded_observations = 0;
}

/// Result of a cardinality estimation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardinalityEstimate {
    pub estimate: u64,
    pub standard_error: f64,
    pub observations: u64,
}

/// Linear Counting cardinality estimator.
/// Fixed-size bitmap; bounded memory regardless of distinct key count.
/// Accurate at low-to-moderate cardinalities; error grows as the bitmap fills.
/// Only producer-side hashed keys are observed — no raw IP addresses.
/// Hash quality matters: poor distribution biases estimates. Prefer stable,
/// well-distributed hashes from the capture/producer layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardinalityEstimator {
    bitmap: Vec<u64>,
    bit_count: usize,
    observed: u64,
}

impl CardinalityEstimator {
    /// Create an estimator with `bit_count` bitmap bits.
    /// Default 4096 bits (512 bytes) supports up to ~2000 distinct keys
    /// with reasonable relative error.
    pub fn new(bit_count: usize) -> Self {
        // A zero-bit estimator cannot accept observations (modulo by zero).
        // Keep the public constructor total and guarantee at least one bit.
        let bit_count = bit_count.max(1);
        Self {
            bitmap: vec![0; bit_count.div_ceil(64)],
            bit_count,
            observed: 0,
        }
    }

    pub fn bit_count(&self) -> usize {
        self.bit_count
    }

    /// Observe a hashed identity key.
    pub fn observe(&mut self, key: u64) {
        use ahash::RandomState;
        // Fixed-seed RandomState for deterministic hashing within a crate version.
        // Same key always maps to same bit within the same build of the same crate version.
        // Cross-version persistence: if the ahash crate changes its internal hash format
        // between major versions, persisted bitmap indexes would require migration.
        // Within a single long-running process or same-crate-version restarts, stability is guaranteed.
        let hash = RandomState::with_seeds(0, 0, 0, 0).hash_one(key);
        let bit = (hash as usize) % self.bit_count;
        let word = bit / 64;
        let mask = 1u64 << (bit % 64);
        if self.bitmap[word] & mask == 0 {
            self.bitmap[word] |= mask;
        }
        self.observed = self.observed.saturating_add(1);
    }

    /// Return the current estimate with approximate standard error.
    /// When the bitmap saturates (v == 0), `estimate` is `u64::MAX` and
    /// `standard_error` is `f64::INFINITY` — treat results as unreliable.
    /// `standard_error` is an approximate heuristic, not a calibrated
    /// confidence interval.
    pub fn estimate(&self) -> CardinalityEstimate {
        let m = self.bit_count as f64;
        let v = self.zero_bits() as f64;
        let estimate = if v == 0.0 {
            u64::MAX // bitmap saturated
        } else {
            (-m * (v / m).ln()) as u64
        };
        // Approximate standard error for linear counting — heuristic only.
        let se = if v > 0.0 {
            estimate as f64 / v.sqrt()
        } else {
            f64::INFINITY
        };
        CardinalityEstimate {
            estimate,
            standard_error: se,
            observations: self.observed,
        }
    }

    /// True when the bitmap has filled (no zero bits remaining).
    /// Estimates at this point are unreliable — the estimator has saturated.
    pub fn saturated(&self) -> bool {
        self.zero_bits() == 0
    }

    /// Reset the estimator to empty state.
    pub fn clear(&mut self) {
        self.bitmap.iter_mut().for_each(|w| *w = 0);
        self.observed = 0;
    }

    fn zero_bits(&self) -> usize {
        let full_words = self.bit_count / 64;
        let mut zeros = 0;
        for word in self.bitmap.iter().take(full_words) {
            zeros += word.count_zeros() as usize;
        }
        let remainder = self.bit_count % 64;
        if remainder > 0 {
            let mask = (1u64 << remainder) - 1;
            let occupied = (self.bitmap[full_words] & mask).count_ones() as usize;
            zeros += remainder - occupied;
        }
        zeros
    }
}

impl Default for CardinalityEstimator {
    fn default() -> Self {
        Self::new(4096)
    }
}

/// Independent metric profiles for service-specific behavioural baselines.
/// Maintains separate CountMoments per (service, direction, metric, calendar slot).
/// Also tracks bounded cardinality estimators for source and destination
/// distinct identities per service/direction/slot combination.
/// Provides a baseline governance state machine: establish → promote → review → retire.
/// The existing packet-only CalendarAssessment remains as a fallback.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricProfileStore {
    profiles: BTreeMap<ProfileKey, MetricAssessment>,
    source_cardinality: BTreeMap<(String, TrafficDirection, u64), CardinalityEstimator>,
    dest_cardinality: BTreeMap<(String, TrafficDirection, u64), CardinalityEstimator>,
}

impl MetricProfileStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Observe a clean value for a metric profile.
    /// Updates the moments and quality counters.
    pub fn observe(
        &mut self,
        service_id: String,
        direction: TrafficDirection,
        metric: CountMetric,
        calendar_slot: u64,
        value: u64,
        quality: ObservationQuality,
    ) {
        let key = ProfileKey::new(service_id, direction, metric, calendar_slot);
        let assessment = self
            .profiles
            .entry(key)
            .or_insert_with(|| MetricAssessment {
                metric,
                ..Default::default()
            });
        assessment.metric = metric;
        assessment.quality = quality;
        assessment.last_updated_secs = calendar_slot * 3600; // approximate
        match quality {
            ObservationQuality::Clean => {
                observe_clean(&mut assessment.moments, value);
                assessment.observed_packets = assessment.observed_packets.saturating_add(value);
                assessment.clean_observations = assessment.clean_observations.saturating_add(1);
            }
            ObservationQuality::Degraded => {
                assessment.degraded_observations =
                    assessment.degraded_observations.saturating_add(1);
            }
            ObservationQuality::Missing | ObservationQuality::Invalid => {
                // Do not update moments for missing/invalid
            }
        }
    }

    /// Observe a packet and byte count together for a service/direction/slot.
    /// This is the primary entry point for clean traffic observations.
    pub fn observe_volume(
        &mut self,
        service_id: String,
        direction: TrafficDirection,
        calendar_slot: u64,
        packets: u64,
        bytes: u64,
        quality: ObservationQuality,
    ) {
        self.observe(
            service_id.clone(),
            direction,
            CountMetric::Packets,
            calendar_slot,
            packets,
            quality,
        );
        self.observe(
            service_id,
            direction,
            CountMetric::Bytes,
            calendar_slot,
            bytes,
            quality,
        );
    }

    /// Observe source and destination keys for cardinality estimation.
    /// Only clean observations are counted. Keys are producer-side hashes;
    /// raw IP addresses never enter this path.
    pub fn observe_cardinality(
        &mut self,
        service_id: String,
        direction: TrafficDirection,
        calendar_slot: u64,
        source_key: Option<u64>,
        dest_key: Option<u64>,
        quality: ObservationQuality,
    ) {
        if quality != ObservationQuality::Clean {
            return;
        }
        let card_key = (service_id.clone(), direction, calendar_slot);
        if let Some(sk) = source_key {
            self.source_cardinality
                .entry(card_key.clone())
                .or_default()
                .observe(sk);
        }
        if let Some(dk) = dest_key {
            self.dest_cardinality
                .entry(card_key)
                .or_default()
                .observe(dk);
        }
    }

    /// Source cardinality estimate for a service/direction/slot.
    pub fn source_cardinality_estimate(
        &self,
        service_id: &str,
        direction: TrafficDirection,
        calendar_slot: u64,
    ) -> Option<CardinalityEstimate> {
        self.source_cardinality
            .get(&(service_id.into(), direction, calendar_slot))
            .map(|e| e.estimate())
    }

    /// Destination cardinality estimate for a service/direction/slot.
    pub fn dest_cardinality_estimate(
        &self,
        service_id: &str,
        direction: TrafficDirection,
        calendar_slot: u64,
    ) -> Option<CardinalityEstimate> {
        self.dest_cardinality
            .get(&(service_id.into(), direction, calendar_slot))
            .map(|e| e.estimate())
    }

    /// Observe protocol-specific packet counts.
    pub fn observe_protocol(
        &mut self,
        service_id: String,
        direction: TrafficDirection,
        calendar_slot: u64,
        protocol_packets: [(CountMetric, u64); 4],
        quality: ObservationQuality,
    ) {
        for (metric, value) in protocol_packets {
            self.observe(
                service_id.clone(),
                direction,
                metric,
                calendar_slot,
                value,
                quality,
            );
        }
    }

    /// Observe per-protocol packet counts using the existing Protocol classification array.
    /// Takes `[u64; 5]` aligned with Protocol variant positions: [Other=0, Tcp=1, Udp=2, Icmp=3, IcmpV6=4].
    /// Each protocol variant gets its own independent CountMoments profile. Does NOT update
    /// the general `Packets` profile — every protocol observation is scoped exclusively to its
    /// metric variant.
    pub fn observe_protocol_packets(
        &mut self,
        service_id: String,
        direction: TrafficDirection,
        calendar_slot: u64,
        protocol_packets: [u64; 5],
        quality: ObservationQuality,
    ) {
        let metrics = [
            CountMetric::OtherPackets,
            CountMetric::TcpPackets,
            CountMetric::UdpPackets,
            CountMetric::IcmpPackets,
            CountMetric::IcmpV6Packets,
        ];
        for (idx, &count) in protocol_packets.iter().enumerate() {
            if count == 0 {
                continue;
            }
            let metric = metrics[idx];
            self.observe(
                service_id.clone(),
                direction,
                metric,
                calendar_slot,
                count,
                quality,
            );
        }
    }

    /// Observe ingress traffic (packets and bytes).
    /// Direction is sourced from the observation, not inferred from ports.
    pub fn observe_ingress(
        &mut self,
        service_id: String,
        calendar_slot: u64,
        packets: u64,
        bytes: u64,
        quality: ObservationQuality,
    ) {
        self.observe_volume(
            service_id,
            TrafficDirection::Ingress,
            calendar_slot,
            packets,
            bytes,
            quality,
        );
    }

    /// Observe egress traffic (packets and bytes).
    /// Direction is sourced from the observation, not inferred from ports.
    pub fn observe_egress(
        &mut self,
        service_id: String,
        calendar_slot: u64,
        packets: u64,
        bytes: u64,
        quality: ObservationQuality,
    ) {
        self.observe_volume(
            service_id,
            TrafficDirection::Egress,
            calendar_slot,
            packets,
            bytes,
            quality,
        );
    }

    /// Observe internal traffic (packets and bytes).
    /// Direction is sourced from the observation, not inferred from ports.
    pub fn observe_internal(
        &mut self,
        service_id: String,
        calendar_slot: u64,
        packets: u64,
        bytes: u64,
        quality: ObservationQuality,
    ) {
        self.observe_volume(
            service_id,
            TrafficDirection::Internal,
            calendar_slot,
            packets,
            bytes,
            quality,
        );
    }

    /// Get a cloned assessment for a specific metric profile.
    pub fn assessment(
        &self,
        service_id: &str,
        direction: TrafficDirection,
        metric: CountMetric,
        calendar_slot: u64,
    ) -> Option<MetricAssessment> {
        let key = ProfileKey::new(service_id.into(), direction, metric, calendar_slot);
        self.profiles.get(&key).cloned()
    }

    /// Get all metric assessments for a service/direction/slot.
    pub fn all_assessments(
        &self,
        service_id: &str,
        direction: TrafficDirection,
        calendar_slot: u64,
    ) -> Vec<MetricAssessment> {
        let mut results = Vec::new();
        for metric in [
            CountMetric::Packets,
            CountMetric::Bytes,
            CountMetric::TcpPackets,
            CountMetric::UdpPackets,
            CountMetric::IcmpPackets,
            CountMetric::IcmpV6Packets,
            CountMetric::OtherPackets,
        ] {
            if let Some(a) = self.assessment(service_id, direction, metric, calendar_slot) {
                results.push(a);
            }
        }
        results
    }

    /// Prune metric and cardinality state older than the caller's retention boundary.
    /// The caller supplies an absolute UTC hour so this store follows DiscoveryConfig.
    pub fn prune_before(&mut self, oldest_hour: u64) {
        self.profiles
            .retain(|key, _| key.calendar_slot >= oldest_hour);
        self.source_cardinality
            .retain(|(_, _, hour), _| *hour >= oldest_hour);
        self.dest_cardinality
            .retain(|(_, _, hour), _| *hour >= oldest_hour);
    }

    /// Number of distinct profiles stored.
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;

    #[test]
    fn cardinality_estimator_handles_zero_and_partial_word_sizes() {
        let mut empty = CardinalityEstimator::new(0);
        assert_eq!(empty.bit_count(), 1);
        empty.observe(42);
        assert_eq!(empty.zero_bits(), 0);
        assert!(empty.saturated());

        let mut partial = CardinalityEstimator::new(7);
        assert_eq!(partial.zero_bits(), 7);
        partial.observe(42);
        assert_eq!(partial.zero_bits(), 6);
        assert!(!partial.saturated());
    }

    #[test]
    fn pruning_removes_expired_metric_and_cardinality_profiles() {
        let mut store = MetricProfileStore::new();
        store.observe(
            "service-a".into(),
            TrafficDirection::Ingress,
            CountMetric::Packets,
            10,
            100,
            ObservationQuality::Clean,
        );
        store.observe_cardinality(
            "service-a".into(),
            TrafficDirection::Ingress,
            10,
            Some(1),
            Some(2),
            ObservationQuality::Clean,
        );
        store.observe(
            "service-a".into(),
            TrafficDirection::Ingress,
            CountMetric::Packets,
            20,
            120,
            ObservationQuality::Clean,
        );
        store.observe_cardinality(
            "service-a".into(),
            TrafficDirection::Ingress,
            20,
            Some(3),
            Some(4),
            ObservationQuality::Clean,
        );

        store.prune_before(20);

        assert!(
            store
                .assessment(
                    "service-a",
                    TrafficDirection::Ingress,
                    CountMetric::Packets,
                    10
                )
                .is_none()
        );
        assert!(
            store
                .assessment(
                    "service-a",
                    TrafficDirection::Ingress,
                    CountMetric::Packets,
                    20
                )
                .is_some()
        );
        assert!(
            store
                .source_cardinality_estimate("service-a", TrafficDirection::Ingress, 10)
                .is_none()
        );
        assert!(
            store
                .source_cardinality_estimate("service-a", TrafficDirection::Ingress, 20)
                .is_some()
        );
    }
}

// ── Baseline governance state machine ──
// Baseline lifecycle: Pending → Established → Promoted (or RequiresReview)
// → Retired. Once Retired, the baseline is no longer used for assessment.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BaselineState {
    /// Baseline is being established; not yet trusted for anomaly detection.
    #[default]
    Pending,
    /// Baseline established with sufficient clean observations; used in assess_*.
    Established,
    /// Baseline promoted — approved for production anomaly gating.
    Promoted,
    /// Baseline requires human review before further use.
    RequiresReview,
    /// Baseline retired — superseded or withdrawn.
    Retired,
}

/// Transitions a baseline state. Returns Ok(new_state) or Err(reason) if the
/// transition is invalid.
#[allow(dead_code)]
pub fn baseline_transition(
    current: BaselineState,
    event: BaselineEvent,
) -> Result<BaselineState, &'static str> {
    use BaselineEvent::*;
    Ok(match (current, event) {
        (BaselineState::Pending, Establish) => BaselineState::Established,
        (BaselineState::Pending, Retire) => BaselineState::Retired,

        (BaselineState::Established, Promote) => BaselineState::Promoted,
        (BaselineState::Established, RequireReview) => BaselineState::RequiresReview,
        (BaselineState::Established, Retire) => BaselineState::Retired,

        (BaselineState::Promoted, RequireReview) => BaselineState::RequiresReview,
        (BaselineState::Promoted, Retire) => BaselineState::Retired,

        (BaselineState::RequiresReview, Establish) => BaselineState::Established,
        (BaselineState::RequiresReview, Retire) => BaselineState::Retired,

        (BaselineState::Retired, _) => return Err("retired baseline cannot transition"),
        (BaselineState::Established, Establish) => return Err("already established"),
        (BaselineState::Promoted, Promote) => return Err("already promoted"),
        (_current, _) => return Err("invalid transition"),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BaselineEvent {
    Establish,
    Promote,
    RequireReview,
    Retire,
}

/// Governance state per profile key — tracks the baseline lifecycle for a
/// (service, direction, metric, calendar slot) profile.
#[derive(Debug, Clone, Default)]
pub struct BaselineGovernance {
    states: std::collections::BTreeMap<ProfileKey, BaselineState>,
}

impl BaselineGovernance {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that a baseline has been established for the given profile.
    #[allow(dead_code)]
    pub fn establish(&mut self, key: &ProfileKey) -> Result<BaselineState, &'static str> {
        let current = self.states.entry(key.clone()).or_default();
        let next = baseline_transition(*current, BaselineEvent::Establish)?;
        *current = next;
        Ok(next)
    }

    /// Promote a baseline to promoted state.
    #[allow(dead_code)]
    pub fn promote(&mut self, key: &ProfileKey) -> Result<BaselineState, &'static str> {
        let current = self.states.entry(key.clone()).or_default();
        let next = baseline_transition(*current, BaselineEvent::Promote)?;
        *current = next;
        Ok(next)
    }

    /// Mark a baseline as requiring review.
    #[allow(dead_code)]
    pub fn require_review(&mut self, key: &ProfileKey) -> Result<BaselineState, &'static str> {
        let current = self.states.entry(key.clone()).or_default();
        let next = baseline_transition(*current, BaselineEvent::RequireReview)?;
        *current = next;
        Ok(next)
    }

    /// Retire a baseline.
    #[allow(dead_code)]
    pub fn retire(&mut self, key: &ProfileKey) -> Result<BaselineState, &'static str> {
        let current = self.states.entry(key.clone()).or_default();
        let next = baseline_transition(*current, BaselineEvent::Retire)?;
        *current = next;
        Ok(next)
    }

    /// Get the current state of a baseline.
    pub fn state(&self, key: &ProfileKey) -> BaselineState {
        self.states.get(key).copied().unwrap_or_default()
    }

    /// True if the baseline is usable for anomaly assessment (Established or Promoted).
    #[allow(dead_code)]
    pub fn is_usable(&self, key: &ProfileKey) -> bool {
        matches!(
            self.state(key),
            BaselineState::Established | BaselineState::Promoted
        )
    }

    /// True if the baseline is promoted (approved for production gating).
    #[allow(dead_code)]
    pub fn is_promoted(&self, key: &ProfileKey) -> bool {
        self.state(key) == BaselineState::Promoted
    }
}

#[cfg(test)]
mod baseline_governance_tests {
    use super::*;

    fn key() -> ProfileKey {
        ProfileKey::new(
            "svc-a".into(),
            TrafficDirection::Ingress,
            CountMetric::Packets,
            0,
        )
    }

    #[test]
    fn pending_establishes_to_established() {
        let mut gov = BaselineGovernance::new();
        let key = key();
        assert_eq!(gov.state(&key), BaselineState::Pending);
        assert_eq!(gov.establish(&key).unwrap(), BaselineState::Established);
    }

    #[test]
    fn established_promotes() {
        let mut gov = BaselineGovernance::new();
        let key = key();
        gov.establish(&key).unwrap();
        assert_eq!(gov.promote(&key).unwrap(), BaselineState::Promoted);
    }

    #[test]
    fn established_requires_review() {
        let mut gov = BaselineGovernance::new();
        let key = key();
        gov.establish(&key).unwrap();
        assert_eq!(
            gov.require_review(&key).unwrap(),
            BaselineState::RequiresReview
        );
    }

    #[test]
    fn review_can_reestablish() {
        let mut gov = BaselineGovernance::new();
        let key = key();
        gov.establish(&key).unwrap();
        gov.require_review(&key).unwrap();
        assert_eq!(gov.establish(&key).unwrap(), BaselineState::Established);
    }

    #[test]
    fn retired_cannot_transition() {
        let mut gov = BaselineGovernance::new();
        let key = key();
        gov.establish(&key).unwrap();
        gov.retire(&key).unwrap();
        assert!(gov.establish(&key).is_err());
        assert!(gov.promote(&key).is_err());
    }

    #[test]
    fn is_usable_for_established_and_promoted() {
        let mut gov = BaselineGovernance::new();
        let key = key();
        assert!(!gov.is_usable(&key));
        gov.establish(&key).unwrap();
        assert!(gov.is_usable(&key));
        gov.promote(&key).unwrap();
        assert!(gov.is_usable(&key));
        gov.require_review(&key).unwrap();
        assert!(!gov.is_usable(&key));
    }
}
