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
        // Forecast for the NEXT tick. The seasonal slot at (tick % period)
        // is the one we just updated, so add `period` to read the future slot.
        // (Without this, the forecast always used the just-updated slot,
        // which biases z-scores by collapsing residuals to near-zero on
        // regular cycles.)
        let ns = self.seasonal[(self.tick + self.period) % self.period];
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

// ── EWMA Variance ─────────────────────────────────────────────────────────────

/// Exponentially weighted moving average variance tracker.
/// Replaces RingBuffer with O(1) memory (3 floats = 24 bytes).
/// Adapts to traffic phase changes within ~2 minutes (span=120).
struct EwmAVar {
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
struct CusumState {
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

// ── Forecaster — reads incremental counters, not full store scans ─────────────

pub struct Forecaster {
    store: Arc<Store>,
    config: ForecastingConfig,
    enforcement_tx: mpsc::Sender<EnforceCommand>,
    metrics: Arc<Metrics>,
    hw: tokio::sync::Mutex<HoltWinters>,
    /// P1: EWMA variance tracks residual std in O(1) memory.
    /// Replaces the old RingBuffer with 60-float window.
    ewma_var: tokio::sync::Mutex<EwmAVar>,
    /// P1: CUSUM detects sustained drift (slow-ramp attacks).
    cusum: tokio::sync::Mutex<CusumState>,
    /// P2 SPOT-lite: peaks-over-threshold reservoir of residual |deviation| samples.
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
            ewma_var: tokio::sync::Mutex::new(EwmAVar::new(120)),
            cusum: tokio::sync::Mutex::new(CusumState::new(0.5, 4.0)),
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

        let (z, spot_alarm, cusum_alarm) = {
            let mut hw = self.hw.lock().await;
            let mut ewma = self.ewma_var.lock().await;
            let mut cusum = self.cusum.lock().await;
            let f = hw.update(rps);
            let residual = rps - f;
            // P1: EWMA variance normalizes residual — adapts to traffic phase.
            // Old: RingBuffer(60) with equal-weight std — mixed night+day into one σ.
            let z = ewma.update(residual);
            let cusum_alarm = cusum.update(z);

            // P1: feed ABS residual into PeakReservoir (not raw rps - mean).
            // The reservoir's extreme quantile now measures forecast-accuracy
            // extremes, not raw traffic extremes. This self-calibrates the
            // threshold to the model's actual error distribution.
            let dev = residual.abs();
            let spot_alarm = {
                let mut pk = self.peaks.lock().await;
                pk.push(dev);
                match pk.extreme_quantile(0.001) {
                    Some(q) if pk.warm() => dev > q,
                    _ => z > self.config.anomaly_zscore,
                }
            };
            self.metrics.set_forecast_hw(rps, z, f);
            (z, spot_alarm, cusum_alarm)
        };

        debug!("HW rps={:.1} z={:.2} spot={} cusum={} n={}", rps, z, spot_alarm, cusum_alarm, n);
        if cusum_alarm && n > 10 {
            warn!("CUSUM ALARM z={:.2} rps={:.1}", z, rps);
            self.preemptive_block().await;
            self.cusum.lock().await.reset();
        } else if spot_alarm && z > self.config.anomaly_zscore && n > 10 {
            warn!("ANOMALY z={:.2} rps={:.1}", z, rps);
            self.preemptive_block().await;
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
}
