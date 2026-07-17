use sysinfo::{CpuExt, System, SystemExt};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const HISTORY: usize = 80;
const BLOCK_LOG: usize = 40;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

<<<<<<< HEAD
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
=======
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

static mut SYSTEM_PTR: *mut System = std::ptr::null_mut();
static SYSTEM: Once = Once::new();

fn get_system() -> &'static mut System {
    SYSTEM.call_once(|| {
        let sys = Box::new(System::new_all());
        unsafe { SYSTEM_PTR = Box::into_raw(sys); }
    });
    unsafe { &mut *SYSTEM_PTR }
>>>>>>> 5b4bb0e (Refactor: metrics to ms, detection timing; config tuning)
}

pub fn get_system_usage() -> (f32, usize) {
    with_system(|sys| {
        sys.refresh_all();
        let cpu_usage = sys.global_cpu_info().cpu_usage();
        let total_memory_mb = (sys.total_memory() as f64 / 1024.0 / 1024.0) as usize;
        (cpu_usage, total_memory_mb)
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRecord {
    pub ts_ms:  u64,
    pub ip:     String,
    pub reason: String,
    pub module: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleStats {
    pub label:    String,
    pub events:   u64,
    pub errors:   u64,
    pub rate_per_sec: f64,
    pub detail:   serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSnapshot {
    pub ts_ms:            u64,
    pub uptime_secs:      u64,
    pub ips_tracked:      usize,
    pub blocked_total:    u64,
    pub ram_bytes:        usize,
    pub ram_limit_mb:     usize,
    pub ram_pct:          f64,
    pub cpu_usage:        f32,
    pub memory_usage_mb:  usize,
    pub ipc_requests:     u64,
    pub events_ingested:  u64,
    pub events_rejected:  u64,
    pub channel_depth:    usize,
    pub batches_total:    u64,
    pub promotions:       u64,
    pub cold_skipped:     u64,
    pub blocks_applied:   u64,
    pub pipeline:         PipelineFlow,
    pub is_healthy:       bool,
    pub health_reason:    String,
}

impl Default for DashboardSnapshot {
    fn default() -> Self {
        Self {
            ts_ms: 0, uptime_secs: 0, ips_tracked: 0, blocked_total: 0,
            ram_bytes: 0, ram_limit_mb: 0, ram_pct: 0.0, cpu_usage: 0.0,
            memory_usage_mb: 0, ipc_requests: 0, events_ingested: 0,
            events_rejected: 0, channel_depth: 0, batches_total: 0,
            promotions: 0, cold_skipped: 0, blocks_applied: 0,
            pipeline: PipelineFlow { ingest:0, queued:0, batched:0, promoted:0, merged:0, blocked:0 },
            is_healthy: true, health_reason: "initializing".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubnetRow {
    pub prefix:  String,
    pub events:  u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
<<<<<<< HEAD
            if log.len() >= BLOCK_LOG {
                log.pop_front();
            }
            log.push_back(BlockRecord {
                ts_ms: now_ms(),
                ip: ip.to_string(),
                reason: reason.to_string(),
                module: module.to_string(),
            });
=======
            if log.len() >= BLOCK_LOG { log.pop_front(); }
            log.push_back(BlockRecord { ts_ms: now_ms(), ip: ip.to_string(), reason: reason.to_string(), module: module.to_string() });
>>>>>>> 5b4bb0e (Refactor: metrics to ms, detection timing; config tuning)
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

    pub fn get_batch_history(&self) -> Vec<BatchRecord> {
        self.batch_history
            .lock()
            .map(|h| h.iter().cloned().collect())
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
        let _last_pr = self.last_batch_promoted.load(Ordering::Relaxed);
        let _last_bl = self.last_batch_blocks.load(Ordering::Relaxed);
        
        let (_cpu_usage, total_system_memory_mb) = get_system_usage();

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
                events: self.forecast_ticks.load(Ordering::Relaxed) + self.entropy_ticks.load(Ordering::Relaxed),
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


impl Default for Metrics {
    fn default() -> Self { Self::new() }
}
