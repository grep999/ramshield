use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

use crate::config::ForecastingConfig;
use crate::metrics::Metrics;
use crate::storage::Store;
use crate::error::RamShieldError;

/// Holt-Winters double exponential smoothing for traffic forecasting.
///
/// Based on: Hyndman & Athanasopoulos, "Forecasting: Principles and Practice"
/// L(t) = α·Y(t) + (1-α)·(L(t-1) + T(t-1))
/// T(t) = β·(L(t) - L(t-1)) + (1-β)·T(t-1)
/// F(t+1) = L(t) + T(t)
struct HoltWinters {
    level: f64,
    trend: f64,
    alpha: f64,
    beta: f64,
}

impl HoltWinters {
    fn new(alpha: f64, beta: f64) -> Self {
        Self {
            level: 0.0,
            trend: 0.0,
            alpha: alpha.clamp(0.0, 1.0),
            beta: beta.clamp(0.0, 1.0),
        }
    }

    fn update(&mut self, observation: f64) -> f64 {
        if self.level == 0.0 && self.trend == 0.0 {
            self.level = observation;
            return observation;
        }
        let prev_level = self.level;
        self.level = self.alpha * observation + (1.0 - self.alpha) * (self.level + self.trend);
        self.trend = self.beta * (self.level - prev_level) + (1.0 - self.beta) * self.trend;
        self.level + self.trend
    }
}

/// Shannon entropy calculator for traffic pattern analysis.
///
/// H = -Σ p(x) · log2(p(x))
/// Normalized: H_norm = H / log2(N) ∈ [0, 1]
/// High entropy = unpredictable traffic = potential attack indicator.
struct EntropyCalculator {
    counts: HashMap<u64, u64>,
    total: u64,
}

impl EntropyCalculator {
    fn new() -> Self {
        Self { counts: HashMap::new(), total: 0 }
    }

    fn record(&mut self, value: u64) {
        *self.counts.entry(value).or_insert(0) += 1;
        self.total += 1;
    }

    fn entropy(&self) -> f64 {
        if self.total == 0 { return 0.0; }
        let t = self.total as f64;
        self.counts.values()
            .filter(|&&c| c > 0)
            .map(|&c| {
                let p = c as f64 / t;
                -p * p.log2()
            })
            .sum()
    }

    fn normalized_entropy(&self) -> f64 {
        let e = self.entropy();
        let max_e = if self.total > 0 { (self.total as f64).log2() } else { 1.0 };
        if max_e > 0.0 { (e / max_e).clamp(0.0, 1.0) } else { 0.0 }
    }
}

/// Threat assessor using Z-score anomaly detection on forecast errors.
///
/// z = (error - mean_error) / std_error
/// Combined with entropy: threat = 0.6·min(z/threshold, 1) + 0.4·entropy_score
struct ThreatAssessor {
    errors: Vec<f64>,
    max_errors: usize,
}

impl ThreatAssessor {
    fn new() -> Self {
        Self { errors: Vec::new(), max_errors: 60 }
    }

    fn assess(&mut self, actual: f64, forecast: f64, entropy: f64, entropy_threshold: f64) -> f64 {
        let error = (actual - forecast).abs();
        self.errors.push(error);
        if self.errors.len() > self.max_errors { self.errors.remove(0); }

        let (mean_err, std_err) = if self.errors.is_empty() {
            (0.0, 0.0)
        } else {
            let n = self.errors.len() as f64;
            let mean = self.errors.iter().sum::<f64>() / n;
            let var = self.errors.iter().map(|&e| (e - mean).powi(2)).sum::<f64>() / n;
            (mean, var.sqrt())
        };

        let anomaly_score = if std_err > 0.0 {
            ((error - mean_err) / std_err / 3.0).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let entropy_score = if entropy > entropy_threshold {
            ((entropy - entropy_threshold) / (1.0 - entropy_threshold)).clamp(0.0, 1.0)
        } else {
            0.0
        };

        (anomaly_score * 0.6 + entropy_score * 0.4).clamp(0.0, 1.0)
    }
}

pub struct ForecastingEngine {
    config: ForecastingConfig,
    metrics: Arc<Metrics>,
    _storage: Arc<Store>,
    hw: Arc<parking_lot::Mutex<HoltWinters>>,
    entropy: Arc<parking_lot::Mutex<EntropyCalculator>>,
    threat: Arc<parking_lot::Mutex<ThreatAssessor>>,
    ticks: Arc<parking_lot::Mutex<u64>>,
}

impl ForecastingEngine {
    pub fn new(config: ForecastingConfig, metrics: Arc<Metrics>, storage: Arc<Store>) -> Self {
        let hw_alpha = config.hw_alpha;
        let hw_beta = config.hw_beta;
        Self {
            config,
            metrics,
            _storage: storage,
            hw: Arc::new(parking_lot::Mutex::new(HoltWinters::new(hw_alpha, hw_beta))),
            entropy: Arc::new(parking_lot::Mutex::new(EntropyCalculator::new())),
            threat: Arc::new(parking_lot::Mutex::new(ThreatAssessor::new())),
            ticks: Arc::new(parking_lot::Mutex::new(0)),
        }
    }

    pub async fn start(&self, mut shutdown_rx: broadcast::Receiver<()>) -> Result<(), RamShieldError> {
        info!("ForecastingEngine: Starting...");
        let interval_secs = self.config.interval_secs.max(1);
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.tick().await;
                }
                _ = shutdown_rx.recv() => {
                    info!("ForecastingEngine: Shutting down.");
                    break;
                }
            }
        }
        Ok(())
    }

    async fn tick(&self) {
        *self.ticks.lock() += 1;

        let current_rps = self.estimate_rps();

        let forecast = {
            let mut hw = self.hw.lock();
            hw.update(current_rps)
        };

        let error = (current_rps - forecast).abs();
        {
            let mut ent = self.entropy.lock();
            ent.record(error as u64);
            let norm_ent = ent.normalized_entropy();
            self.metrics.set_entropy(norm_ent);
        }

        let threat_score = {
            let mut t = self.threat.lock();
            let ent_val = self.entropy.lock().entropy();
            t.assess(current_rps, forecast, ent_val, self.config.entropy_threshold)
        };

        self.metrics.set_forecast_hw(forecast, threat_score, current_rps);

        if threat_score > 0.7 {
            info!("ForecastingEngine: High threat detected (score={:.2}, forecast={:.0}, actual={:.0})",
                threat_score, forecast, current_rps);
        }
    }

    fn estimate_rps(&self) -> f64 {
        let history = self.metrics.get_batch_history();
        if let Some(last) = history.last() {
            return last.events as f64;
        }
        0.0
    }
}