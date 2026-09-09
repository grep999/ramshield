# ramshield-forecasting

```text
Store.get_batch_history() ──→ Forecaster::tick()
                                    │
                    ┌───────────────┼───────────────┐
                    ↓               ↓               ↓
              HoltWinters     CUSUM          Bayesian
              (seasonality)   (slow ramp)    (hypothesis)
                    │               │               │
                    └───────┬───────┘───────────────┘
                            ↓
                    Anomaly z-score ──→ EnforceCommand
                    + entropy
```

## Why it exists

The detection crate catches obvious attacks — high rate, many IPs, status code anomalies. But a slow ramp that stays below every individual threshold escapes detection. The forecasting crate runs statistical models on the *trend* of events over time, catching attacks that look normal in any single sample but anomalous across multiple windows.

## How it works

### Tick Cycle

`Forecaster::tick()` runs periodically (default: every 10 seconds). It pulls the last 120 batch records from `Store::get_batch_history()` — a ring buffer of recent detection flushes — and feeds them to three independent models.

### Holt-Winters Triple Exponential Smoothing

A seasonal trend model with three parameters:
- `alpha` (level smoothing): how quickly the baseline adjusts to new data.
- `beta` (trend smoothing): how quickly the trend direction changes.
- `gamma` (seasonality): how strongly daily/weekly patterns influence the forecast.

`HoltWinters::update(y)` returns the one-step-ahead forecast. If the actual value `y` deviates from the forecast by more than `anomaly_zscore` standard deviations, the data point is flagged as anomalous.

```rust
pub struct HoltWinters {
    level: f64,
    trend: f64,
    seasonal: Vec<f64>,
    alpha: f64,
    beta: f64,
    gamma: f64,
    period: usize,  // default: 120 ticks = 20 minutes
}
```

### EWMA Variance Tracker

Internal to the forecaster — `EwmAVar` tracks mean and variance of the event rate with O(1) memory (3 floats). It provides the standard deviation estimate that the anomaly z-score calculation uses. Not publicly exposed.

### CUSUM (Cumulative Sum) Detector

A second change-point detector, independent of the CUSUM in the detection crate. This one operates on the *batch-level* event rate (events per 500ms window) rather than per-IP rates:

```rust
pub struct CusumState {
    cusum_value: f64,
    baseline: f64,
    warmup_samples: u8,
    threshold: f64,
}
```

The detection crate's CUSUM catches individual IPs drifting upward. This one catches the *aggregate* rate drifting — a slow increase across the entire traffic stream that no single IP triggers.

### Bayesian Hypothesis Tracker

Classifies the current traffic state into one of four hypotheses:

```rust
pub enum Hypothesis {
    Normal,       // H0: baseline traffic
    FlashCrowd,   // H1: legitimate spike (product launch, viral link)
    SlowRamp,     // H2: coordinated slow ramp attack
    Volumetric,   // H3: fast flood
}
```

`bayesian_update(z, delta_h, threat, cusum_alarm)` computes posterior probabilities for all four hypotheses using Bayes' rule. The model uses pre-computed likelihood tables — not learned parameters — so it requires no training data. `best_above_threshold()` returns the hypothesis with the highest posterior that exceeds a confidence threshold (default 0.95).

The key insight: a flash crowd and a slow ramp attack can produce identical event rate curves. The Bayesian tracker distinguishes them by looking at entropy changes (`delta_h`) and CUSUM state — flash crowds have high entropy (many different IPs), slow ramps have low entropy (few IPs sending more events).

### Peak Reservoir

A space-efficient reservoir sampler that maintains the top-N event rates seen in the current window. Used to compute percentiles for the dashboard without storing all data points.

### Output

When a hypothesis exceeds the confidence threshold, `Forecaster` sends an `EnforceCommand` through the same channel used by detection — enforcement applies the same block/unblock logic regardless of which subsystem triggered it.

## Uniqueness

**Three-model consensus.** Holt-Winters catches seasonal anomalies, CUSUM catches slow drift, and Bayesian classification catches the *type* of anomaly. No single model is trusted — the system requires agreement across multiple statistical methods before issuing a block decision.

**Flash crowd vs. attack distinction.** This is the key differentiator from rate-based detection. A product launch and a DDoS can look identical in event volume. The entropy-aware Bayesian tracker uses the *distribution* of source IPs — not just the count — to tell them apart.

**Private internal models.** `EwmAVar` and `CusumState` are not part of the public API. They're implementation details that could be replaced without breaking any consumer.

## Dependencies

**Reads from:** `ramshield-storage` (`Store::get_batch_history()`), `ramshield-config` (`ForecastingConfig`).

**Written to:** `ramshield-enforcement` (sends `EnforceCommand` via channel), `ramshield-metrics` (forecast values, z-scores, hypothesis posteriors).

**Internal dependencies:** `ramshield-types` (`EnforceCommand`, `BlockReason::RateLimit`).

## Benchmarks

No standalone benchmarks — the forecaster runs every 10 seconds and consumes <1ms of CPU. The Holt-Winters update is O(period) where period defaults to 120 — trivial at that scale.

## Testing

31 tests covering: Holt-Winters forecast accuracy on synthetic seasonal data, CUSUM warmup period (6 samples), CUSUM drift detection below individual thresholds, Bayesian hypothesis classification (flash crowd vs. slow ramp with identical event curves), and edge cases (zero events, single-sample windows).
