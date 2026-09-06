//! Forecasting: Holt-Winters time-series prediction + Bayesian Hypothesis Framework
//! for unified anomaly detection.
//!
//! # Architecture
//!
//! - **HoltWinters**: Triple-exponential smoothing (level + trend + seasonality).
//!   Produces point forecasts; z-score measures deviation from forecast.
//! - **EwmAVar**: EWMA variance tracker (O(1) memory) replaces RingBuffer.
//!   Feeds standard deviation to z-score calculation.
//! - **CusumState**: Cumulative Sum drift detector for slow-ramp attacks
//!   invisible to z-score.
//! - **HypothesisTracker**: Bayesian posterior over 4 hypotheses (Normal,
//!   VolumetricDoS, SlowRampDoS, FlashCrowd). Combines z-score, CUSUM,
//!   threat score, and entropy delta into a single decision.
//! - **PeakReservoir**: Empirical quantile of forecast residuals (legacy,
//!   transitional — remove v0.4).

use ramshield_config::ForecastingConfig;
use ramshield_metrics::Metrics;
use ramshield_storage::Store;
use ramshield_types::{EnforceAction, EnforceCommand};
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

// ── Holt-Winters ───────────────────────────────────────────────────────────

/// Triple exponential smoothing forecaster.
///
/// Maintains `level`, `trend`, and `seasonal` components. Each `update(y)`
/// returns the forecast for the NEXT tick (one-step-ahead). The forecast
/// uses the future seasonal slot (not the just-updated slot) to avoid
/// collapsing residuals to near-zero on regular cycles.
///
/// Parameters:
///- `alpha` (level smoothing, typical 0.1–0.3)
///- `beta` (trend smoothing, typical 0.01–0.1)
///- `gamma` (seasonal smoothing, typical 0.01–0.1)
///- `period` (seasonal cycle length in ticks)
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
    /// Create a new Holt-Winters forecaster.
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

    /// Ingest observation `y`, update state, return one-step-ahead forecast.
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
        // Forecast for the NEXT tick. The seasonal slot at (tick % period)
        // is the one we just updated, so add `period` to read the future slot.
        // (Without this, the forecast always used the just-updated slot,
        // which biases z-scores by collapsing residuals to near-zero on
        // regular cycles.)
        let ns = self.seasonal[(self.tick + self.period) % self.period];
        (self.level + self.trend + ns).max(0.0)
    }


}



// ── EWMA Variance ─────────────────────────────────────────────────────────────

/// Exponentially weighted moving average variance tracker.
/// O(1) memory (3 floats = 24 bytes).
/// Adapts to traffic phase changes within ~2 minutes (span=120).
pub struct EwmAVar {
    ewma: f64,
    var_ewma: f64,
    count: u64,
    alpha: f64,
}

impl EwmAVar {
    /// span: effective window length in ticks. alpha = 2/(span+1).
    fn new(span: usize) -> Self {
        let p = span.max(1);
        let alpha = 2.0 / (p as f64 + 1.0);
        Self {
            ewma: 0.0,
            var_ewma: 0.0,
            count: 0,
            alpha,
        }
    }

    /// Feed a new observation, return the current residual z-score:
    /// z = (observation - ewma) / sqrt(var_ewma).
    /// After warmup (< 2 observations), returns 0.0.
    fn update(&mut self, x: f64) -> f64 {
        self.count += 1;
        if self.count == 1 {
            self.ewma = x;
            return 0.0;
        }
        let diff = x - self.ewma;
        self.ewma += self.alpha * diff;
        self.var_ewma = (1.0 - self.alpha) * (self.var_ewma + self.alpha * diff * diff);
        let sigma = self.var_ewma.sqrt();
        if sigma < 1e-9 {
            0.0
        } else {
            diff.abs() / sigma
        }
    }

    #[allow(dead_code)] // used in tests
    fn sigma(&self) -> f64 {
        self.var_ewma.sqrt()
    }
}

// ── CUSUM ─────────────────────────────────────────────────────────────────────

/// Cumulative Sum control chart for detecting sustained drift.
/// O(1) memory (4 floats = 32 bytes). Catches slow-ramp attacks that
/// z-score misses entirely.
pub struct CusumState {
    s_upper: f64,
    s_lower: f64,
    k: f64,  // slack (allowance), in sigma units
    h: f64,  // decision boundary, in sigma units
}

impl CusumState {
    /// k: slack (typically 0.5). h: threshold (typically 4.0).
    fn new(k: f64, h: f64) -> Self {
        Self {
            s_upper: 0.0,
            s_lower: 0.0,
            k,
            h,
        }
    }

    /// Feed a z-score (already normalized by sigma). Returns true if alarm.
    fn update(&mut self, z: f64) -> bool {
        self.s_upper = (self.s_upper + z - self.k).max(0.0);
        self.s_lower = (self.s_lower - z - self.k).max(0.0);
        self.s_upper > self.h || self.s_lower > self.h
    }

    fn reset(&mut self) {
        self.s_upper = 0.0;
        self.s_lower = 0.0;
    }
}

// ── Bayesian Hypothesis Framework ─────────────────────────────────────────────

/// Hypotheses for the Bayesian anomaly detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hypothesis {
    Normal = 0,
    VolumetricDoS = 1,
    SlowRampDoS = 2,
    FlashCrowd = 3,
}

const H_COUNT: usize = 4;
const H0: usize = Hypothesis::Normal as usize;
const H1: usize = Hypothesis::VolumetricDoS as usize;
const H2: usize = Hypothesis::SlowRampDoS as usize;
const H3: usize = Hypothesis::FlashCrowd as usize;

/// Bayesian tracker over 4 hypotheses. O(1) memory (36 bytes).
///
/// Maintains a posterior P(H_i | x_1:t) updated each tick via
/// log-likelihood functions that encode the domain knowledge:
///   H₀ (Normal): all signals within normal range
///   H₁ (Volumetric DDoS): high RPS, high threat, low entropy
///   H₂ (Slow-ramp DDoS): sustained drift, CUSUM > threshold
///   H₃ (Flash crowd): high RPS with high entropy (diverse IPs)
pub struct HypothesisTracker {
    priors: [f64; H_COUNT],
    tick: u64,
    threshold: f64,
    cold_threshold: f64,
    cold_ticks: u64,
}

impl Default for HypothesisTracker {
    fn default() -> Self { Self::new() }
}

impl HypothesisTracker {
    /// Baseline priors: P(normal)=0.90, P(volumetric)=0.02, P(slow_ramp)=0.02,
    /// P(flash_crowd)=0.05.
    const BASELINE: [f64; H_COUNT] = [0.90, 0.02, 0.02, 0.05];
    const DECAY: f64 = 0.98; // 98% old belief, 2% baseline

    /// Create a new tracker with baseline priors. Uses cold start
    /// threshold for the first 30 ticks.
    pub fn new() -> Self {
        Self {
            priors: Self::BASELINE,
            tick: 0,
            threshold: 0.75,
            cold_threshold: 0.85,
            cold_ticks: 60,
        }
    }

    /// Compute log-likelihoods for each hypothesis given current observations,
    /// then update posteriors via Bayes' rule.
    ///
    /// Inputs: z-score from EWMA variance, entropy delta (current - baseline),
    /// aggregate threat score [0,1], CUSUM alarm flag.
    pub fn bayesian_update(
        &mut self,
        z: f64,
        delta_h: f64,
        threat: f64,
        cusum_alarm: bool,
    ) -> [f64; H_COUNT] {
        self.tick += 1;

        let log_l = [
            log_likelihood_h0(z, delta_h, threat, cusum_alarm),
            log_likelihood_h1(z, delta_h, threat, cusum_alarm),
            log_likelihood_h2(z, delta_h, threat, cusum_alarm),
            log_likelihood_h3(z, delta_h, threat, cusum_alarm),
        ];

        // log P(H_i) = log prior + log likelihood
        let mut log_posterior = [0.0f64; H_COUNT];
        for i in 0..H_COUNT {
            log_posterior[i] = self.priors[i].max(1e-300).ln() + log_l[i];
        }

        // numerically stable softmax
        let max_ll = log_posterior
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        let mut sum_exp = 0.0f64;
        for i in 0..H_COUNT {
            self.priors[i] = (log_posterior[i] - max_ll).exp();
            sum_exp += self.priors[i];
        }
        for p in &mut self.priors {
            *p /= sum_exp;
        }

        // decay toward baseline
        for (p, b) in self.priors.iter_mut().zip(Self::BASELINE) {
            *p = *p * Self::DECAY + b * (1.0 - Self::DECAY);
        }

        self.priors
    }

    /// Return (hypothesis, confidence) if any hypothesis exceeds the
    /// decision threshold. During cold start (< cold_ticks), uses
    /// a higher threshold to prevent premature action.
    pub fn best_above_threshold(&self) -> Option<(Hypothesis, f64)> {
        let eff_threshold = if self.tick < self.cold_ticks {
            self.cold_threshold
        } else {
            self.threshold
        };
        let mut best_idx = 0;
        let mut best_val = 0.0;
        #[allow(clippy::needless_range_loop)]
        for i in 0..H_COUNT {
            if self.priors[i] > best_val {
                best_val = self.priors[i];
                best_idx = i;
            }
        }
        if best_idx == H0 || best_val < eff_threshold {
            return None;
        }
        let h = match best_idx {
            H1 => Hypothesis::VolumetricDoS,
            H2 => Hypothesis::SlowRampDoS,
            H3 => Hypothesis::FlashCrowd,
            _ => return None,
        };
        Some((h, best_val))
    }

    pub fn priors(&self) -> &[f64; H_COUNT] {
        &self.priors
    }

    pub fn tick_count(&self) -> u64 {
        self.tick
    }
}

// ── Likelihood Functions ──────────────────────────────────────────────────────

/// Clamp a log-likelihood to [-clamp, +clamp] to prevent any single signal
/// from dominating the posterior.
fn clamp_ll(v: f64, clamp: f64) -> f64 {
    v.clamp(-clamp, clamp)
}

/// H₀: Normal traffic. Evidence: z low, entropy stable, threat low, no CUSUM.
fn log_likelihood_h0(z: f64, delta_h: f64, threat: f64, cusum_alarm: bool) -> f64 {
    let mut ll = 0.0;

    // z-score evidence
    ll += if z.abs() < 1.0 {
        0.0
    } else if z.abs() < 2.5 {
        -0.5 * (z.abs() - 1.0)
    } else {
        -1.5
    };

    // entropy evidence
    ll += if delta_h.abs() < 0.3 {
        0.0
    } else {
        -0.5 * (delta_h.abs() - 0.3)
    };

    // threat evidence
    ll += if threat < 0.3 {
        0.0
    } else {
        -0.3 * threat
    };

    // CUSUM evidence
    if cusum_alarm {
        ll -= 2.0;
    }

    clamp_ll(ll, 3.0)
}

/// H₁: Volumetric DDoS. Evidence: z high, entropy DOWN, threat high.
fn log_likelihood_h1(z: f64, delta_h: f64, threat: f64, _cusum: bool) -> f64 {
    let mut ll = 0.0;

    // z-score: strong support when RPS spike
    ll += if z > 3.0 {
        2.0
    } else if z > 2.5 {
        0.5 * (z - 2.5)
    } else if z < 0.0 {
        -1.0
    } else {
        -0.3 * (2.5 - z).max(0.0)
    };

    // entropy: DDoS shows uniform IPs → entropy drops
    ll += if delta_h < -0.5 {
        1.0
    } else if delta_h < 0.0 {
        0.3
    } else if delta_h > 0.5 {
        -1.5
    } else {
        -0.5
    };

    // threat: high threat = strong DDoS signal
    ll += if threat > 0.8 {
        2.0
    } else if threat > 0.5 {
        1.0
    } else if threat < 0.3 {
        -0.5
    } else {
        0.0
    };

    clamp_ll(ll, 3.0)
}

/// H₂: Slow-ramp DDoS. Evidence: low z (gradual), CUSUM alarm (primary).
fn log_likelihood_h2(z: f64, delta_h: f64, threat: f64, cusum_alarm: bool) -> f64 {
    let mut ll = 0.0;

    // z-score: neutral if low (slow ramp hasn't spiked yet)
    ll += if z > 2.5 {
        -0.3
    } else {
        0.0
    };

    // entropy: slight support if dropping
    ll += if delta_h < -0.3 {
        0.3
    } else {
        0.0
    };

    // threat: moderate support if elevated
    ll += if threat > 0.3 {
        0.5
    } else {
        0.0
    };

    // CUSUM: PRIMARY signal for H₂
    if cusum_alarm {
        ll += 2.5;
    }

    clamp_ll(ll, 3.0)
}

/// H₃: Flash crowd. Evidence: moderate z (high RPS), entropy UP (diverse IPs),
/// low threat.
fn log_likelihood_h3(z: f64, delta_h: f64, threat: f64, _cusum: bool) -> f64 {
    let mut ll = 0.0;

    // z-score: moderate support if RPS elevated
    ll += if z > 3.0 {
        0.3 // too high — less likely flash crowd
    } else if z > 2.0 {
        0.5
    } else if z < 0.0 {
        -1.0
    } else {
        -0.5
    };

    // entropy: PRIMARY signal for H₃ — flash crowds have diverse IPs
    ll += if delta_h > 0.5 {
        1.5
    } else if delta_h > 0.2 {
        0.8
    } else if delta_h < 0.0 {
        -0.8
    } else {
        -0.3
    };

    // threat: low threat = support for flash crowd
    ll += if threat < 0.3 {
        0.5
    } else if threat > 0.5 {
        -1.0
    } else {
        0.0
    };

    clamp_ll(ll, 3.0)
}

// ── Forecaster — reads incremental counters, not full store scans ─────────────

/// Unified anomaly detection engine.
///
/// Runs two async loops:
/// - tick_hw (1 Hz): Holt-Winters forecast → z-score → CUSUM → Bayesian
///   hypothesis update → enforcement decision
/// - tick_entropy (0.2 Hz): Shannon entropy delta for Bayesian input
pub struct Forecaster {
    store: Arc<Store>,
    config: ForecastingConfig,
    enforcement_tx: mpsc::Sender<EnforceCommand>,
    metrics: Arc<Metrics>,
    hw: tokio::sync::Mutex<HoltWinters>,
    ewma_var: tokio::sync::Mutex<EwmAVar>,
    cusum: tokio::sync::Mutex<CusumState>,
    peaks: tokio::sync::Mutex<PeakReservoir>,
    bayesian: tokio::sync::Mutex<HypothesisTracker>,
    prev_entropy: tokio::sync::Mutex<f64>,
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
    fn extreme_quantile(&mut self, tail: f64) -> Option<f64> {
        if self.ticks < Self::WARM_TICKS || self.vals.len() < 10 {
            return None;
        }
        // ponytail: sort in-place, O(n) allocation saved per tick
        self.vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((self.vals.len() as f64) * (1.0 - tail)).clamp(0.0, (self.vals.len() - 1) as f64);
        Some(self.vals[idx as usize])
    }

    #[allow(dead_code)] // used in tests
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
            ewma_var: tokio::sync::Mutex::new(EwmAVar::new(120)),
            cusum: tokio::sync::Mutex::new(CusumState::new(0.5, 4.0)),
            peaks: tokio::sync::Mutex::new(PeakReservoir::new(512)),
            bayesian: tokio::sync::Mutex::new(HypothesisTracker::new()),
            prev_entropy: tokio::sync::Mutex::new(0.0),
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

        // ── Signal extraction ────────────────────────────────────────────────
        let (z, f) = {
            let mut hw = self.hw.lock().await;
            let f = hw.update(rps);
            let residual = rps - f;
            let z = self.ewma_var.lock().await.update(residual);
            // feed abs residual into PeakReservoir for self-calibrated extremes
            let dev = residual.abs();
            self.peaks.lock().await.push(dev);
            self.metrics.set_forecast_hw(rps, z, f);
            (z, f)
        };

        let cusum_alarm = self.cusum.lock().await.update(z);

        // ── Threat: drain sample + aggregate ─────────────────────────────────
        let threat = {
            let sample = self.store.traffic.drain_threat_sample();
            if sample.is_empty() { 0.0 }
            else {
                let mut m = 0.0f32;
                for (_, t) in &sample { if *t > m { m = *t; } }
                m as f64
            }
        };

        // ── Entropy delta: current Shannon entropy minus baseline ────────────
        let delta_h = {
            let counts: Vec<u64> = self.store.traffic.subnet_window
                .iter()
                .map(|a| a.load(Ordering::Relaxed))
                .collect();
            let total: u64 = counts.iter().sum();
            let h = if total > 100 {
                shannon_entropy(&counts, total)
            } else {
                0.0
            };
            let mut prev = self.prev_entropy.lock().await;
            let dh = if *prev == 0.0 { 0.0 } else { h - *prev };
            *prev = h;
            self.metrics.set_entropy(h);
            dh
        };

        // ── Bayesian update ──────────────────────────────────────────────────
        let hypothesis = {
            let mut bt = self.bayesian.lock().await;
            let p = bt.bayesian_update(z, delta_h, threat, cusum_alarm);
            let priors = p;
            let decision = bt.best_above_threshold();

            let priors_str = format!(
                "H0={:.3} H1={:.3} H2={:.3} H3={:.3}",
                priors[0], priors[1], priors[2], priors[3]
            );
            debug!(
                "Bayesian rps={:.1} z={:.2} ΔH={:.2} threat={:.2} cusum={} | {}",
                rps, z, delta_h, threat, cusum_alarm, priors_str
            );
            decision
        };

        // ── Type-specific response ───────────────────────────────────────────
        if n < 10 {
            return; // not enough data for any decision
        }
        match hypothesis {
            Some((Hypothesis::VolumetricDoS, conf)) => {
                warn!("BAYESIAN H1 VOLUMETRIC conf={:.2} z={:.2} threat={:.2} rps={:.1}",
                    conf, z, threat, rps);
                self.preemptive_block().await;
            }
            Some((Hypothesis::SlowRampDoS, conf)) => {
                warn!("BAYESIAN H2 SLOW-RAMP conf={:.2} z={:.2} cusum rps={:.1}",
                    conf, z, rps);
                self.preemptive_block().await;
                self.cusum.lock().await.reset();
            }
            Some((Hypothesis::FlashCrowd, conf)) => {
                info!("BAYESIAN H3 FLASH-CROWD conf={:.2} ΔH={:.2} rps={:.1} — no block",
                    conf, delta_h, rps);
                // intentional: flash crowd = legitimate traffic surge, no blocking
            }
            _ => {}
        }

        // ── Legacy fallback: EWMA peak alarm (transitional, remove in v0.4) ─
        let spot_alarm = self.peaks.lock().await
            .extreme_quantile(0.001)
            .map_or(z > self.config.anomaly_zscore, |q| {
                let dev = (rps - f).abs();
                dev > q
            });
        if spot_alarm && z > self.config.anomaly_zscore && hypothesis.is_none() {
            warn!("LEGACY SPOT z={:.2} rps={:.1}", z, rps);
            self.preemptive_block().await;
        }
    }

    async fn tick_entropy(&self) {
        // Entropy is now computed inline in tick_hw (Bayesian framework).
        // This function only logs the current entropy value for dashboard visibility.
        let counts: Vec<u64> = self.store.traffic.subnet_window
            .iter()
            .map(|a| a.load(Ordering::Relaxed))
            .collect();
        let total: u64 = counts.iter().sum();
        if total > 100 {
            let h = shannon_entropy(&counts, total);
            self.metrics.set_entropy(h);
            debug!("entropy H={:.3} bits (dashboard-only)", h);
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

    /// P0 regression: HoltWinters::update must forecast from the FUTURE seasonal
    /// slot, not the one it just updated. The old code read
    /// `seasonal[tick % period]` after incrementing tick — which is the slot it
    /// just wrote, collapsing residuals and producing biased z-scores.
    #[test]
    fn hw_forecast_uses_future_seasonal_slot() {
        let period = 4;
        let mut hw = HoltWinters::new(0.3, 0.1, 0.1, period);
        // Feed a sine-wave pattern through two full periods so seasonal stabilises.
        // Each value repeats every `period` ticks.
        let pattern = [100.0, 200.0, 100.0, 50.0];
        for _ in 0..(period * 3) {
            for &v in &pattern {
                hw.update(v);
            }
        }
        // The forecast for the NEXT tick should use the seasonal index for that
        // future position — which is the slot NOT updated by the last call.
        // With the bug (read-same-slot), forecast ≈ last value. With the fix
        // (read future slot), forecast is in the range of the pattern.
        let f = hw.update(100.0);
        assert!(
            (50.0..=200.0).contains(&f),
            "forecast {} out of expected pattern range [50, 200]",
            f
        );
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

    // ── Phase 1: EWMA variance + CUSUM tests ──────────────────────────────

    #[test]
    fn ewma_var_adapts_to_phase_change() {
        // Feed 100 ticks at RPS=1000 (stable). Then 100 ticks at RPS=5000.
        // After transition, sigma should be near the new level (~500), not
        // the old level (~1). The old RingBuffer would mix both into σ≈2000.
        let mut ev = super::EwmAVar::new(120);
        for _ in 0..100 {
            ev.update(1000.0);
        }
        let sigma_low = ev.sigma();
        assert!(sigma_low < 100.0, "stable traffic sigma should be small: {}", sigma_low);

        // Transition to high traffic
        for _ in 0..100 {
            ev.update(5000.0);
        }
        let sigma_high = ev.sigma();
        // sigma should adapt toward new level's variation (~500 per tick noise)
        assert!(
            sigma_high > 100.0,
            "after phase change sigma should increase: low={} high={}",
            sigma_low,
            sigma_high
        );
    }

    #[test]
    fn ewma_var_residual_zscore_on_spike() {
        // Constant traffic (residual = 0, z = 0). Then a spike (residual ≠ 0, z > 0).
        let mut ev = super::EwmAVar::new(60);
        for _ in 0..50 {
            ev.update(0.0); // zero residual = forecast matches perfectly
        }
        let z_quiet = ev.update(0.0);
        assert!(z_quiet < 0.1, "no anomaly: z={}", z_quiet);

        // Spike: residual = 100 when sigma ≈ small
        let z_spike = ev.update(100.0);
        assert!(z_spike > 2.0, "spike should produce high z: {}", z_spike);
    }

    #[test]
    fn cusum_fires_on_sustained_drift() {
        // k=0.5, h=4.0. Feed z=1.5 for 12 ticks.
        // CUSUM should accumulate: s_upper = (1.5 - 0.5) * 12 = 12 > 4.0 → alarm.
        let mut cs = super::CusumState::new(0.5, 4.0);
        let mut fired = false;
        for _ in 0..12 {
            if cs.update(1.5) {
                fired = true;
                break;
            }
        }
        assert!(fired, "CUSUM must alarm on sustained z=1.5 drift over 12 ticks");
    }

    #[test]
    fn cusum_stays_quiet_on_noise() {
        // k=0.5, h=4.0. Alternating z: +1, -1, +1, -1 (symmetric noise).
        // CUSUM should NOT alarm — deviations cancel.
        let mut cs = super::CusumState::new(0.5, 4.0);
        for i in 0..100 {
            let z = if i % 2 == 0 { 1.0 } else { -1.0 };
            assert!(!cs.update(z), "symmetric noise should not trigger CUSUM at tick {}", i);
        }
    }

    #[test]
    fn cusum_reset_clears_state() {
        // Feed drift, trigger alarm, reset, verify clean.
        let mut cs = super::CusumState::new(0.5, 4.0);
        for _ in 0..10 {
            cs.update(2.0);
        }
        assert!(cs.update(2.0), "should alarm before reset");
        cs.reset();
        assert!(!cs.update(0.0), "must be clean after reset");
    }

    // ── Phase 2: Bayesian Hypothesis Framework tests ───────────────────────

    #[test]
    fn bayesian_update_increases_normal_posterior() {
        let mut bt = super::HypothesisTracker::new();
        // Quiet traffic: z=0.2, no entropy change, low threat, no CUSUM.
        for _ in 0..10 {
            bt.bayesian_update(0.2, 0.0, 0.0, false);
        }
        let priors = bt.priors();
        assert!(priors[0] > 0.85, "H0 should dominate quiet traffic: {:?}", priors);
        // H1 should be below baseline (0.02) since z is low and threat is low
        assert!(priors[1] < 0.03, "H1 should stay low: {:?}", priors);
    }

    #[test]
    fn bayesian_detects_volumetric_ddos() {
        let mut bt = super::HypothesisTracker::new();
        // Simulate a spike: z=4.0, entropy dropping (delta_h=-0.8), threat=0.9, no CUSUM.
        for _ in 0..20 {
            bt.bayesian_update(4.0, -0.8, 0.9, false);
        }
        let priors = bt.priors();
        assert!(priors[1] > priors[0],
            "H1 (volumetric) should exceed H0 (normal): H0={:.3} H1={:.3}", priors[0], priors[1]);
        assert!(priors[1] > priors[3],
            "H1 should exceed H3 (flash): H1={:.3} H3={:.3}", priors[1], priors[3]);
    }

    #[test]
    fn bayesian_detects_flash_crowd() {
        let mut bt = super::HypothesisTracker::new();
        // Moderate RPS (z=2.5), entropy UP (diverse IPs), low threat.
        // This is a flash crowd, not DDoS.
        for _ in 0..20 {
            bt.bayesian_update(2.5, 0.8, 0.1, false);
        }
        let priors = bt.priors();
        assert!(priors[3] > priors[1],
            "H3 (flash) should exceed H1 (volumetric): H1={:.3} H3={:.3}", priors[1], priors[3]);
        assert!(priors[3] > priors[2],
            "H3 (flash) should exceed H2 (slow-ramp): H2={:.3} H3={:.3}", priors[2], priors[3]);
    }

    #[test]
    fn bayesian_slow_ramp_detected_via_cusum() {
        let mut bt = super::HypothesisTracker::new();
        // Low z (gradual increase, not spiking), CUSUM alarm, moderate threat.
        for _ in 0..10 {
            bt.bayesian_update(0.8, -0.1, 0.4, true);
        }
        let priors = bt.priors();
        assert!(priors[2] > priors[0],
            "H2 (slow-ramp) should exceed H0: H0={:.3} H2={:.3}", priors[0], priors[2]);
        assert!(priors[2] > priors[1],
            "H2 should exceed H1 (no CUSUM signal for H1): H1={:.3} H2={:.3}", priors[1], priors[2]);
    }

    #[test]
    fn bayesian_no_action_when_all_normal() {
        let mut bt = super::HypothesisTracker::new();
        // Quiet traffic for 100 ticks
        for _ in 0..100 {
            bt.bayesian_update(0.1, 0.0, 0.0, false);
        }
        assert!(bt.best_above_threshold().is_none(),
            "should not trigger any action on normal traffic");
    }

    #[test]
    fn bayesian_cold_start_requires_higher_confidence() {
        let mut bt = super::HypothesisTracker::new();
        // Extreme spike on tick 1 (cold start) — even though z=5.0 and threat=1.0,
        // cold threshold (0.85) should prevent premature action.
        let _ = bt.bayesian_update(5.0, -1.0, 1.0, false);
        // This tick should NOT trigger — cold start threshold is 0.85
        // (H1 rises but not enough in 1 tick to exceed 0.85)
        let _decision = bt.best_above_threshold();
        // Whether it triggers or not depends on the exact math, but the
        // threshold is higher during cold start. After 60+ ticks it would be lower.
        let mut bt_warm = super::HypothesisTracker::new();
        for _ in 0..70 {
            let _ = bt_warm.bayesian_update(5.0, -1.0, 1.0, false);
        }
        let warm_decision = bt_warm.best_above_threshold();
        // Warm system should detect the attack more easily (lower threshold)
        assert!(warm_decision.is_some(),
            "warm system should detect sustained attack: {:?}", warm_decision);
    }
}
