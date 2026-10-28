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
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

mod forecaster;
mod hypothesis;
mod models;

pub use models::shannon_entropy;

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

// ── EWMA Variance ─────────────────────────────────────────────────────────────

/// Exponentially weighted moving average variance tracker.
/// O(1) memory (4 fields = 32 bytes).
/// Adapts to traffic phase changes within ~2 minutes (span=120).
pub struct EwmAVar {
    ewma: f64,
    var_ewma: f64,
    count: u64,
    alpha: f64,
}

// ── CUSUM ─────────────────────────────────────────────────────────────────────

/// Cumulative Sum control chart for detecting sustained drift.
/// O(1) memory (4 floats = 32 bytes). Catches slow-ramp attacks that
/// z-score misses entirely.
pub struct CusumState {
    s_upper: f64,
    s_lower: f64,
    k: f64, // slack (allowance), in sigma units
    h: f64, // decision boundary, in sigma units
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
const CLAMP_LL: f64 = 4.0; // ponytail: was 3.0, raised for multi-signal coherence

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

// ── Forecaster — reads incremental counters, not full store scans ─────────────

/// Unified anomaly detection engine.
///
/// Runs two async loops:
/// - tick_hw (1 Hz): Holt-Winters forecast → z-score → CUSUM → Bayesian
///   hypothesis update → enforcement decision
/// - Shannon entropy computed inline in tick_hw for Bayesian input
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
    // Cooldown gate: SLOW-RAMP WARN emitted at most once per WARN_COOLDOWN_MS
    // while the hypothesis persists; quieter debug ticks in between.
    last_slow_ramp_warn_ms: std::sync::atomic::AtomicU64,
    /// Optional lock-free snapshot publisher; keeps the detector crate out of
    /// forecasting's dependency graph while allowing the daemon to bridge context.
    context_publisher: Option<Arc<dyn Fn(Option<(Hypothesis, f64)>, u64) + Send + Sync + 'static>>,
}

/// Bounded reservoir of positive deviations; `extreme_q` returns the value that
/// exceeds (1 − 1/q_target) of observed peaks. Warm-up falls back to z-score.
struct PeakReservoir {
    vals: Vec<f64>,
    cap: usize,
    ticks: u64,
}

#[cfg(test)]
mod tests;
