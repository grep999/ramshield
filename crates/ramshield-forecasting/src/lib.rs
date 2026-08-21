use ramshield_config::ForecastingConfig;
use ramshield_learning::PatternLearner;
use ramshield_metrics::Metrics;
use ramshield_storage::Store;
use ramshield_types::{EnforceAction, EnforceCommand};
use ramshield_types::error::BlockReason;
use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

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
    command_tx: mpsc::Sender<EnforceCommand>,
    metrics: Arc<Metrics>,
    hw: tokio::sync::Mutex<HoltWinters>,
    history: tokio::sync::Mutex<RingBuffer>,
    #[allow(dead_code)]
    pattern_learner: Arc<PatternLearner>,
}

impl Forecaster {
    pub fn new(
        store: Arc<Store>,
        config: ForecastingConfig,
        command_tx: mpsc::Sender<EnforceCommand>,
        metrics: Arc<Metrics>,
        #[allow(dead_code)] pattern_learner: Arc<PatternLearner>,
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
            command_tx,
            metrics,
            hw: tokio::sync::Mutex::new(hw),
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
        let rps = traffic.events_last_second.load(Ordering::Relaxed) as f64;
        let n = traffic.unique_ips_window.load(Ordering::Relaxed);

        let z = {
            let mut hw = self.hw.lock().await;
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
            .iter()
            .map(|a| a.load(Ordering::Relaxed))
            .collect();
        if counts.iter().all(|&c| c == 0) {
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

    fn snapshot_threat_sample(&self) -> Vec<(IpAddr, f32)> {
        let sample = self.store.traffic.drain_threat_sample();
        for item in &sample {
            self.store.traffic.threat_sample.push(*item);
        }
        sample
    }

    fn block_cmd(&self, ip: IpAddr, _reason: BlockReason, ttl_seconds: u64) -> EnforceCommand {
        EnforceCommand {
            decision_id: Uuid::new_v4(),
            policy_version: 1,
            source: "forecasting".into(),
            actor: "forecaster".into(),
            timestamp_utc: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            ttl_seconds,
            reason: "forecast_anomaly".into(),
            ip,
            action: EnforceAction::Block,
        }
    }

    async fn preemptive_block(&self) {
        let sample = self.snapshot_threat_sample();
        if sample.is_empty() {
            return;
        }
        let mut n = 0usize;
        for (ip, score) in sample {
            if score <= 0.7 {
                continue;
            }
            if self
                .command_tx
                .send(self.block_cmd(ip, BlockReason::ForecastAnomaly, 300))
                .await
                .is_ok()
            {
                self.metrics
                    .record_block(&ip.to_string(), "forecast_anomaly", "forecasting");
                n += 1;
            }
        }
        if n > 0 {
            info!("pre-emptive blocks: {}", n);
        }
    }

    async fn entropy_block(&self) {
        let mut top = self.snapshot_threat_sample();
        if top.is_empty() {
            return;
        }
        top.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let cut = (top.len() / 10).clamp(1, 50);
        let mut n = 0usize;
        for (ip, _) in top.iter().take(cut) {
            if self
                .command_tx
                .send(self.block_cmd(*ip, BlockReason::EntropyAnomaly, 600))
                .await
                .is_ok()
            {
                self.metrics
                    .record_block(&ip.to_string(), "entropy_anomaly", "forecasting");
                n += 1;
            }
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
}
