use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::System;

const HISTORY: usize = 80;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn with_system<F, R>(f: F) -> R
where
    F: FnOnce(&mut System) -> R,
{
    static SYS: Mutex<Option<System>> = Mutex::new(None);
    let mut guard = SYS.lock().unwrap();
    if guard.is_none() {
        *guard = Some(System::new_all());
    }
    f(guard.as_mut().unwrap())
}

/// (cpu_usage, total_ram_mb, own_process_rss_mb). Cached 1s — see get_system_usage.
pub fn get_system_usage() -> (f32, usize, usize) {
    // ponytail: 1s TTL cache — dashboard polls snapshot+modules per cycle and
    // both need the same numbers; upgrade to crossbeam channel ticker if
    // sub-second freshness ever matters.
    static CACHE: Mutex<Option<(std::time::Instant, f32, usize, usize)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap();
    if let Some((_, cpu, mem, rss)) =
        (*cache).filter(|(at, ..)| at.elapsed() < std::time::Duration::from_secs(1))
    {
        return (cpu, mem, rss);
    }
    let fresh = with_system(|sys| {
        // CPU% needs two samples spaced ~200ms+; refresh_specifics avoids the
        // full process-table walk of refresh_all() on every dashboard poll.
        sys.refresh_specifics(
            sysinfo::RefreshKind::nothing()
                .with_cpu(sysinfo::CpuRefreshKind::nothing().with_cpu_usage())
                .with_memory(sysinfo::MemoryRefreshKind::everything()),
        );
        let cpu_usage = sys.global_cpu_usage();
        // sysinfo 0.30+: total_memory() returns bytes (was KB before).
        let total_memory_mb = (sys.total_memory() / (1024 * 1024)) as usize;
        let rss_mb = sys
            .process(
                sysinfo::get_current_pid()
                    .ok()
                    .unwrap_or(sysinfo::Pid::from(0)),
            )
            .map(|p| p.memory() / (1024 * 1024))
            .unwrap_or(0) as usize;
        (cpu_usage, total_memory_mb, rss_mb)
    });
    *cache = Some((std::time::Instant::now(), fresh.0, fresh.1, fresh.2));
    fresh
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRecord {
    pub ts_ms: u64,
    pub events: u32,
    pub unique_ips: u32,
    /// Unique IPs promoted to full tracking
    pub promoted: u32,
    /// Unique IPs skipped (below promotion threshold)
    pub cold_skipped: u32,
    /// Connection events in promoted IPs
    pub promoted_events: u32,
    /// Connection events in cold-skipped IPs
    pub cold_skipped_events: u32,
    pub blocks: u32,
    pub hot_subnets: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRecord {
    pub ts_ms: u64,
    pub ip: String,
    pub reason: String,
    pub module: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleStats {
    pub label: String,
    pub events: u64,
    pub errors: u64,
    pub rate_per_sec: f64,
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSnapshot {
    pub ts_ms: u64,
    pub uptime_secs: u64,
    pub ips_tracked: usize,
    pub blocked_total: u64,
    pub ram_bytes: usize,
    pub ram_limit_mb: usize,
    pub ram_pct: f64,
    pub cpu_usage: f32,
    pub memory_usage_mb: usize,
    pub total_ram_mb: usize,
    pub ipc_requests: u64,
    pub events_ingested: u64,
    pub events_rejected: u64,
    pub channel_depth: usize,
    pub batches_total: u64,
    pub promotions: u64,
    pub cold_skipped: u64,
    pub blocks_applied: u64,
    pub pipeline: PipelineFlow,
    pub is_healthy: bool,
    pub health_reason: String,
}

impl Default for DashboardSnapshot {
    fn default() -> Self {
        Self {
            ts_ms: 0,
            uptime_secs: 0,
            ips_tracked: 0,
            blocked_total: 0,
            ram_bytes: 0,
            ram_limit_mb: 0,
            ram_pct: 0.0,
            cpu_usage: 0.0,
            memory_usage_mb: 0,
            total_ram_mb: 0,
            ipc_requests: 0,
            events_ingested: 0,
            events_rejected: 0,
            channel_depth: 0,
            batches_total: 0,
            promotions: 0,
            cold_skipped: 0,
            blocks_applied: 0,
            pipeline: PipelineFlow {
                ingest: 0,
                queued: 0,
                batched: 0,
                promoted: 0,
                merged: 0,
                blocked: 0,
            },
            is_healthy: true,
            health_reason: "initializing".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubnetRow {
    pub prefix: String,
    pub events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineFlow {
    pub ingest: u64,
    pub queued: u64,
    pub batched: u64,
    pub promoted: u64,
    pub merged: u64,
    pub blocked: u64,
}

pub struct Metrics {
    pub requests_total: Arc<AtomicU64>,
    pub blocks_total: Arc<AtomicU64>,
    pub events_ingested: Arc<AtomicU64>,
    pub events_rejected: Arc<AtomicU64>,
    pub batches_total: Arc<AtomicU64>,
    pub promotions_total: Arc<AtomicU64>,
    pub cold_skipped_total: Arc<AtomicU64>,
    pub blocks_detection: Arc<AtomicU64>,
    pub blocks_subnet: Arc<AtomicU64>,
    pub blocks_forecast: Arc<AtomicU64>,
    pub forecast_ticks: Arc<AtomicU64>,
    pub entropy_ticks: Arc<AtomicU64>,
    pub hw_rps_bits: Arc<AtomicU64>,
    pub hw_z_bits: Arc<AtomicU64>,
    pub hw_forecast_bits: Arc<AtomicU64>,
    pub entropy_bits: Arc<AtomicU64>,
    pub last_batch_events: Arc<AtomicU64>,
    pub last_batch_promoted: Arc<AtomicU64>,
    pub last_batch_blocks: Arc<AtomicU64>,
    pub last_batch: Arc<Mutex<Option<Arc<BatchRecord>>>>,
    pub batch_history: Arc<Mutex<VecDeque<Arc<BatchRecord>>>>,
    pub block_log: Arc<Mutex<VecDeque<BlockRecord>>>,
    pub block_log_cap: usize,
    started_ms: u64,
}

impl Metrics {
    pub fn new() -> Self {
        Self::with_block_log(1_000)
    }

    /// `block_log_size`: ring size served by `/api/history/blocks`.
    /// Was a hardcoded 40 — useless during floods. Config-driven now
    /// (`[dashboard] block_log_size`, default 1000).
    pub fn with_block_log(block_log_size: usize) -> Self {
        Self {
            requests_total: Arc::new(AtomicU64::new(0)),
            blocks_total: Arc::new(AtomicU64::new(0)),
            events_ingested: Arc::new(AtomicU64::new(0)),
            events_rejected: Arc::new(AtomicU64::new(0)),
            batches_total: Arc::new(AtomicU64::new(0)),
            promotions_total: Arc::new(AtomicU64::new(0)),
            cold_skipped_total: Arc::new(AtomicU64::new(0)),
            blocks_detection: Arc::new(AtomicU64::new(0)),
            blocks_subnet: Arc::new(AtomicU64::new(0)),
            blocks_forecast: Arc::new(AtomicU64::new(0)),
            forecast_ticks: Arc::new(AtomicU64::new(0)),
            entropy_ticks: Arc::new(AtomicU64::new(0)),
            hw_rps_bits: Arc::new(AtomicU64::new(0)),
            hw_z_bits: Arc::new(AtomicU64::new(0)),
            hw_forecast_bits: Arc::new(AtomicU64::new(0)),
            entropy_bits: Arc::new(AtomicU64::new(0)),
            last_batch_events: Arc::new(AtomicU64::new(0)),
            last_batch_promoted: Arc::new(AtomicU64::new(0)),
            last_batch_blocks: Arc::new(AtomicU64::new(0)),
            last_batch: Arc::new(Mutex::new(None)),
            batch_history: Arc::new(Mutex::new(VecDeque::with_capacity(HISTORY))),
            block_log: Arc::new(Mutex::new(VecDeque::with_capacity(block_log_size.max(1)))),
            block_log_cap: block_log_size.max(1),
            started_ms: now_ms(),
        }
    }

    pub fn inc_requests(&self) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
    }
    pub fn inc_blocks(&self) {
        self.blocks_total.fetch_add(1, Ordering::Relaxed);
    }
    pub fn inc_ingested(&self, n: u64) {
        self.events_ingested.fetch_add(n, Ordering::Relaxed);
    }
    pub fn inc_rejected(&self, n: u64) {
        self.events_rejected.fetch_add(n, Ordering::Relaxed);
    }

    pub fn record_batch(&self, rec: BatchRecord) {
        self.batches_total.fetch_add(1, Ordering::Relaxed);
        self.last_batch_events
            .store(rec.events as u64, Ordering::Relaxed);
        self.last_batch_promoted
            .store(rec.promoted as u64, Ordering::Relaxed);
        self.last_batch_blocks
            .store(rec.blocks as u64, Ordering::Relaxed);
        self.promotions_total
            .fetch_add(rec.promoted as u64, Ordering::Relaxed);
        self.cold_skipped_total
            .fetch_add(rec.cold_skipped as u64, Ordering::Relaxed);
        self.blocks_detection
            .fetch_add(rec.blocks as u64, Ordering::Relaxed);
        // ponytail: Arc avoids the full BatchRecord clone (~64 B) on every batch.
        let shared = Arc::new(rec);
        if let Ok(mut h) = self.batch_history.lock() {
            if h.len() >= HISTORY {
                h.pop_front();
            }
            h.push_back(Arc::clone(&shared));
        }
        if let Ok(mut lb) = self.last_batch.lock() {
            *lb = Some(shared);
        }
    }

    pub fn record_block(&self, ip: &str, reason: &str, module: &str) {
        if let Ok(mut log) = self.block_log.lock() {
            while log.len() >= self.block_log_cap {
                log.pop_front();
            }
            log.push_back(BlockRecord {
                ts_ms: now_ms(),
                ip: ip.to_string(),
                reason: reason.to_string(),
                module: module.to_string(),
            });
        }
    }

    /// Record a block event with `IpAddr`, avoiding caller-side `to_string()`.
    #[inline]
    pub fn record_block_ip(&self, ip: &IpAddr, reason: &str, module: &str) {
        if let Ok(mut log) = self.block_log.lock() {
            while log.len() >= self.block_log_cap {
                log.pop_front();
            }
            // ponytail: avoid double-format of IpAddr — caller used to call
            // `record_block(&ip.to_string(), ...)` which allocated a throwaway
            // String just to feed it to `to_string()` again inside.
            log.push_back(BlockRecord {
                ts_ms: now_ms(),
                ip: ip.to_string(),
                reason: reason.to_string(),
                module: module.to_string(),
            });
        }
    }

    pub fn set_forecast_hw(&self, rps: f64, z: f64, forecast: f64) {
        self.hw_rps_bits.store(rps.to_bits(), Ordering::Relaxed);
        self.hw_z_bits.store(z.to_bits(), Ordering::Relaxed);
        self.hw_forecast_bits
            .store(forecast.to_bits(), Ordering::Relaxed);
        self.forecast_ticks.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_entropy(&self, h: f64) {
        self.entropy_bits.store(h.to_bits(), Ordering::Relaxed);
        self.entropy_ticks.fetch_add(1, Ordering::Relaxed);
    }

    fn f64(bits: &AtomicU64) -> f64 {
        f64::from_bits(bits.load(Ordering::Relaxed))
    }

    pub fn get_batch_history(&self) -> Vec<BatchRecord> {
        // ponytail: Arc::try_unwrap would copy if we're the sole holder; in
        // practice the deque + last_batch always hold one ref each, so 2 copies.
        // Cheap (64 B/record × 80 entries).
        self.batch_history
            .lock()
            .map(|h| h.iter().map(|a| (**a).clone()).collect())
            .unwrap_or_default()
    }

    pub fn get_block_log(&self) -> Vec<BlockRecord> {
        self.block_log
            .lock()
            .map(|h| h.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn get_module_stats_data(
        &self,
        _uptime_secs: u64,
        ingested: u64,
        _channel_depth: usize,
        ips_tracked: usize,
        ram_bytes: usize,
        ram_limit_mb: usize,
    ) -> Vec<ModuleStats> {
        let elapsed = ((now_ms().saturating_sub(self.started_ms)) as f64 / 1000.0).max(0.001);
        let batches = self.batches_total.load(Ordering::Relaxed);

        let hw_rps = Metrics::f64(&self.hw_rps_bits);
        let hw_z = Metrics::f64(&self.hw_z_bits);
        let hw_f = Metrics::f64(&self.hw_forecast_bits);
        let entropy = Metrics::f64(&self.entropy_bits);

        let last_ev = self.last_batch_events.load(Ordering::Relaxed);

        let (_cpu_usage, total_system_memory_mb, _rss) = get_system_usage();

        vec![
            ModuleStats {
                label: "IPC".into(),
                events: self.requests_total.load(Ordering::Relaxed),
                errors: self.events_rejected.load(Ordering::Relaxed),
                rate_per_sec: self.requests_total.load(Ordering::Relaxed) as f64 / elapsed,
                detail: serde_json::json!({
                    "ingested": ingested,
                    "rejected": self.events_rejected.load(Ordering::Relaxed),
                }),
            },
            ModuleStats {
                label: "Detection".into(),
                events: ingested,
                errors: 0,
                rate_per_sec: ingested as f64 / elapsed,
                detail: serde_json::json!({
                    "batches": batches,
                    "promotions": self.promotions_total.load(Ordering::Relaxed),
                    "cold_skipped": self.cold_skipped_total.load(Ordering::Relaxed),
                    "blocks": self.blocks_detection.load(Ordering::Relaxed),
                    "subnet_blocks": self.blocks_subnet.load(Ordering::Relaxed),
                    "last_batch_events": last_ev,
                }),
            },
            ModuleStats {
                label: "Forecasting".into(),
                events: self.forecast_ticks.load(Ordering::Relaxed)
                    + self.entropy_ticks.load(Ordering::Relaxed),
                errors: 0,
                rate_per_sec: self.forecast_ticks.load(Ordering::Relaxed) as f64 / elapsed,
                detail: serde_json::json!({
                    "hw_rps": hw_rps,
                    "hw_forecast": hw_f,
                    "hw_zscore": hw_z,
                    "entropy": entropy,
                    "forecast_blocks": self.blocks_forecast.load(Ordering::Relaxed),
                }),
            },
            ModuleStats {
                label: "Storage".into(),
                events: ips_tracked as u64,
                errors: 0,
                rate_per_sec: 0.0,
                detail: serde_json::json!({
                    "ram_mb": ram_bytes as f64 / (1024.0 * 1024.0),
                    "limit_mb": ram_limit_mb,
                    "ips_tracked": ips_tracked,
                    "total_system_memory_mb": total_system_memory_mb,
                }),
            },
        ]
    }
}

impl Metrics {
    /// Render all counters in Prometheus exposition format.
    /// ponytail: replaced dead stdout printer; upgrade path is
    /// metrics-exporter-prometheus if cardinality ever demands it.
    pub fn render_prometheus(&self) -> String {
        let elapsed = ((now_ms().saturating_sub(self.started_ms)) as f64 / 1000.0).max(0.001);

        macro_rules! emit {
            ($name:literal, $val:expr, $help:literal, $typ:literal) => {{
                let mut out = String::new();
                out.push_str(&format!("# HELP {} {}\n", $name, $help));
                out.push_str(&format!("# TYPE {} {}\n", $name, $typ));
                out.push_str(&format!("{} {}\n", $name, $val));
                out
            }};
        }

        let mut out = String::new();

        out.push_str(&emit!(
            "ramshield_uptime_seconds",
            elapsed as u64,
            "Process uptime in seconds.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_requests_total",
            self.requests_total.load(Ordering::Relaxed),
            "Total IPC requests received.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_blocks_total",
            self.blocks_total.load(Ordering::Relaxed),
            "Total blocks issued.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_events_ingested_total",
            self.events_ingested.load(Ordering::Relaxed),
            "Total events ingested.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_events_rejected_total",
            self.events_rejected.load(Ordering::Relaxed),
            "Total events rejected.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_batches_total",
            self.batches_total.load(Ordering::Relaxed),
            "Total detection batches processed.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_promotions_total",
            self.promotions_total.load(Ordering::Relaxed),
            "Total IPs promoted to tracking.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_cold_skipped_total",
            self.cold_skipped_total.load(Ordering::Relaxed),
            "Total cold IPs skipped.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_blocks_detection",
            self.blocks_detection.load(Ordering::Relaxed),
            "Blocks from detection engine.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_blocks_subnet",
            self.blocks_subnet.load(Ordering::Relaxed),
            "Blocks from subnet module.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_blocks_forecast",
            self.blocks_forecast.load(Ordering::Relaxed),
            "Blocks from forecasting.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_forecast_ticks",
            self.forecast_ticks.load(Ordering::Relaxed),
            "Forecast ticks executed.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_entropy_ticks",
            self.entropy_ticks.load(Ordering::Relaxed),
            "Entropy check ticks executed.",
            "counter"
        ));
        out.push_str(&emit!(
            "ramshield_hw_rps",
            Metrics::f64(&self.hw_rps_bits),
            "Holt-Winters RPS forecast.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_hw_zscore",
            Metrics::f64(&self.hw_z_bits),
            "Holt-Winters z-score.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_hw_forecast",
            Metrics::f64(&self.hw_forecast_bits),
            "Holt-Winters forecast value.",
            "gauge"
        ));
        out.push_str(&emit!(
            "ramshield_entropy",
            Metrics::f64(&self.entropy_bits),
            "Current IP entropy.",
            "gauge"
        ));
        println!(); // trailing newline flushes stanza

        out
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_log_evicts_at_configured_cap() {
        let m = Metrics::with_block_log(5);
        for i in 0..12 {
            m.record_block(&format!("10.0.0.{i}"), "high_rps", "detection");
        }
        let log = m.block_log.lock().unwrap();
        assert_eq!(log.len(), 5, "ring must evict oldest beyond cap");
        // newest survives, oldest gone
        assert_eq!(log.back().unwrap().ip, "10.0.0.11");
        assert_eq!(log.front().unwrap().ip, "10.0.0.7");
    }

    #[test]
    fn block_log_cap_floor_is_one() {
        // zero/nonsense config must not produce a zero-capacity deadlock ring
        let m = Metrics::with_block_log(0);
        m.record_block("10.0.0.1", "high_rps", "detection");
        assert_eq!(m.block_log.lock().unwrap().len(), 1);
    }
}
