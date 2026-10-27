use super::*;

/// Positive migration default for adaptive CUSUM variance.
/// Keep this in storage to avoid a dependency cycle on ramshield-detection.
fn default_adaptive_variance() -> f64 {
    0.5
}

/// Incremental traffic counters — updated on batch flush, read by forecasting
/// without scanning the full store (Kafka-style consumer lag / Prometheus counters).
#[derive(Debug)]
pub struct TrafficCounters {
    pub events_last_second: AtomicU64,
    pub unique_ips_window: AtomicU64,
    pub promoted_ips: AtomicU64,
    /// Subnet event counts from the latest flush window (for entropy at scale).
    /// Lock-free atomic array for concurrent reads/writes from detection and forecasting.
    pub subnet_window: [AtomicU64; 256],
    /// High-threat IPs from latest flush (bounded sample for preemptive block).
    /// Lock-free unbounded MPMC queue.
    pub threat_sample: SegQueue<(IpAddr, f32)>,
    /// RAM limit in MB from config.
    pub ram_limit_mb: AtomicUsize,
    /// Byte-precise usage tracking (crate port — complements ram_bytes estimate).
    pub used_bytes: AtomicU64,
    /// Process uptime in seconds.
    pub uptime_secs: AtomicU64,
}

impl TrafficCounters {
    pub fn new() -> Self {
        Self {
            events_last_second: AtomicU64::new(0),
            unique_ips_window: AtomicU64::new(0),
            promoted_ips: AtomicU64::new(0),
            subnet_window: std::array::from_fn(|_| AtomicU64::new(0)),
            threat_sample: SegQueue::new(),
            ram_limit_mb: AtomicUsize::new(0),
            used_bytes: AtomicU64::new(0),
            uptime_secs: AtomicU64::new(0),
        }
    }

    pub fn record_flush(&self, total_events: u64, unique_ips: u64, subnet_counts: &[u64]) {
        self.events_last_second
            .store(total_events, Ordering::Relaxed);
        self.unique_ips_window.store(unique_ips, Ordering::Relaxed);
        // Snapshot semantics: this flush's counts fully replace the previous
        // window. Only zero the slots BEYOND the incoming range — slots
        // inside the range are about to be overwritten with the new value
        // anyway. Saves ~256 atomic stores on every flush when the
        // subnet count is well under 256.
        let n = subnet_counts.len().min(256);
        for slot in &self.subnet_window[n..] {
            slot.store(0, Ordering::Relaxed);
        }
        for (i, count) in subnet_counts.iter().take(256).enumerate() {
            self.subnet_window[i].store(*count, Ordering::Relaxed);
        }
    }

    /// Push multiple threat samples into the queue.
    pub fn push_threat_samples(&self, samples: Vec<(IpAddr, f32)>) {
        for item in samples {
            // P1-9: bound the queue. The only drainer is the forecaster (may
            // be disabled/stalled); without a cap a sustained feed grows
            // ~180MB/hr. 1024 samples ≈ 8 flushes of slack — the newest
            // threat signals win, stale ones are dropped.
            if self.threat_sample.len() >= 1024 {
                break;
            }
            self.threat_sample.push(item);
        }
    }

    /// Atomically drain the threat sample queue.
    pub fn drain_threat_sample(&self) -> Vec<(IpAddr, f32)> {
        let mut sample = Vec::with_capacity(self.threat_sample.len());
        while let Some(item) = self.threat_sample.pop() {
            sample.push(item);
        }
        sample
    }
}

impl Default for TrafficCounters {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    Counter(u64),
    /// Small payloads stored as Vec<u8>.
    /// Note: we intentionally avoid [u8; 64] because serde only auto-derives
    /// fixed arrays up to [T; 32]. Vec<u8> is serde-compatible at any size.
    Inline(Vec<u8>),
    Blob(Vec<u8>),
    IpRecord(IpRecord),
    SubnetRecord(SubnetRecord),
}

impl Value {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() <= INLINE_MAX {
            Value::Inline(bytes.to_vec())
        } else {
            Value::Blob(bytes.to_vec())
        }
    }

    pub fn heap_bytes(&self) -> usize {
        match self {
            Value::Inline(v) => v.len(),
            Value::Blob(v) => v.len(),
            // P1 fix: IpRecord/SubnetRecord live INSIDE the enum variant —
            // size_of::<Entry>() already accounts for them. Returning
            // size_of::<IpRecord>() here double-counted ~136 B per blocked
            // IP, starving the RAM budget to roughly half its real size.
            // Both structs are pure-value ([u32; 5], [u64; 4], fieldless
            // enums) — zero heap side-allocation, so 0 is exact.
            Value::IpRecord(_) => 0,
            Value::SubnetRecord(_) => 0,
            _ => 0,
        }
    }

    pub fn is_blocked(&self) -> bool {
        matches!(self, Value::IpRecord(rec) if rec.block_state != BlockState::Clean)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpRecord {
    pub ip: IpAddr,
    pub request_count: u64,
    pub ewma_rps: f64,
    /// P1 CUSUM accumulator (Page 1954) — sustained sub-threshold drift.
    #[serde(default)]
    pub cusum_s: f64,
    /// Slow-EWMA baseline the CUSUM measures deviation from.
    #[serde(default)]
    pub baseline_rps: f64,
    /// Debounce latch: previous sample was over threshold.
    #[serde(default)]
    pub prev_sample_hot: bool,
    /// Flush samples observed (saturating) — gates CUSUM warm-up. u8 is plenty:
    /// 6 samples to arm, saturates long before overflow matters.
    #[serde(default)]
    pub sample_count: u8,
    /// Consecutive samples breaching the opt-in relative detector.
    /// Persisted with the record so detector state does not live in a side map.
    #[serde(default)]
    pub relative_breach_streak: u8,
    /// Sliding-window count of distinct over-threshold batch samples.
    /// Resets on window expiry or block. ponytail: u8 caps at 255.
    #[serde(default)]
    pub pulse_samples_in_window: u8,
    /// Earliest pulse-sample timestamp in the current sliding window (ns).
    /// 0 = window not yet opened. Resets on expiry or block.
    #[serde(default)]
    pub pulse_window_start_ns: u64,
    /// Adaptive detector's independent baseline (legacy baseline remains unchanged).
    #[serde(default)]
    pub adaptive_baseline_rps: f64,
    /// Adaptive detector's independent CUSUM accumulator.
    #[serde(default)]
    pub adaptive_cusum_s: f64,
    /// Adaptive detector's accepted-sample count, independent of legacy detector state.
    #[serde(default)]
    pub adaptive_sample_count: u64,
    /// Adaptive CUSUM variance, persisted with the IP record.
    #[serde(default = "default_adaptive_variance")]
    pub adaptive_variance: f64,
    /// Last monotonic timestamp accepted by the adaptive detector.
    #[serde(default)]
    pub adaptive_last_bucket_ns: u64,
    /// Consecutive recovery evidence for the adaptive detector.
    #[serde(default)]
    pub adaptive_recovery_count: u64,
    /// Persisted adaptive mode: 0 = warming up, 1 = monitoring, 2 = breached.
    #[serde(default)]
    pub adaptive_mode: u8,
    /// Persisted one-second quantization bucket for adaptive CUSUM.
    #[serde(default)]
    pub adaptive_bucket_id: u64,
    /// Packet/event count accumulated in the current adaptive bucket.
    #[serde(default)]
    pub adaptive_bucket_packets: u64,
    /// Distinguishes a real bucket zero from an uninitialized record.
    #[serde(default)]
    pub adaptive_bucket_initialized: bool,
    pub first_seen_ns: u64,
    pub last_seen_ns: u64,
    pub bytes_in: u64,
    pub status_dist: [u32; 5],
    pub proto_fingerprint: u32,
    pub threat_score: f32,
    pub block_state: BlockState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BlockState {
    Clean,
    Suspicious,
    Blocked { reason: BlockReason, since_ns: u64 },
}

/// Subnet aggregate. `network` carries family-complete CIDR metadata
/// (v4 /24, v6 /64) — the single source for display (Task 1: replaces the
/// old v4-shaped `prefix: [u8;3]` that rendered v6 as garbage).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubnetRecord {
    pub network: IpNetwork,
    pub total_rps: u64,
    /// Distinct-source signal for the current window (v4 only): 256-bit map of
    /// seen host octets, 32 B flat. The real swarm signal — one abuser at 500
    /// events is a single offender; 40 distinct IPs × 12 events is an attack.
    /// v6 /64s are too large to bitmap; the batch gate reads exact
    /// `subnet_index` cardinality for them instead (IPv6 plan D1), so
    /// `unique_ips()` returning 0 only means "not the v4 fast path".
    pub host_bitmap: [u64; 4],
    pub last_updated_ns: u64,
}

impl SubnetRecord {
    #[inline]
    pub fn unique_ips(&self) -> u64 {
        self.host_bitmap.iter().map(|w| w.count_ones() as u64).sum()
    }

    #[inline]
    pub(crate) fn mark_host_v4(&mut self, ip: std::net::IpAddr) {
        if let std::net::IpAddr::V4(v4) = ip {
            let o = v4.octets()[3] as usize;
            self.host_bitmap[o / 64] |= 1 << (o % 64);
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub value: Value,
    pub expires_at: Option<Instant>,
}

impl Entry {
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|e| Instant::now() > e)
    }
}

#[cfg(test)]
mod adaptive_migration_tests {
    use super::*;

    #[test]
    fn legacy_record_deserializes_with_safe_adaptive_defaults() {
        let record = IpRecord {
            ip: "192.0.2.1".parse().unwrap(),
            request_count: 0,
            ewma_rps: 0.0,
            cusum_s: 0.0,
            baseline_rps: 0.0,
            prev_sample_hot: false,
            sample_count: 0,
            relative_breach_streak: 0,
            pulse_samples_in_window: 0,
            pulse_window_start_ns: 0,
            adaptive_baseline_rps: 0.0,
            adaptive_cusum_s: 0.0,
            adaptive_sample_count: 0,
            adaptive_variance: default_adaptive_variance(),
            adaptive_last_bucket_ns: 0,
            adaptive_recovery_count: 0,
            adaptive_mode: 0,
            adaptive_bucket_id: 0,
            adaptive_bucket_packets: 0,
            adaptive_bucket_initialized: false,
            first_seen_ns: 0,
            last_seen_ns: 0,
            bytes_in: 0,
            status_dist: [0; 5],
            proto_fingerprint: 0,
            threat_score: 0.0,
            block_state: BlockState::Clean,
        };
        let mut legacy = serde_json::to_value(record).unwrap();
        let object = legacy.as_object_mut().unwrap();
        for field in [
            "adaptive_baseline_rps",
            "adaptive_cusum_s",
            "adaptive_sample_count",
            "adaptive_variance",
            "adaptive_last_bucket_ns",
            "adaptive_recovery_count",
            "adaptive_mode",
            "adaptive_bucket_id",
            "adaptive_bucket_packets",
            "adaptive_bucket_initialized",
        ] {
            object.remove(field);
        }

        let restored: IpRecord = serde_json::from_value(legacy).unwrap();
        assert_eq!(restored.adaptive_variance, default_adaptive_variance());
        assert!(restored.adaptive_variance.is_finite());
        assert!(restored.adaptive_variance > 0.0);
        assert_eq!(restored.adaptive_sample_count, 0);
        assert_eq!(restored.adaptive_mode, 0);
        assert!(!restored.adaptive_bucket_initialized);
    }
}
