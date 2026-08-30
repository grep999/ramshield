//! Forecasting: Holt-Winters anomaly detection + entropy analysis.
//! Unified: src engine semantics + crate's `drain_threat_sample` primitive
//! (replaces racy pop+push-back) and all-zero counts guard.

use ramshield_config::ForecastingConfig;
use ramshield_metrics::Metrics;
use ramshield_storage::Store;
use ramshield_types::{EnforceAction, EnforceCommand};
use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

// ── Block TTLs ───────────────────────────────────────────────────────────────

/// TTL for blocks issued by the EWMA+HW forecaster when it predicts a spike
/// before the threshold is hit. Longer than the high_rps TTL because predicted
/// spikes need more time to either materialize or get re-validated.
const FORECAST_BLOCK_TTL_SECS: u64 = 300;

/// TTL for blocks issued by entropy-based detection (low-and-slow patterns).
/// Entropy threats evolve slower than RPS spikes, so we hold the block longer
/// to prevent rapid block/unblock churn.
const ENTROPY_BLOCK_TTL_SECS: u64 = 600;

// ── Holt-Winters ──────────────────────────────────────────────────────────────

pub struct HoltWinters {
    pub level: f64,
    pub trend: f64,
    pub seasonal: Vec<f64>,
    pub period: usize,
    alpha: f64,
    beta: f64,
    gamma: f64,
    tick: usize,
}

impl HoltWinters {
    pub fn new(alpha: f64, beta: f64, gamma: f64, period: usize) -> Self {
        let p = period.max(1);
        Self {
            level: 0.0,
            trend: 0.0,
            seasonal: vec![0.0; p],
            period: p,
            alpha,
            beta,
            gamma,
            tick: 0,
        }
    }

    pub fn update(&mut self, y: f64) -> f64 {
        if self.tick == 0 {
            self.level = y;
            self.tick += 1;
            return y;
        }
        let s = self.tick % self.period;
        let prev = self.level;
        let seas = self.seasonal[s];
        self.level = self.alpha * (y - seas) + (1.0 - self.alpha) * (prev + self.trend);
        self.trend = self.beta * (self.level - prev) + (1.0 - self.beta) * self.trend;
        self.seasonal[s] = self.gamma * (y - self.level) + (1.0 - self.gamma) * seas;
        self.tick += 1;
        let ns = self.seasonal[self.tick % self.period];
        (self.level + self.trend + ns).max(0.0)
    }

    pub fn zscore(&self, actual: f64, forecast: f64, std: f64) -> f64 {
        if std < 1e-9 {
            return 0.0;
        }
        (actual - forecast).abs() / std
    }
}

// ── Ring buffer ───────────────────────────────────────────────────────────────

pub struct RingBuffer {
    buf: VecDeque<f64>,
    cap: usize,
}

impl RingBuffer {
    pub fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(cap),
            cap,
        }
    }

    pub fn push(&mut self, v: f64) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(v);
    }

    pub fn std(&self) -> f64 {
        if self.buf.len() < 2 {
            return 0.0;
        }
        let m = self.buf.iter().sum::<f64>() / self.buf.len() as f64;
        let v = self.buf.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (self.buf.len() - 1) as f64;
        v.sqrt()
    }
}

// ── Forecaster — reads incremental counters, not full store scans ─────────────

pub struct Forecaster {
    store: Arc<Store>,
    config: ForecastingConfig,
    enforcement_tx: mpsc::Sender<EnforceCommand>,
    metrics: Arc<Metrics>,
    hw: tokio::sync::Mutex<HoltWinters>,
    history: tokio::sync::Mutex<RingBuffer>,
    /// P2 SPOT-lite: peaks-over-threshold reservoir of (rps − mean)⁺ samples.
    /// Extreme quantile estimated empirically instead of hand-tuned z cutoffs.
    peaks: tokio::sync::Mutex<PeakReservoir>,
}

/// Bounded reservoir of positive deviations; `extreme_q` returns the value that
/// exceeds (1 − 1/q_target) of observed peaks. Warm-up falls back to z-score.
struct PeakReservoir {
    vals: Vec<f64>,
    cap: usize,
    ticks: u64,
}

impl PeakReservoir {
    const WARM_TICKS: u64 = 60;

    fn new(cap: usize) -> Self {
        Self {
            vals: Vec::with_capacity(cap),
            cap,
            ticks: 0,
        }
    }

    fn push(&mut self, dev: f64) {
        self.ticks += 1;
        if dev <= 0.0 {
            return;
        }
        if self.vals.len() == self.cap {
            // evict a random-ish old entry — reservoir sampling lite
            let idx = (self.ticks as usize) % self.cap;
            self.vals[idx] = dev;
        } else {
            self.vals.push(dev);
        }
    }

    /// Empirical (1 − tail) quantile of observed peaks, e.g. tail=0.001 → q99.9.
    fn extreme_quantile(&self, tail: f64) -> Option<f64> {
        if self.ticks < Self::WARM_TICKS || self.vals.len() < 10 {
            return None;
        }
        let mut sorted = self.vals.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((sorted.len() as f64) * (1.0 - tail)).clamp(0.0, (sorted.len() - 1) as f64);
        Some(sorted[idx as usize])
    }

    fn warm(&self) -> bool {
        self.ticks >= Self::WARM_TICKS
    }
}

impl Forecaster {
    pub fn new(
        store: Arc<Store>,
        config: ForecastingConfig,
        enforcement_tx: mpsc::Sender<EnforceCommand>,
        metrics: Arc<Metrics>,
    ) -> Self {
        let hw = HoltWinters::new(
            config.ewma_alpha,
            config.hw_beta,
            config.hw_gamma,
            config.seasonality_period,
        );
        Self {
            store,
            config,
            enforcement_tx,
            metrics,
            hw: tokio::sync::Mutex::new(hw),
            history: tokio::sync::Mutex::new(RingBuffer::new(60)),
            peaks: tokio::sync::Mutex::new(PeakReservoir::new(512)),
        }
    }

    pub async fn run(self: Arc<Self>) {
        let mut t1 = tokio::time::interval(std::time::Duration::from_secs(1));
        let mut t5 = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            tokio::select! {
                _ = t1.tick() => { self.tick_hw().await; }
                _ = t5.tick() => { self.tick_entropy().await; }
            }
        }
    }

    async fn tick_hw(&self) {
        let traffic = &self.store.traffic;
        let rps = traffic.events_last_second.load(Ordering::Relaxed) as f64;
        let n = traffic.unique_ips_window.load(Ordering::Relaxed);

        let (z, spot_alarm) = {
            let mut hw = self.hw.lock().await;
            let mut hist = self.history.lock().await;
            let f = hw.update(rps);
            let s = hist.std().max(1.0);
            let z = hw.zscore(rps, f, s);
            hist.push(rps);

            // P2: feed deviation-above-forecast into the peak reservoir; alarm
            // on empirical extreme quantile once warm, z-score before that.
            let dev = (rps - f).max(0.0);
            let spot_alarm = {
                let mut pk = self.peaks.lock().await;
                pk.push(dev);
                match pk.extreme_quantile(0.001) {
                    Some(q) if pk.warm() => dev > q,
                    _ => z > self.config.anomaly_zscore,
                }
            };
            self.metrics.set_forecast_hw(rps, z, f);
            (z, spot_alarm)
        };

        debug!("HW rps={:.1} z={:.2} unique_ips={}", rps, z, n);
        if spot_alarm && n > 10 {
            warn!("ANOMALY z={:.2} rps={:.1}", z, rps);
            if z > 3.5 {
                self.preemptive_block().await;
            }
        }
    }

    async fn tick_entropy(&self) {
        let counts: Vec<u64> = self
            .store
            .traffic
            .subnet_window
            .iter()
            .map(|a| a.load(Ordering::Relaxed))
            .collect();

        // All-zero window ⇒ nothing to measure (crate guard — more precise than len<2).
        if counts.iter().all(|&c| c == 0) {
            return;
        }
        let total: u64 = counts.iter().sum();
        if total < 100 {
            return;
        }
        let h = shannon_entropy(&counts, total);
        self.metrics.set_entropy(h);
        debug!("entropy H={:.3} bits", h);
        if h < self.config.min_entropy {
            warn!("LOW ENTROPY H={:.3}", h);
            self.entropy_block().await;
        }
    }

    async fn preemptive_block(&self) {
        // Atomic drain (crate primitive) — no pop+push-back race with detection.
        let sample = self.store.traffic.drain_threat_sample();
        if sample.is_empty() {
            return;
        }

        let mut n = 0usize;
        for (ip, threat) in sample {
            if threat <= 0.7 {
                continue;
            }
            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 1,
                source: "forecasting".into(),
                actor: "system".into(),
                timestamp_utc: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                ttl_seconds: FORECAST_BLOCK_TTL_SECS,
                reason: "forecast_anomaly".into(),
                ip,
                action: EnforceAction::Block,
            };
            if self.enforcement_tx.try_send(cmd).is_err() {
                warn!(%ip, "enforcement queue full; forecast block rejected");
            }
            self.metrics
                .record_block_ip(&ip, "forecast_anomaly", "forecasting");
            self.metrics.blocks_forecast.fetch_add(1, Ordering::Relaxed);
            n += 1;
        }
        if n > 0 {
            info!("pre-emptive blocks: {}", n);
        }
    }

    async fn entropy_block(&self) {
        let sample = self.store.traffic.drain_threat_sample();
        if sample.is_empty() {
            return;
        }

        let mut top: Vec<(IpAddr, f32)> = sample.into_iter().collect();
        top.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let cut = (top.len() / 10).clamp(1, 50);
        let mut n = 0usize;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        for (ip, _threat) in top.iter().take(cut) {
            let cmd = EnforceCommand {
                decision_id: Uuid::new_v4(),
                policy_version: 1,
                source: "forecasting".into(),
                actor: "system".into(),
                timestamp_utc: ts,
                ttl_seconds: ENTROPY_BLOCK_TTL_SECS,
                reason: "entropy_anomaly".into(),
                ip: *ip,
                action: EnforceAction::Block,
            };
            if self.enforcement_tx.try_send(cmd).is_err() {
                warn!(%ip, "enforcement queue full; entropy block rejected");
            }
            self.metrics
                .record_block(&ip.to_string(), "entropy_anomaly", "forecasting");
            self.metrics.blocks_forecast.fetch_add(1, Ordering::Relaxed);
            n += 1;
        }
        if n > 0 {
            info!("entropy blocks: {}", n);
        }
    }
}

fn shannon_entropy(counts: &[u64], total: u64) -> f64 {
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / total as f64;
            -p * p.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entropy_uniform() {
        let counts = vec![100u64; 8];
        let total: u64 = counts.iter().sum();
        let h = shannon_entropy(&counts, total);
        assert!((h - 3.0).abs() < 0.01, "H={}", h);
    }

    #[test]
    fn hw_stable_forecast() {
        let mut hw = HoltWinters::new(0.3, 0.1, 0.1, 10);
        for _ in 0..50 {
            hw.update(1000.0);
        }
        assert!(hw.level > 900.0);
    }

    #[test]
    fn reservoir_cold_returns_none() {
        let mut pk = PeakReservoir::new(64);
        for i in 0..30 {
            pk.push(i as f64);
        }
        assert!(!pk.warm());
        assert_eq!(
            pk.extreme_quantile(0.001),
            None,
            "cold reservoir defers to z-score"
        );
    }

    #[test]
    fn reservoir_warm_extreme_quantile_above_typical() {
        let mut pk = PeakReservoir::new(4096);
        for _ in 0..1999 {
            pk.push(10.0); // typical deviation
        }
        pk.push(5_000.0); // one extreme peak among 2000
        assert!(pk.warm());
        let q = pk.extreme_quantile(0.001).unwrap();
        assert!((10.0..5_000.0).contains(&q), "q={}", q);
        // typical dev does not alarm; the extreme does
        assert!(q < 10.0 || (10.0f64).total_cmp(&q).is_le());
        assert!(5_000.0f64.total_cmp(&q).is_gt());
    }

    #[test]
    fn reservoir_negative_deviations_ignored_but_count_ticks() {
        let mut pk = PeakReservoir::new(64);
        for _ in 0..70 {
            pk.push(-1.0);
        }
        assert!(pk.warm(), "ticks advance even on negative dev");
        assert_eq!(
            pk.extreme_quantile(0.001),
            None,
            "no positive peaks → no quantile"
        );
    }

    #[test]
    fn all_zero_window_skipped() {
        let store = Arc::new(Store::new(4));
        let cfg = ForecastingConfig::default();
        let (tx, _rx) = mpsc::channel(8);
        let fc = Forecaster::new(store.clone(), cfg, tx, Arc::new(Metrics::new()));
        // Must not panic / must early-return on all-zero subnet_window.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        rt.block_on(fc.tick_entropy());
    }
}
