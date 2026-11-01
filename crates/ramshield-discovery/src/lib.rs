//! Passive production-environment discovery.
//! This crate has no dependency on detection or enforcement. It accepts
//! normalized observations, keeps bounded hourly aggregates, and persists
//! them atomically. It never changes live configuration or network state.

use serde::{Deserialize, Serialize};
use std::{fs::{self, File, OpenOptions}, io::{self, Write}, path::{Path, PathBuf}};

const SCHEMA: u32 = 1;
const HOURS_PER_DAY: usize = 24;
const SOURCE_BUCKETS: usize = 64;
const DESTINATION_BUCKETS: usize = 64;
const MAX_PORTS: usize = 256;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[repr(usize)]
pub enum Protocol { Other = 0, Tcp = 1, Udp = 2, Icmp = 3, IcmpV6 = 4 }

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
    fn default() -> Self { Self { observation_days: 90, max_ports_per_hour: 64 } }
}
impl DiscoveryConfig {
    fn validate(&self) -> io::Result<()> {
        if !(1..=365).contains(&self.observation_days) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "observation_days must be 1..=365"));
        }
        if !(1..=MAX_PORTS).contains(&self.max_ports_per_hour) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "max_ports_per_hour must be 1..=256"));
        }
        Ok(())
    }
    fn bucket_count(&self) -> usize { usize::from(self.observation_days) * HOURS_PER_DAY }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PortCounter { port: u16, count: u64, error_bound: u64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HourBucket {
    hour: u64, samples: u64, packets: u64, bytes: u64,
    protocols: [u64; 5], ports: Vec<PortCounter>,
    sources: [u64; SOURCE_BUCKETS], destinations: [u64; DESTINATION_BUCKETS],
    capture_drops: u64, ingest_drops: u64, malformed: u64,
    queue_depth_peak: u64, queue_capacity_peak: u64,
    cpu_millis: u64, memory_bytes_peak: u64,
}
impl HourBucket {
    fn new(hour: u64) -> Self {
        Self { hour, samples: 0, packets: 0, bytes: 0, protocols: [0; 5],
            ports: Vec::new(), sources: [0; SOURCE_BUCKETS],
            destinations: [0; DESTINATION_BUCKETS], capture_drops: 0,
            ingest_drops: 0, malformed: 0, queue_depth_peak: 0,
            queue_capacity_peak: 0, cpu_millis: 0, memory_bytes_peak: 0 }
    }
    fn add_port(&mut self, port: u16, limit: usize) {
        if let Some(p) = self.ports.iter_mut().find(|p| p.port == port) {
            p.count = p.count.saturating_add(1); return;
        }
        if self.ports.len() < limit {
            self.ports.push(PortCounter { port, count: 1, error_bound: 0 }); return;
        }
        if let Some((idx, min)) = self.ports.iter().enumerate().min_by_key(|(_, p)| p.count) {
            let bound = min.count;
            self.ports[idx] = PortCounter { port, count: bound.saturating_add(1), error_bound: bound };
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedState {
    schema: u32, observation_days: u16, buckets: Vec<Option<HourBucket>>,
    accepted: u64, too_old: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortSummary { pub port: u16, pub count: u64, pub error_bound: u64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourSummary {
    pub hour: u64, pub samples: u64, pub packets: u64, pub bytes: u64,
    pub protocols: [u64; 5], pub top_ports: Vec<PortSummary>,
    pub source_distribution: [u64; SOURCE_BUCKETS],
    pub destination_distribution: [u64; DESTINATION_BUCKETS],
    pub capture_drops: u64, pub ingest_drops: u64, pub malformed_observations: u64,
    pub queue_depth_peak: u64, pub queue_capacity_peak: u64,
    pub cpu_millis: u64, pub memory_bytes_peak: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverySnapshot {
    pub schema_version: u32, pub observation_days: u16,
    pub observations_accepted: u64, pub observations_too_old: u64,
    pub oldest_hour: Option<u64>, pub newest_hour: Option<u64>,
    pub hours: Vec<HourSummary>,
}

pub struct Discovery { config: DiscoveryConfig, state: PersistedState, path: PathBuf }

impl Discovery {
    pub fn open(path: impl AsRef<Path>, config: DiscoveryConfig) -> io::Result<Self> {
        config.validate()?;
        let path = path.as_ref().to_path_buf();
        let state = if path.exists() {
            let bytes = fs::read(&path)?;
            let state: PersistedState = serde_json::from_slice(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if state.schema != SCHEMA || state.observation_days != config.observation_days
                || state.buckets.len() != config.bucket_count() {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "discovery state schema/configuration mismatch"));
            }
            state
        } else {
            PersistedState { schema: SCHEMA, observation_days: config.observation_days,
                buckets: vec![None; config.bucket_count()], accepted: 0, too_old: 0 }
        };
        Ok(Self { config, state, path })
    }

    /// Aggregate a normalized observation. No enforcement or detection action is possible here.
    pub fn observe(&mut self, sample: TrafficSample) {
        let hour = sample.timestamp_secs / 3600;
        let Some(bucket) = self.bucket_mut(hour) else {
            self.state.too_old = self.state.too_old.saturating_add(1); return;
        };
        bucket.samples = bucket.samples.saturating_add(1);
        bucket.packets = bucket.packets.saturating_add(sample.packets);
        bucket.bytes = bucket.bytes.saturating_add(sample.bytes);
        let protocol = sample.protocol as usize;
        bucket.protocols[protocol] = bucket.protocols[protocol].saturating_add(sample.packets);
        if let Some(port) = sample.destination_port { bucket.add_port(port, self.config.max_ports_per_hour); }
        if let Some(key) = sample.source_key {
            let i = key as usize % SOURCE_BUCKETS;
            bucket.sources[i] = bucket.sources[i].saturating_add(sample.packets);
        }
        if let Some(key) = sample.destination_key {
            let i = key as usize % DESTINATION_BUCKETS;
            bucket.destinations[i] = bucket.destinations[i].saturating_add(sample.packets);
        }
        self.state.accepted = self.state.accepted.saturating_add(1);
    }

    pub fn observe_health(&mut self, sample: HealthSample) {
        let Some(bucket) = self.bucket_mut(sample.timestamp_secs / 3600) else { return; };
        bucket.capture_drops = bucket.capture_drops.saturating_add(sample.capture_drops);
        bucket.ingest_drops = bucket.ingest_drops.saturating_add(sample.ingest_drops);
        bucket.malformed = bucket.malformed.saturating_add(sample.malformed_observations);
        bucket.queue_depth_peak = bucket.queue_depth_peak.max(sample.queue_depth);
        bucket.queue_capacity_peak = bucket.queue_capacity_peak.max(sample.queue_capacity);
        bucket.cpu_millis = bucket.cpu_millis.saturating_add(sample.cpu_millis);
        bucket.memory_bytes_peak = bucket.memory_bytes_peak.max(sample.memory_bytes);
    }

    /// Atomic snapshot persistence: temp file, file fsync, rename, directory fsync.
    pub fn flush(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() { fs::create_dir_all(parent)?; }
        let bytes = serde_json::to_vec(&self.state)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut tmp = self.path.clone();
        let ext = self.path.extension().and_then(|s| s.to_str()).unwrap_or("json");
        tmp.set_extension(format!("{ext}.tmp"));
        let mut file = OpenOptions::new().create(true).truncate(true).write(true).open(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, &self.path)?;
        if let Some(parent) = self.path.parent() { File::open(parent)?.sync_all()?; }
        Ok(())
    }

    pub fn snapshot(&self) -> DiscoverySnapshot {
        let mut buckets: Vec<_> = self.state.buckets.iter().flatten().collect();
        buckets.sort_by_key(|b| b.hour);
        let hours = buckets.iter().map(|b| HourSummary {
            hour: b.hour, samples: b.samples, packets: b.packets, bytes: b.bytes,
            protocols: b.protocols,
            top_ports: b.ports.iter().map(|p| PortSummary { port: p.port, count: p.count, error_bound: p.error_bound }).collect(),
            source_distribution: b.sources, destination_distribution: b.destinations,
            capture_drops: b.capture_drops, ingest_drops: b.ingest_drops,
            malformed_observations: b.malformed, queue_depth_peak: b.queue_depth_peak,
            queue_capacity_peak: b.queue_capacity_peak, cpu_millis: b.cpu_millis,
            memory_bytes_peak: b.memory_bytes_peak,
        }).collect();
        DiscoverySnapshot { schema_version: SCHEMA, observation_days: self.config.observation_days,
            observations_accepted: self.state.accepted, observations_too_old: self.state.too_old,
            oldest_hour: buckets.first().map(|b| b.hour), newest_hour: buckets.last().map(|b| b.hour), hours }
    }

    fn bucket_mut(&mut self, hour: u64) -> Option<&mut HourBucket> {
        if let Some(newest) = self.state.buckets.iter().flatten().map(|b| b.hour).max() {
            let oldest = newest.saturating_sub(self.config.bucket_count() as u64 - 1);
            if hour < oldest { return None; }
        }
        let len = self.state.buckets.len();
        let index = hour as usize % len;
        if self.state.buckets[index].as_ref().is_none_or(|b| b.hour != hour) {
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
        let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("rs-discovery-{n}.json"))
    }
    fn sample(t: u64) -> TrafficSample {
        TrafficSample { timestamp_secs: t, packets: 3, bytes: 300, protocol: Protocol::Tcp,
            destination_port: Some(443), source_key: Some(17), destination_key: Some(29) }
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
        { let mut d = Discovery::open(&p, DiscoveryConfig::default()).unwrap(); d.observe(sample(1_700_000_000)); d.flush().unwrap(); }
        let d = Discovery::open(&p, DiscoveryConfig::default()).unwrap();
        assert_eq!(d.snapshot().observations_accepted, 1);
        let _ = fs::remove_file(p);
    }
    #[test]
    fn retention_and_port_cardinality_are_bounded() {
        let p = path();
        let cfg = DiscoveryConfig { observation_days: 1, max_ports_per_hour: 2 };
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
}
