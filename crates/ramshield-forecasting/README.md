# ramshield-forecasting

Anomaly detection engine using Holt-Winters time-series forecasting combined with a Bayesian Hypothesis Framework. This module answers the question: "Is the current traffic pattern normal, or is something wrong?" — and if wrong, what kind of attack is it.

## Architecture

```
tick_hw (1 Hz)                           tick_entropy (0.2 Hz)
    │                                          │
    ▼                                          ▼
HoltWinters::update(rps)                 Shannon entropy delta
    │                                     (diversity of IPs)
    ▼                                          │
EwmAVar → z-score = (rps - forecast) / σ        │
    │                                          │
    ▼                                          │
CusumState (drift detector)                     │
    │                                          │
    ▼                                          ▼
              ┌──────────────────────────────────┐
              │   HypothesisTracker               │
              │   signals: z, cusum, threat, ΔH   │
              │   → Bayes' rule on 4 hypotheses   │
              └────────────┬─────────────────────┘
                           │
              ┌────────────┼────────────────┐
              │            │                │
              ▼            ▼                ▼
           Normal     Volumetric       FlashCrowd
           (noop)     (block RPS)     (log only)
```

## HoltWinters (triple exponential smoothing)

```rust
pub struct HoltWinters {
    level: f64,
    trend: f64,
    seasonal: Vec<f64>,
    alpha: f64,  // level smoothing (default: 0.3)
    beta: f64,   // trend smoothing (default: 0.1)
    gamma: f64,  // seasonal smoothing (default: 0.1)
    period: usize, // seasonality period (default: 60 ticks = 1 min)
}
impl HoltWinters {
    pub fn new(alpha: f64, beta: f64, gamma: f64, period: usize) -> Self
    pub fn update(&mut self, y: f64)  // one-step-ahead forecast for NEXT tick
}
```

Maintains three components: `level` (baseline), `trend` (direction), and `seasonal[period]` (cyclical pattern). The `update()` method takes the current observation and returns the one-step-ahead forecast via the `level` field after update.

**Key design decision:** The forecast uses the future seasonal slot (not the just-updated one) to avoid residual collapse on regular traffic cycles. Without this, a perfectly periodic traffic pattern would produce zero residuals and mask real anomalies.

**Benchmark:** 37-57 ns/op. Called once per second (1 Hz tick).

## EwmAVar (EWMA variance tracker)

```rust
pub struct EwmAVar { ... }
impl EwmAVar {
    pub fn new(span: usize) -> Self  // span=120 ticks ≈ 2 min
    pub fn update(&mut self, x: f64) -> f64  // returns current mean
    pub fn variance(&self) -> f64
    pub fn std_dev(&self) -> f64
}
```

O(1) memory: 3 floats (mean, variance, tick_count) = 24 bytes. Replaces the old `RingBuffer<60>` (480 bytes, O(n) standard deviation). Alpha=0.02 gives a 120-tick adaptation window — fast enough to track legitimate traffic shifts, slow enough to filter noise.

The z-score is computed as `(rps - forecast) / std_dev`. A z-score above 3.0 (configurable `anomaly_zscore`) triggers the CUSUM detector.

## CusumState (CUSUM drift detector)

```rust
pub struct CusumState { ... }
impl CusumState {
    pub fn new() -> Self
    pub fn update(&mut self, z: f64) -> (f64, bool)  // returns (new_s, fired)
}
```

O(1) memory: 4 floats = 32 bytes. Detects slow-ramp attacks invisible to z-score — attacks that stay below the anomaly threshold but sustain a drift over minutes.

- `drift allowance k = 0.5σ` — minimum deviation to accumulate.
- `decision boundary h = 4.0σ` — consecutive drift samples to trigger.
- When fired: sends enforcement command, resets both accumulators.

**Benchmark:** 3.1 ns/op. Called once per second.

## HypothesisTracker (Bayesian brain)

The core decision engine. Maintains posterior probabilities over four competing hypotheses about the current traffic state.

```rust
pub struct HypothesisTracker {
    posteriors: [f64; 4],  // H0..H3
}
impl HypothesisTracker {
    pub fn new() -> Self
    pub fn bayesian_update(&mut self, z: f64, cusum: f64, threat: f64, delta_entropy: f64)
    pub fn best_above_threshold(&self) -> Option<(usize, f64)>
}
```

### Hypotheses

| ID | Name | Prior | Meaning |
|----|------|-------|---------|
| H0 | Normal | 0.90 | Legitimate traffic, no attack |
| H1 | VolumetricDDoS | 0.02 | High-rate flood attack |
| H2 | SlowRampDoS | 0.02 | Gradual ramp-up below threshold |
| H3 | FlashCrowd | 0.05 | Legitimate traffic spike (release, news) |

### Signal inputs (per tick)

1. **z-score** — EWMA residual deviation. High z → H1 likely.
2. **CUSUM alarm** — sustained drift. High cusum → H2 likely.
3. **max threat** — per-IP threat score from detection crate. High threat → H1 likely.
4. **entropy delta** — change in IP diversity. Low diversity → H1 (botnet). High diversity → H3 (flash crowd).

### Update cycle

1. Compute log-likelihood ratios for each hypothesis × each signal.
2. Apply Bayes' rule: `P(H|evidence) ∝ P(evidence|H) × P(H)`.
3. Softmax normalize posteriors.
4. Decay toward baseline (multiply by 0.98, renormalize) — prevents stale decisions.
5. Cold start: first 30 ticks use higher threshold (0.85 vs 0.75) to prevent premature action.

### Decision dispatch

| Highest posterior above threshold | Action |
|-----------------------------------|--------|
| H0 (Normal) | No-op |
| H1 (VolumetricDDoS) | Block by RPS spike, send `EnforceCommand::Block` |
| H2 (SlowRampDoS) | Block by sustained deviation, send `EnforceCommand::Block` |
| H3 (FlashCrowd) | Log only — legitimate traffic, NOT blocked |

**Benchmark:** bayesian_update = 157 ns/op; best_above_threshold = 5.5 ns/op. Called once per second.

## PeakReservoir (legacy, transitional)

```rust
pub struct PeakReservoir { ... }  // 512-element cap, modular eviction
```

Empirical quantile of forecast residuals. Used as a spot alarm fallback until v0.4 removal. Kept for backward compatibility with dashboards that display the quantile metric.

## Dependencies

`ramshield-types` (for `EnforceCommand`). Pure math — no async, no I/O, no external crates beyond `serde`.

## Tests (18)

- HW: forecast correctness, seasonal slot usage, level/trend convergence.
- EwmAVar: phase adaptation, zero stddev edge case, spike residual z-score.
- CUSUM: drift detection, noise rejection, reset after alarm, cap behavior.
- Bayesian: normal traffic stays H0, volumetric detection fires H1, slow ramp fires H2, flash crowd stays H3, cold start delay, posterior decay over time.
- PeakReservoir: cold start, warm quantile, negative deviation handling.
