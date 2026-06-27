use crate::config::ForecastingConfig;
use crate::detection::BlockDecision;
use crate::metrics::Metrics;
use crate::storage::{BlockReason, Store};
use crate::learning::PatternLearner;
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

// ── Holt-Winters ──────────────────────────────────────────────────────────────

pub struct HoltWinters {
    pub level:    f64,
    pub trend:    f64,
    pub seasonal: Vec<f64>,
    pub period:   usize,
    alpha: f64,
    beta:  f64,
    gamma: f64,
    tick:  usize,
}

impl HoltWinters {
    pub fn new(alpha: f64, beta: f64, gamma: f64, period: usize) -> Self {
        let p = period.max(1);
        Self {
            level: 0.0, trend: 0.0, seasonal: vec![0.0; p],
            period: p, alpha, beta, gamma, tick: 0,
        }
    }

    pub fn update(&mut self, y: f64) -> f64 {
        if self.tick == 0 { self.level = y; self.tick += 1; return y; }
        let s    = self.tick % self.period;
        let prev = self.level;
        let seas = self.seasonal[s];
        self.level       = self.alpha * (y - seas) + (1.0 - self.alpha) * (prev + self.trend);
        self.trend       = self.beta  * (self.level - prev) + (1.0 - self.beta) * self.trend;
        self.seasonal[s] = self.gamma * (y - self.level) + (1.0 - self.gamma) * seas;
        self.tick       += 1;
        let ns = self.seasonal[self.tick % self.period];
        (self.level + self.trend + ns).max(0.0)
    }

    pub fn zscore(&self, actual: f64, forecast: f64, std: f64) -> f64 {
        if std < 1e-9 { return 0.0; }
        (actual - forecast).abs() / std
    }
}

// ── Ring buffer ───────────────────────────────────────────────────────────────

pub struct RingBuffer {
    buf: VecDeque<f64>,
    cap: usize,
}

impl RingBuffer {
    pub fn new(cap: usize) -> Self { Self { buf: VecDeque::with_capacity(cap), cap } }

    pub fn push(&mut self, v: f64) {
        if self.buf.len() == self.cap { self.buf.pop_front(); }
        self.buf.push_back(v);
    }

    pub fn std(&self) -> f64 {
        if self.buf.len() < 2 { return 0.0; }
        let m = self.buf.iter().sum::<f64>() / self.buf.len() as f64;
        let v = self.buf.iter().map(|x| (x - m).powi(2)).sum::<f64>()
            / (self.buf.len() - 1) as f64;
        v.sqrt()
    }
}

// ── Forecaster — reads incremental counters, not full store scans ─────────────

pub struct Forecaster {
    store:    Arc<Store>,
    config:   ForecastingConfig,
    block_tx: broadcast::Sender<BlockDecision>,
    metrics:  Arc<Metrics>,
    hw:       tokio::sync::Mutex<HoltWinters>,
    history:  tokio::sync::Mutex<RingBuffer>,
    /// Pattern learner for attack detection
    #[allow(dead_code)]
    pattern_learner: Arc<PatternLearner>,
}

impl Forecaster {
    pub fn new(
        store:    Arc<Store>,
        config:   ForecastingConfig,
        block_tx: broadcast::Sender<BlockDecision>,
        metrics:  Arc<Metrics>,
    #[allow(dead_code)]
    pattern_learner: Arc<PatternLearner>,
    ) -> Self {
        let hw = HoltWinters::new(
            config.ewma_alpha, config.hw_beta, config.hw_gamma, config.seasonality_period,
        );
        Self {
            store, config, block_tx, metrics,
            hw:      tokio::sync::Mutex::new(hw),
            history: tokio::sync::Mutex::new(RingBuffer::new(60)),
            pattern_learner,
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
        let rps     = traffic.events_last_second.load(Ordering::Relaxed) as f64;
        let n       = traffic.unique_ips_window.load(Ordering::Relaxed);

        let z = {
            let mut hw   = self.hw.lock().await;
            let mut hist = self.history.lock().await;
            let f = hw.update(rps);
            let s = hist.std().max(1.0);
            let z = hw.zscore(rps, f, s);
            hist.push(rps);
            self.metrics.set_forecast_hw(rps, z, f);
            z
        };

        debug!("HW rps={:.1} z={:.2} unique_ips={}", rps, z, n);
        if z > self.config.anomaly_zscore && n > 10 {
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
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default();

        if counts.len() < 2 {
            return;
        }
        let total: u64 = counts.iter().sum();
        if total < 100 {
            return;
        }
        let h = shannon_entropy(&counts, total);
        self.metrics.set_entropy(h);
        debug!("entropy H={:.3} bits ({} subnets)", h, counts.len());
        if h < self.config.min_entropy {
            warn!("LOW ENTROPY H={:.3}", h);
            self.entropy_block().await;
        }
    }

    async fn preemptive_block(&self) {
        let sample = self
            .store
            .traffic
            .threat_sample
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default();

        let mut n = 0usize;
        for (ip, threat) in sample {
            if threat <= 0.7 {
                continue;
            }
            let _ = self.block_tx.send(BlockDecision {
                ip,
                reason: BlockReason::ForecastAnomaly,
                ttl_secs: Some(300),
                batch_subnet: None,
            });
            self.metrics.record_block(&ip.to_string(), "forecast_anomaly", "forecasting");
            self.metrics.blocks_forecast.fetch_add(1, Ordering::Relaxed);
            n += 1;
        }
        if n > 0 {
            info!("pre-emptive blocks: {}", n);
        }
    }

    async fn entropy_block(&self) {
        // Use bounded threat_sample instead of scanning the full store
        // This maintains O(1) cost regardless of store size
        let sample = self
            .store
            .traffic
            .threat_sample
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default();

        if sample.is_empty() {
            return;
        }

        // Sort by request_count (desc) - use threat_score as available metric
        let mut top: Vec<(std::net::IpAddr, f32)> = sample.into_iter().collect();
        top.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let cut = (top.len() / 10).max(1).min(50);
        let mut n = 0usize;

        for (ip, _threat) in top.iter().take(cut) {
            let _ = self.block_tx.send(BlockDecision {
                ip: *ip,
                reason: BlockReason::EntropyAnomaly,
                ttl_secs: Some(600),
                batch_subnet: None,
            });
            self.metrics.record_block(&ip.to_string(), "entropy_anomaly", "forecasting");
            self.metrics.blocks_forecast.fetch_add(1, Ordering::Relaxed);
            n += 1;
        }

        if n > 0 {
            info!("entropy blocks: {}", n);
        }
    }
}

fn shannon_entropy(counts: &[u64], total: u64) -> f64 {
    counts.iter().filter(|&&c| c > 0)
        .map(|&c| { let p = c as f64 / total as f64; -p * p.log2() })
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
        for _ in 0..50 { hw.update(1000.0); }
        assert!(hw.level > 900.0);
    }
}
