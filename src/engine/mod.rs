use crate::config::{Config, ConfigHandle};
use crate::detection::{BlockDecision, ConnectionEvent, DetectionEngine};
use crate::dns::DnsMonitor;
use crate::error::{Result, RsError};
use crate::forecasting::Forecaster;
use crate::storage::wal::WalEntry;
use crate::learning::PatternLearner;
pub use crate::learning;
use crate::ipc::{IpDetail, Request, Response, Stats};
use crate::metrics::{DashboardSnapshot, Metrics, now_ms, get_system_usage};
use crate::storage::{BlockReason, BlockState, IpRecord, Store, Value};
use crate::util::BoundedVecDeque;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, RwLock,
};
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Semaphore};
use tracing::{info, warn};
use num_cpus;

pub struct Engine {
    pub store:         Arc<Store>,
    pub config:        ConfigHandle,
    pub metrics:       Arc<Metrics>,
    #[allow(dead_code)]
    pub block_tx:          broadcast::Sender<BlockDecision>,
    pub detection:         Arc<DetectionEngine>,
    pub start_time:        Instant,
    pub blocked_count:     Arc<AtomicU64>,
    /// Shutdown flag - set to true on Ctrl+C, signals graceful termination
    pub shutdown:          Arc<AtomicBool>,
    /// DNS monitor for traffic prediction
    pub dns_monitor:       Option<Arc<DnsMonitor>>,
    pub pattern_learner:    Arc<PatternLearner>,
    pub snapshot_cache:    Arc<RwLock<DashboardSnapshot>>,
}

impl Engine {
    pub fn new(config: Config) -> Self {
        let cfg      = config.into_handle();
        let store    = Arc::new(Store::new(cfg.load().engine.shard_count));
        let metrics  = Arc::new(Metrics::new());
        let (btx, _) = broadcast::channel::<BlockDecision>(8_192);
        let pattern_learner = Arc::new(PatternLearner::new(cfg.load().detection.pattern_similarity_threshold));
        let shutdown = Arc::new(AtomicBool::new(false));
        let detection = Arc::new(DetectionEngine::new(
            store.clone(), cfg.clone(), btx.clone(), metrics.clone(), pattern_learner.clone(), shutdown.clone(),
        ));

        // WAL replay: restore blocked IPs from previous session
        if cfg.load().storage.wal_enabled {
            let wal_dir = cfg.load().storage.wal_path.clone();
            match crate::storage::wal::Wal::replay(&wal_dir) {
                Ok(entries) => {
                    use crate::storage::{BlockState, IpRecord, Value};
                    let mut restored = 0u64;
                    for entry in entries {
                        if let WalEntry::BlockIp { ip, ttl_secs, .. } = entry {
                            if let Ok(parsed) = ip.parse::<std::net::IpAddr>() {
                                let now = now_ns();
                                let rec = IpRecord {
                                    ip: parsed,
                                    request_count: 0,
                                    ewma_rps: 0.0,
                                    baseline_rps: 0.0,
                                    baseline_threat: 0.0,
                                    behavior_history_rps: BoundedVecDeque::new(32),
                                    behavior_history_threat: BoundedVecDeque::new(32),
                                    first_seen_ns: now,
                                    last_seen_ns: now,
                                    bytes_in: 0,
                                    status_dist: [0; 5],
                                    proto_fingerprint: 0,
                                    country: [0; 2],
                                    threat_score: 1.0,
                                    block_state: BlockState::Blocked {
                                        reason: crate::storage::BlockReason::ManualBlock,
                                        since_ns: now,
                                    },
                                    asn: 0,
                                };
                                let lim = cfg.load().engine.ram_limit_mb * 1024 * 1024;
                                if store.insert(ip, Value::IpRecord(rec), ttl_secs, lim).is_ok() {
                                    restored += 1;
                                }
                            }
                        }
                    }
                    if restored > 0 {
                        info!("WAL replay: restored {} blocked IPs", restored);
                    }
                }
                Err(e) => {
                    warn!("WAL replay failed: {}", e);
                }
            }
        }

        let dns_monitor = Arc::new(DnsMonitor::new());
        let initial_snap = DashboardSnapshot::default();
        
        Self {
            store, config: cfg, metrics, block_tx: btx, detection,
            pattern_learner,
            start_time:    Instant::now(),
            blocked_count: Arc::new(AtomicU64::new(0)),
            shutdown,
            dns_monitor:   Some(dns_monitor),
            snapshot_cache: Arc::new(RwLock::new(initial_snap)),
        }
    }

    pub fn start(self: &Arc<Self>) {
        // Snapshot cache refresher: refresh every 2s from dedicated OS thread
        // Uses blocking RwLock (std::sync), not tokio::sync
        {
            let eng_cache = self.clone();
            std::thread::Builder::new()
                .name("rs-snapcache".into())
                .spawn(move || {
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        let snap = eng_cache.compute_full_snapshot();
                        if let Ok(mut cache) = eng_cache.snapshot_cache.write() {
                            *cache = snap;
                        }
                    }
                })
                .expect("spawn snapshot cache thread");
        }
        let cfg = self.config.load();
        let n = if cfg.engine.worker_threads == 0 { num_cpus::get() } else { cfg.engine.worker_threads };

        // Detection workers
        self.detection.clone().spawn_workers(n);

        // Block applier task
        {
            let eng     = self.clone();
            let mut brx = self.block_tx.subscribe();
            tokio::spawn(async move {
                while let Ok(d) = brx.recv().await {
                    eng.apply_block(&d);
                    eng.metrics.inc_blocks();
                }
            });
        }

        // Forecaster (runs until the process exits)
        if cfg.forecasting.enabled {
            let fc = Arc::new(Forecaster::new(
                self.store.clone(),
                cfg.forecasting.clone(),
                self.block_tx.clone(),
                self.metrics.clone(),
                self.pattern_learner.clone(),
            ));
            tokio::spawn(async move { fc.run().await; });
        }

        // DNS monitoring
        let eng = self.clone();
        tokio::spawn(async move {
            if let Some(ref dns_monitor) = eng.dns_monitor {
                dns_monitor.monitor_dns_queries().await;
            }
        });

        // IPC TCP server
        let eng = self.clone();
        let addr = self.config.load().ipc.tcp_addr.clone();
        let max_connections = self.config.load().ipc.max_connections;
        tokio::spawn(async move {
            if let Err(e) = ipc_server(eng.clone(), &addr, max_connections).await {
                tracing::error!("IPC server error: {}", e);
            }
        });

        info!("Engine started ({} workers, IPC {})", n, cfg.ipc.tcp_addr);
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        info!("Shutdown signal received — draining pending events");
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    pub fn dashboard_snapshot(&self) -> DashboardSnapshot {
        // Read from cache without blocking; fallback to direct computation on lock contention
        if let Ok(g) = self.snapshot_cache.try_read() {
            return g.clone();
        }
        self.compute_full_snapshot()
    }

    pub fn compute_full_snapshot(&self) -> DashboardSnapshot {
        let cfg = self.config.load();
        
        let channel_depth = self.detection.queue_depth();
        let uptime_secs = self.start_time.elapsed().as_secs();
        let ips_tracked = self.store.len();
        let blocked_total = self.blocked_count.load(Ordering::Relaxed);
        let ram_bytes = self.store.ram_bytes();
        let ram_limit_mb = cfg.engine.ram_limit_mb;
        
        let (cpu_usage, total_memory_mb) = get_system_usage();
        
        let is_healthy = channel_depth < 1_000_000; 
        let health_reason = if is_healthy {
            "ok".to_string()
        } else {
            "channel saturated".to_string()
        };

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
            ipc_requests: self.metrics.requests_total.load(Ordering::Relaxed),
            events_ingested: self.metrics.events_ingested.load(Ordering::Relaxed),
            events_rejected: self.metrics.events_rejected.load(Ordering::Relaxed),
            channel_depth,
            batches_total: self.metrics.batches_total.load(Ordering::Relaxed),
            promotions: self.metrics.promotions_total.load(Ordering::Relaxed),
            cold_skipped: self.metrics.cold_skipped_total.load(Ordering::Relaxed),
            blocks_applied: self.metrics.blocks_total.load(Ordering::Relaxed),
            pipeline: crate::metrics::PipelineFlow {
                ingest:   self.metrics.events_ingested.load(Ordering::Relaxed),
                queued:   channel_depth as u64,
                batched:  self.metrics.last_batch_events.load(Ordering::Relaxed),
                promoted: self.metrics.last_batch_promoted.load(Ordering::Relaxed),
                merged:   self.metrics.last_batch_promoted.load(Ordering::Relaxed), // Assuming merged = promoted for now
                blocked:  self.metrics.last_batch_blocks.load(Ordering::Relaxed),
            },
            is_healthy,
            health_reason,
        }
    }

    pub fn get_batch_history(&self) -> Vec<crate::metrics::BatchRecord> {
        self.metrics.get_batch_history()
    }

    pub fn get_block_log(&self) -> Vec<crate::metrics::BlockRecord> {
        self.metrics.get_block_log()
    }

    pub fn get_hot_subnets(&self) -> Vec<crate::metrics::SubnetRow> {
        let hot: Vec<crate::metrics::SubnetRow> = self
            .store
            .subnet_table()
            .iter()
            .map(|e| {
                let p = e.value().prefix;
                crate::metrics::SubnetRow {
                    prefix: format!("{}.{}.{}", p[0], p[1], p[2]),
                    events: e.value().total_rps,
                }
            })
            .filter(|r| r.events > 0)
            .collect();
        hot
    }

    pub fn get_module_stats(&self) -> Vec<crate::metrics::ModuleStats> {
        let cfg = self.config.load();
        let uptime_secs = self.start_time.elapsed().as_secs();
        let ingested = self.metrics.events_ingested.load(Ordering::Relaxed);
        let channel_depth = self.detection.queue_depth();
        let ips_tracked = self.store.len();
        let ram_bytes = self.store.ram_bytes();
        let ram_limit_mb = cfg.engine.ram_limit_mb;
        self.metrics.get_module_stats_data(
            uptime_secs,
            ingested,
            channel_depth,
            ips_tracked,
            ram_bytes,
            ram_limit_mb,
        )
    }

    fn apply_block(&self, d: &BlockDecision) {
        let ip  = d.ip.to_string();
        let cfg = self.config.load();
        let now = now_ns();

        let mut rec = match self.store.get(&ip) {
            Some(Value::IpRecord(r)) => r,
            _ => IpRecord {
                ip: d.ip, request_count: 0, ewma_rps: 0.0,
                baseline_rps: 0.0, baseline_threat: 0.0,
                behavior_history_rps: BoundedVecDeque::new(cfg.detection.history_cap),
                behavior_history_threat: BoundedVecDeque::new(cfg.detection.history_cap),
                first_seen_ns: now, last_seen_ns: now, bytes_in: 0,
                status_dist: [0; 5], proto_fingerprint: 0,
                country: [0; 2], threat_score: 1.0,
                block_state: BlockState::Clean, asn: 0,
            },
        };

        if matches!(rec.block_state, BlockState::Blocked { .. }) { return; }
        rec.block_state = BlockState::Blocked { reason: d.reason.clone(), since_ns: now };

        let ttl = d.ttl_secs
            .or_else(|| (cfg.detection.block_ttl_secs > 0).then_some(cfg.detection.block_ttl_secs));
        let lim = cfg.engine.ram_limit_mb * 1024 * 1024;
        if let Err(e) = self.store.insert(ip.clone(), Value::IpRecord(rec), ttl, lim) {
            warn!("Failed to insert IP record for {}: {}", ip, e);
        }
        self.blocked_count.fetch_add(1, Ordering::Relaxed);
        self.metrics.record_block(&ip, &d.reason.to_string(), "engine");
    }

    pub fn handle(&self, req: Request) -> Response {
        self.metrics.inc_requests();
        match req {
            Request::CheckIp { ip }                            => self.check_ip(&ip),
            Request::BlockIp { ip, reason, ttl_secs }          => self.manual_block(&ip, &reason, ttl_secs),
            Request::UnblockIp { ip }                          => self.unblock(&ip),
            Request::GetIpStats { ip }                         => self.ip_detail(&ip),
            Request::GetStats                                  => self.stats(),
            Request::ReportConnection { ip, bytes, status_code, proto_fp } =>
                self.report_one(&ip, bytes, status_code, proto_fp),
            Request::ReportConnections { events }                => self.report_batch(events),
            Request::Flush => Response::Ok { message: "flushed".into() },
        }
    }

    fn check_ip(&self, ip: &str) -> Response {
        match self.store.get(ip) {
            Some(Value::IpRecord(r)) => Response::IpStatus {
                ip:       ip.to_string(),
                blocked:  matches!(r.block_state, BlockState::Blocked { .. }),
                threat:   r.threat_score,
                ewma_rps: r.ewma_rps,
                reason:   match &r.block_state {
                    BlockState::Blocked { reason, .. } => Some(reason.to_string()),
                    _ => None,
                },
            },
            _ => Response::IpStatus {
                ip: ip.to_string(), blocked: false, threat: 0.0, ewma_rps: 0.0, reason: None,
            },
        }
    }

    fn manual_block(&self, ip: &str, _reason: &str, ttl_secs: Option<u64>) -> Response {
        let parsed: std::net::IpAddr = match ip.parse() {
            Ok(a)  => a,
            Err(_) => return Response::Error { code: 400, message: "invalid IP".into() },
        };
        self.apply_block(&BlockDecision {
            ip: parsed, reason: BlockReason::ManualBlock, ttl_secs, batch_subnet: None,
        });
        Response::Ok { message: format!("blocked {}", ip) }
    }

    fn unblock(&self, ip: &str) -> Response {
        match self.store.get(ip) {
            Some(Value::IpRecord(mut r)) => {
                r.block_state = BlockState::Clean;
                let lim = self.config.load().engine.ram_limit_mb * 1024 * 1024;
                let _ = self.store.insert(ip.to_string(), Value::IpRecord(r), None, lim);
                Response::Ok { message: format!("unblocked {}", ip) }
            }
            _ => Response::Error { code: 404, message: "IP not found".into() },
        }
    }

    fn ip_detail(&self, ip: &str) -> Response {
        match self.store.get(ip) {
            Some(Value::IpRecord(r)) => {
                let now = now_ns();
                Response::IpDetail(IpDetail {
                    ip:           ip.to_string(),
                    count:        r.request_count,
                    ewma_rps:     r.ewma_rps,
                    threat:       r.threat_score,
                    state:        format!("{:?}", r.block_state),
                    bytes_in:     r.bytes_in,
                    first_seen_s: now.saturating_sub(r.first_seen_ns) / 1_000_000_000,
                    last_seen_s:  now.saturating_sub(r.last_seen_ns)  / 1_000_000_000,
                })
            }
            _ => Response::Error { code: 404, message: "IP not found".into() },
        }
    }

    fn stats(&self) -> Response {
        let cfg = self.config.load();
        Response::Stats(Stats {
            ips_tracked:  self.store.len(),
            blocked:      self.blocked_count.load(Ordering::Relaxed),
            ram_bytes:    self.store.ram_bytes(),
            ram_limit_mb: cfg.engine.ram_limit_mb,
            uptime_secs:  self.start_time.elapsed().as_secs(),
            evictions:    self.store.total_evictions.load(Ordering::Relaxed),
        })
    }

    fn report_one(&self, ip: &str, bytes: u64, status_code: u16, proto_fp: u32) -> Response {
        match self.parse_event(ip, bytes, status_code, proto_fp) {
            Ok(ev) => match self.detection.event_sender().try_send(ev) {
                Ok(_)  => {
                    self.metrics.inc_ingested(1);
                    Response::Ok { message: "accepted".into() }
                }
                Err(_) => {
                    self.metrics.inc_rejected(1);
                    Response::Error { code: 503, message: "channel full".into() }
                }
            },
            Err(msg) => Response::Error { code: 400, message: msg },
        }
    }

    fn report_batch(&self, events: Vec<crate::ipc::ConnectionReport>) -> Response {
        let ts = now_ns();
        let mut accepted = 0u32;
        let mut rejected = 0u32;
        let tx = self.detection.event_sender();

        for ev in events {
            let parsed: std::net::IpAddr = match ev.ip.parse() {
                Ok(a)  => a,
                Err(_) => { rejected += 1; continue; }
            };
            match tx.try_send(ConnectionEvent {
                ip: parsed,
                timestamp_ns: ts,
                bytes: ev.bytes,
                status_code: ev.status_code,
                proto_fingerprint: ev.proto_fp,
            }) {
                Ok(_)  => accepted += 1,
                Err(_) => { rejected += 1; break; }
            }
        }

        self.metrics.inc_ingested(accepted as u64);
        self.metrics.inc_rejected(rejected as u64);
        if accepted == 0 && rejected > 0 {
            return Response::Error { code: 503, message: "channel full or all invalid".into() };
        }
        Response::BatchOk { accepted, rejected }
    }

    fn parse_event(
        &self, ip: &str, bytes: u64, status_code: u16, proto_fp: u32,
    ) -> std::result::Result<ConnectionEvent, String> {
        let parsed: std::net::IpAddr = ip.parse().map_err(|_| "invalid IP".to_string())?;
        Ok(ConnectionEvent {
            ip: parsed,
            timestamp_ns: now_ns(),
            bytes,
            status_code,
            proto_fingerprint: proto_fp,
        })
    }
}

async fn ipc_server(engine: Arc<Engine>, addr: &str, max: usize) -> Result<()> {
    let listener = TcpListener::bind(addr).await
        .map_err(|e| RsError::Ipc(e.to_string()))?;
    let sema = Arc::new(Semaphore::new(max));
    info!("IPC listening on {}", addr);
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(v)  => v,
            Err(e) => { warn!("accept: {}", e); continue; }
        };
        let permit = match sema.clone().try_acquire_owned() {
            Ok(p)  => p,
            Err(_) => { warn!("conn limit, dropping {}", peer); continue; }
        };
        let eng = engine.clone();
        tokio::spawn(async move {
            let _p = permit;
            conn_handler(eng, stream).await;
        });
    }
}

async fn conn_handler(engine: Arc<Engine>, stream: tokio::net::TcpStream) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let resp = match serde_json::from_str::<Request>(&line) {
            Ok(req) => engine.handle(req),
            Err(e)  => Response::Error { code: 400, message: e.to_string() },
        };
        let mut out = serde_json::to_string(&resp)
            .unwrap_or_else(|_| r#"{"type":"error","code":500,"message":"serialise failed"}"#.into());
        out.push('\n');
        if writer.write_all(out.as_bytes()).await.is_err() { break; }
    }
}

fn now_ns() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64
}
