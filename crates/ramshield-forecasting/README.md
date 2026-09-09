# ramshield-forecasting

## Problem

The detection module catches obvious attacks: high rate, many IPs, status code anomalies. But sophisticated attacks stay below every individual threshold. A slow ramp that adds 10 requests/second per minute will never trigger a static rate limit — until it's too late. And a flash crowd from a product launch looks identical to a volumetric DDoS if you only look at rate.

The forecasting module answers: "Is the overall traffic pattern consistent with normal behavior, or does it match a known attack profile?" It uses time-series forecasting to predict what *should* happen next, then measures the deviation.

## How it works

### Pipeline (runs once per second)

```
TrafficCounters (from Store — no store scan)
        │
        ▼
HoltWinters::update(rps)          ← triple exponential smoothing
        │
        ▼
EwmAVar → z-score                 ← normalized residual
        │
        ▼
CusumState → alarm flag           ← slow-ramp drift detector
        │
        ▼
HypothesisTracker::bayesian_update(z, delta_h, threat, cusum_alarm)
        │
        ▼
Decision: Block or No-op
```

### HoltWinters (triple exponential smoothing)

```rust
pub struct HoltWinters {
    level: f64,        // baseline
    trend: f64,        // direction
    seasonal: Vec<f64>, // cyclical pattern
}
impl HoltWinters {
    pub fn new(alpha: f64, beta: f64, gamma: f64, period: usize) -> Self
    pub fn update(&mut self, y: f64)  // returns forecast for NEXT tick
}
```

Maintains level + trend + seasonal components. The forecast uses the *future* seasonal slot to avoid residual collapse on regular traffic cycles. Params: α=0.3, β=0.1, γ=0.1, period=60 (1-minute seasonality). Benchmark: 37-57 ns/op.

### EwmAVar (internal, private)

O(1) EWMA variance tracker (3 floats = 24 bytes). Alpha=0.02, span=120 ticks (≈2 min). Normalizes forecast residuals into z-scores. Replaces the old RingBuffer<60> (480 bytes, O(n) standard deviation).

### CusumState (internal, private)

Two-sided CUSUM (Page 1954). Detects slow-ramp attacks invisible to z-score:
- Upper: `s_upper = max(0, s_upper + z - k)` — accumulates positive deviation
- Lower: `s_lower = max(0, s_lower - z - k)` — accumulates negative deviation
- Drift allowance k=0.5σ, decision boundary h=4.0σ
- When fired: sends enforcement command, resets both accumulators. 3 ns/op.

### HypothesisTracker (Bayesian brain)

```rust
pub struct HypothesisTracker { posteriors: [f64; 4] }
impl HypothesisTracker {
    pub fn new() -> Self
    pub fn bayesian_update(&mut self, z: f64, delta_h: f64, threat: f64, cusum_alarm: bool) -> [f64; 4]
    pub fn best_above_threshold(&self) -> Option<(Hypothesis, f64)>
}
```

Four competing hypotheses about traffic state:

| ID | Name | Prior | Signal |
|----|------|-------|--------|
| H0 | Normal | 0.91 | Low z, stable entropy, low threat |
| H1 | VolumetricDDoS | 0.02 | High z, entropy ↓, high threat |
| H2 | SlowRampDoS | 0.02 | CUSUM alarm primary signal |
| H3 | FlashCrowd | 0.05 | High entropy, low threat |

Bayes' rule updates posteriors from 4 signal inputs. 98% decay toward baseline each tick prevents stale decisions. Cold start: first 30 ticks use higher threshold (0.85 vs 0.75) to prevent premature action. 157 ns/op.

### Decision dispatch

| Highest posterior | Action |
|-------------------|--------|
| H0 (Normal) | No-op |
| H1 (VolumetricDDoS) | Block by RPS spike |
| H2 (SlowRampDoS) | Block by sustained deviation |
| H3 (FlashCrowd) | **Log only** — legitimate, never blocked |

The flash-crowd distinction is critical. Blocking a legitimate traffic spike (e.g., product launch) would be worse than the attack itself.

## Dependencies

```
ramshield-forecasting
  ← ramshield-types    (EnforceCommand, EnforceAction)
  ← ramshield-storage  (Store — reads TrafficCounters, no store scan)
  ← ramshield-metrics  (Metrics — set_forecast_hw, set_entropy)
  ← ramshield-config   (ForecastingConfig — alpha, beta, gamma, thresholds)
  ← tokio, uuid, tracing

Sends to: ramshield-enforcement (via EnforceCommand channel)
```

This module reads from the Store's `TrafficCounters` (lock-free atomics) — it never scans the DashMap. This keeps the 1 Hz tick fast and non-blocking.

## Key benchmarks

| Function | ns/op | ops/sec |
|----------|-------|---------|
| HoltWinters::update | 37-57 | 17-27M |
| bayesian_update | 157 | 6.3M |
| best_above_threshold | 5.5 | 175M |

At 1 Hz tick rate, total CPU per tick: ~200 ns. Negligible.

## What to read next

- `crates/ramshield-detection/` — produces threat scores this module reads
- `crates/ramshield-enforcement/` — receives block commands from this module
- `crates/ramshield-storage/` — TrafficCounters (atomic counters, no DashMap scan)
