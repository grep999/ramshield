use sysinfo::{CpuExt, System, SystemExt};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const HISTORY: usize = 80;
const BLOCK_LOG: usize = 40;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn get_system_usage() -> (f32, usize) {
    let mut sys = System::new_all();
    sys.refresh_all();
    let cpu_usage = sys.global_cpu_info().cpu_usage();
    let total_memory_mb = (sys.total_memory() as f64 / 1024.0 / 1024.0) as usize;
    (cpu_usage, total_memory_mb)
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchRecord {
    pub ts_ms:              u64,
    pub events:             u32,
    pub unique_ips:         u32,
    /// Unique IPs promoted to full tracking
    pub promoted:           u32,
    /// Unique IPs skipped (below promotion threshold)
    pub cold_skipped:       u32,
    /// Connection events in promoted IPs
    pub promoted_events:    u32,
    /// Connection events in cold-skipped IPs
    pub cold_skipped_events: u32,
    pub blocks:             u32,
    pub hot_subnets:        u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockRecord {
    pub ts_ms:  u64,
    pub ip:     String,
    pub reason: String,
    pub module: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleStats {
    pub label:    String,
    pub events:   u64,
    pub errors:   u64,
    pub rate_per_sec: f64,
    pub detail:   serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardSnapshot {
    pub ts_ms:            u64,
    pub uptime_secs:      u64,
    pub ips_tracked:      usize,
    pub blocked_total:    u64,
    pub ram_bytes:        usize,
    pub ram_limit_mb:     usize,
    pub ram_pct:          f64,
    pub cpu_usage:        f32, // New field for CPU usage
    pub memory_usage_mb:  usize, // New field for total system memory usage in MB
    pub ipc_requests:     u64,
    pub events_ingested:  u64,
    pub events_rejected:  u64,
    pub channel_depth:    usize,
    pub batches_total:    u64,
    pub promotions:       u64,
    pub cold_skipped:     u64,
    pub blocks_applied:   u64,
    pub last_batch:       Option<BatchRecord>,
    pub batch_history:    Vec<BatchRecord>,
    pub recent_blocks:    Vec<BlockRecord>,
    pub hot_subnets:      Vec<SubnetRow>,
    pub pipeline:         PipelineFlow,
    pub modules:          Vec<ModuleStats>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubnetRow {
    pub prefix:  String,
    pub events:  u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineFlow {
    pub ingest:   u64,
    pub queued:   u64,
    pub batched:  u64,
    pub promoted: u64,
    pub merged:   u64,
    pub blocked:  u64,
}

pub struct Metrics {
    pub requests_total:     Arc<AtomicU64>,
    pub blocks_total:       Arc<AtomicU64>,
    pub events_ingested:    Arc<AtomicU64>,
    pub events_rejected:    Arc<AtomicU64>,
    pub batches_total:      Arc<AtomicU64>,
    pub promotions_total:   Arc<AtomicU64>,
    pub cold_skipped_total: Arc<AtomicU64>,
    pub blocks_detection:   Arc<AtomicU64>,
    pub blocks_subnet:      Arc<AtomicU64>,
    pub blocks_forecast:    Arc<AtomicU64>,
    pub forecast_ticks:     Arc<AtomicU64>,
    pub entropy_ticks:      Arc<AtomicU64>,
    pub hw_rps_bits:        Arc<AtomicU64>,
    pub hw_z_bits:          Arc<AtomicU64>,
    pub hw_forecast_bits:   Arc<AtomicU64>,
    pub entropy_bits:       Arc<AtomicU64>,
    pub last_batch_events:  Arc<AtomicU64>,
    pub last_batch_promoted: Arc<AtomicU64>,
    pub last_batch_blocks:  Arc<AtomicU64>,
    pub last_batch:         Arc<Mutex<Option<BatchRecord>>>,
    pub batch_history:      Arc<Mutex<VecDeque<BatchRecord>>>,
    pub block_log:          Arc<Mutex<VecDeque<BlockRecord>>>,
    started_ms:             u64,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            requests_total:     Arc::new(AtomicU64::new(0)),
            blocks_total:       Arc::new(AtomicU64::new(0)),
            events_ingested:    Arc::new(AtomicU64::new(0)),
            events_rejected:    Arc::new(AtomicU64::new(0)),
            batches_total:      Arc::new(AtomicU64::new(0)),
            promotions_total:   Arc::new(AtomicU64::new(0)),
            cold_skipped_total: Arc::new(AtomicU64::new(0)),
            blocks_detection:   Arc::new(AtomicU64::new(0)),
            blocks_subnet:      Arc::new(AtomicU64::new(0)),
            blocks_forecast:    Arc::new(AtomicU64::new(0)),
            forecast_ticks:     Arc::new(AtomicU64::new(0)),
            entropy_ticks:      Arc::new(AtomicU64::new(0)),
            hw_rps_bits:        Arc::new(AtomicU64::new(0)),
            hw_z_bits:          Arc::new(AtomicU64::new(0)),
            hw_forecast_bits:   Arc::new(AtomicU64::new(0)),
            entropy_bits:       Arc::new(AtomicU64::new(0)),
            last_batch_events:  Arc::new(AtomicU64::new(0)),
            last_batch_promoted: Arc::new(AtomicU64::new(0)),
            last_batch_blocks:  Arc::new(AtomicU64::new(0)),
            last_batch:         Arc::new(Mutex::new(None)),
            batch_history:      Arc::new(Mutex::new(VecDeque::with_capacity(HISTORY))),
            block_log:          Arc::new(Mutex::new(VecDeque::with_capacity(BLOCK_LOG))),
            started_ms:         now_ms(),
        }
    }

    pub fn inc_requests(&self)  { self.requests_total.fetch_add(1, Ordering::Relaxed); }
    pub fn inc_blocks(&self)    { self.blocks_total.fetch_add(1, Ordering::Relaxed); }
    pub fn inc_ingested(&self, n: u64) {
        self.events_ingested.fetch_add(n, Ordering::Relaxed);
    }
    pub fn inc_rejected(&self, n: u64) {
        self.events_rejected.fetch_add(n, Ordering::Relaxed);
    }

    pub fn record_batch(&self, rec: BatchRecord) {
        self.batches_total.fetch_add(1, Ordering::Relaxed);
        self.last_batch_events.store(rec.events as u64, Ordering::Relaxed);
        self.last_batch_promoted.store(rec.promoted as u64, Ordering::Relaxed);
        self.last_batch_blocks.store(rec.blocks as u64, Ordering::Relaxed);
        self.promotions_total.fetch_add(rec.promoted as u64, Ordering::Relaxed);
        self.cold_skipped_total.fetch_add(rec.cold_skipped as u64, Ordering::Relaxed);
        self.blocks_detection.fetch_add(rec.blocks as u64, Ordering::Relaxed);
        if let Ok(mut h) = self.batch_history.lock() {
            if h.len() >= HISTORY {
                h.pop_front();
            }
            h.push_back(rec.clone());
        }
        if let Ok(mut lb) = self.last_batch.lock() {
            *lb = Some(rec);
        }
    }

    pub fn record_block(&self, ip: &str, reason: &str, module: &str) {
        if let Ok(mut log) = self.block_log.lock() {
            if log.len() >= BLOCK_LOG {
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

    pub fn set_forecast_hw(&self, rps: f64, z: f64, forecast: f64) {
        self.hw_rps_bits.store(rps.to_bits(), Ordering::Relaxed);
        self.hw_z_bits.store(z.to_bits(), Ordering::Relaxed);
        self.hw_forecast_bits.store(forecast.to_bits(), Ordering::Relaxed);
        self.forecast_ticks.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_entropy(&self, h: f64) {
        self.entropy_bits.store(h.to_bits(), Ordering::Relaxed);
        self.entropy_ticks.fetch_add(1, Ordering::Relaxed);
    }

    fn f64(bits: &AtomicU64) -> f64 {
        f64::from_bits(bits.load(Ordering::Relaxed))
    }
}

impl Default for Metrics {
    fn default() -> Self { Self::new() }
}

pub fn build_snapshot(
    m: &Metrics,
    uptime_secs: u64,
    ips_tracked: usize,
    blocked_total: u64,
    ram_bytes: usize,
    ram_limit_mb: usize,
    channel_depth: usize,
    hot_subnets: Vec<SubnetRow>,
) -> DashboardSnapshot {
    let elapsed = ((now_ms().saturating_sub(m.started_ms)) as f64 / 1000.0).max(0.001);
    let ingested = m.events_ingested.load(Ordering::Relaxed);
    let batches = m.batches_total.load(Ordering::Relaxed);
    let blocks = m.blocks_total.load(Ordering::Relaxed);

    let batch_history: Vec<BatchRecord> = m
        .batch_history
        .lock()
        .map(|h| h.iter().cloned().collect())
        .unwrap_or_default();

    let recent_blocks = m
        .block_log
        .lock()
        .map(|h| h.iter().cloned().collect())
        .unwrap_or_default();

    let last_batch = m
        .last_batch
        .lock()
        .ok()
        .and_then(|lb| lb.clone())
        .or_else(|| batch_history.last().cloned());

    let hw_rps = Metrics::f64(&m.hw_rps_bits);
    let hw_z = Metrics::f64(&m.hw_z_bits);
    let hw_f = Metrics::f64(&m.hw_forecast_bits);
    let entropy = Metrics::f64(&m.entropy_bits);

    let (last_ev, last_pr, last_bl) = last_batch.as_ref().map(|b| {
        (b.events as u64, b.promoted as u64, b.blocks as u64)
    }).unwrap_or((
        m.last_batch_events.load(Ordering::Relaxed),
        m.last_batch_promoted.load(Ordering::Relaxed),
        m.last_batch_blocks.load(Ordering::Relaxed),
    ));

    let (cpu_usage, total_memory_mb) = get_system_usage();

    DashboardSnapshot {
        ts_ms: now_ms(),
        uptime_secs,
        ips_tracked,
        blocked_total,
        ram_bytes,
        ram_limit_mb,
        ram_pct: if ram_limit_mb > 0 {
            100.0 * ram_bytes as f64 / (ram_limit_mb as f64 * 1024.0 * 1024.0)
        } else {
            0.0
        },
        cpu_usage,
        memory_usage_mb: total_memory_mb,
        ipc_requests: m.requests_total.load(Ordering::Relaxed),
        events_ingested: ingested,
        events_rejected: m.events_rejected.load(Ordering::Relaxed),
        channel_depth,
        batches_total: batches,
        promotions: m.promotions_total.load(Ordering::Relaxed),
        cold_skipped: m.cold_skipped_total.load(Ordering::Relaxed),
        blocks_applied: blocks,
        last_batch,
        batch_history,
        recent_blocks,
        hot_subnets,
        pipeline: PipelineFlow {
            ingest:   ingested,
            queued:   channel_depth as u64,
            batched:  last_ev,
            promoted: last_pr,
            merged:   last_pr,
            blocked:  last_bl,
        },
        modules: vec![
            ModuleStats {
                label: "IPC".into(),
                events: m.requests_total.load(Ordering::Relaxed),
                errors: m.events_rejected.load(Ordering::Relaxed),
                rate_per_sec: m.requests_total.load(Ordering::Relaxed) as f64 / elapsed,
                detail: serde_json::json!({
                    "ingested": ingested,
                    "rejected": m.events_rejected.load(Ordering::Relaxed),
                }),
            },
            ModuleStats {
                label: "Detection".into(),
                events: ingested,
                errors: 0,
                rate_per_sec: ingested as f64 / elapsed,
                detail: serde_json::json!({
                    "batches": batches,
                    "promotions": m.promotions_total.load(Ordering::Relaxed),
                    "cold_skipped": m.cold_skipped_total.load(Ordering::Relaxed),
                    "blocks": m.blocks_detection.load(Ordering::Relaxed),
                    "subnet_blocks": m.blocks_subnet.load(Ordering::Relaxed),
                    "last_batch_events": last_ev,
                }),
            },
            ModuleStats {
                label: "Forecasting".into(),
                events: m.forecast_ticks.load(Ordering::Relaxed) + m.entropy_ticks.load(Ordering::Relaxed),
                errors: 0,
                rate_per_sec: m.forecast_ticks.load(Ordering::Relaxed) as f64 / elapsed,
                detail: serde_json::json!({
                    "hw_rps": hw_rps,
                    "hw_forecast": hw_f,
                    "hw_zscore": hw_z,
                    "entropy": entropy,
                    "forecast_blocks": m.blocks_forecast.load(Ordering::Relaxed),
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
                    "total_system_memory_mb": total_memory_mb,
                }),
            },
        ],
    }
}
