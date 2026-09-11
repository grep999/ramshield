// ── Adaptive brute-force detection ──────────────────────────────────────────
// Sliding-window failure counter per IP with escalating strikes: every block
// lowers the next threshold toward adaptive_floor, so an attacker who returns
// after cooldown faces progressively less tolerance. Slow-spread attacks stay
// inside the window; cooldown TTL re-arms the counter; the sweep thread
// expires cooldowns and evicts idle entries.
//
// Why this resists adaptive (AI-driven) attackers:
//   - Sliding window defeats log-boundary gaming (no fixed minute buckets).
//   - Escalating strikes defeat "N-1 attempts, sleep, repeat" pacing.
//   - Cooldown + strike memory defeats immediate post-block retry probing.

pub mod config;
pub mod types;

pub use config::DetectionConfig;
pub use types::{AuthEvent, BlockCommand, BlockReason};

use std::net::IpAddr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use dashmap::DashMap;

type Store = Arc<DashMap<IpAddr, AuthFailureCounter>>;

#[derive(Debug)]
struct AuthFailureCounter {
    failures: u64,
    first_failure: Instant,
    last_failure: Instant,
    /// Times this IP has been blocked — drives escalation.
    blocks: u32,
    reported: bool,
    backoff_until: Option<Instant>,
}

impl AuthFailureCounter {
    fn new() -> Self {
        Self {
            failures: 0,
            first_failure: Instant::now(),
            last_failure: Instant::now(),
            blocks: 0,
            reported: false,
            backoff_until: None,
        }
    }

    /// Effective threshold: base minus escalation, floored.
    fn threshold(&self, cfg: &DetectionConfig) -> u64 {
        cfg.max_failures
            .saturating_sub(u64::from(self.blocks))
            .max(cfg.adaptive_floor)
    }

    /// Record one failure. Returns true when a block should be emitted.
    fn record(&mut self, now: Instant, cfg: &DetectionConfig) -> bool {
        // Cooldown expiry: re-arm before counting this event.
        if self.reported && self.backoff_until.is_some_and(|t| now >= t) {
            self.reported = false;
            self.failures = 0;
            self.first_failure = now;
        }

        // Sliding window.
        if now.duration_since(self.first_failure) > Duration::from_secs(cfg.window_secs) {
            self.failures = 1;
            self.first_failure = now;
        } else {
            self.failures += 1;
        }
        self.last_failure = now;

        if !self.reported && self.failures >= self.threshold(cfg) {
            self.reported = true;
            self.blocks += 1;
            self.backoff_until = Some(now + Duration::from_secs(cfg.block_ttl_secs));
            return true;
        }
        false
    }

    fn evictable(&self, now: Instant, idle: Duration) -> bool {
        !self.reported && now.duration_since(self.last_failure) > idle
    }
}

fn start_sweep(store: Store, cfg: DetectionConfig, stop: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("bruteforce-sweep".into())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(10));
                let now = Instant::now();
                let idle = Duration::from_secs(cfg.evict_idle_secs);
                store.retain(|_, c| {
                    // Expire finished cooldowns so stats stay accurate.
                    if c.reported && c.backoff_until.is_some_and(|t| now >= t) {
                        c.reported = false;
                        c.failures = 0;
                    }
                    !c.evictable(now, idle)
                });
            }
        })
        .expect("spawn sweep thread");
}

pub struct BruteforceMonitor {
    store: Store,
    config: DetectionConfig,
    block_tx: crossbeam_channel::Sender<BlockCommand>,
    stop_sweep: Arc<AtomicBool>,
}

impl BruteforceMonitor {
    /// Returns the monitor and the receiving end of the block channel.
    pub fn new(config: DetectionConfig) -> (Self, crossbeam_channel::Receiver<BlockCommand>) {
        let store: Store = Arc::new(DashMap::new());
        let (block_tx, block_rx) = crossbeam_channel::bounded(4096);
        let stop_sweep = Arc::new(AtomicBool::new(false));
        start_sweep(store.clone(), config.clone(), stop_sweep.clone());
        let monitor = Self {
            store,
            config,
            block_tx,
            stop_sweep,
        };
        (monitor, block_rx)
    }

    /// Process one failed authentication. Returns true if a block was emitted.
    pub fn process_failure(&self, event: &AuthEvent) -> bool {
        let ip = event.ip;
        let now = event.timestamp;
        let mut counter = self.store.entry(ip).or_insert_with(AuthFailureCounter::new);
        let triggered = counter.record(now, &self.config);
        if triggered {
            let cmd = BlockCommand {
                ip,
                reason: BlockReason::BruteForce,
                ttl_secs: self.config.block_ttl_secs,
                context: format!(
                    "{} failures in {}s from {} (strikes: {})",
                    counter.failures, self.config.window_secs, event.source, counter.blocks,
                ),
            };
            let _ = self.block_tx.try_send(cmd);
        }
        triggered
    }

    pub fn shutdown(&self) {
        self.stop_sweep.store(true, Ordering::Relaxed);
    }

    pub fn snapshot_stats(&self) -> StatsSnapshot {
        let mut s = StatsSnapshot::default();
        for c in self.store.iter() {
            s.ips_tracked += 1;
            s.total_failures += c.failures;
            if c.reported {
                s.currently_blocked += 1;
            }
        }
        s
    }
}

#[derive(Debug, Clone, Default)]
pub struct StatsSnapshot {
    pub ips_tracked: usize,
    pub currently_blocked: usize,
    pub total_failures: u64,
}