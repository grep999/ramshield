# ramshield-forecasting

Anomaly detection engine using Holt-Winters time-series forecasting combined with a Bayesian Hypothesis Framework.

## Architecture

```
tick_hw (1 Hz)                          tick_entropy (0.2 Hz)
    │                                         │
    ▼                                         ▼
HoltWinters::update(rps)                Shannon entropy delta
    │                                    (diversity of IPs)
    ▼                                         │
EWMA variance → z-score = (rps - forecast) / σ    │
    │                                         │
    ▼                                         │
CUSUM drift detector                            │
    │                                         │
    ▼                                         ▼
              ┌─────────────────────────────────┐
              │  HypothesisTracker::update()     │
              │  signals: z, cusum, threat, ΔH   │
              │  → Bayes' rule on 4 hypotheses   │
              └────────────┬────────────────────┘
                           │
              ┌────────────┼────────────────┐
              │            │                │
              ▼            ▼                ▼
           Normal     Volumetric       FlashCrowd
           (noop)     (block RPS)     (log only)
```

## Key Components

### HoltWinters (triple exponential smoothing)
- Maintains `level`, `trend`, and `seasonal[period]` components
- `update(y)` returns one-step-ahead forecast for NEXT tick
- Uses future seasonal slot (not just-updated) to avoid residual collapse on regular cycles
- Params: α=0.3, β=0.03, γ=0.03, period=60 (1-minute seasonality)

### EwmAVar (EWMA variance tracker)
- O(1) memory: 3 floats (mean, variance, tick_count) = 24 bytes
- Alpha=0.02 (span=120 ticks ≈ 2 min adaptation window)
- Replaces old RingBuffer<60> (480 bytes, O(n) std dev)
- Feeds σ to z-score calculation

### CusumState (CUSUM drift detector)
- O(1) memory: 4 floats = 32 bytes
- Detects slow-ramp attacks invisible to z-score (below anomaly threshold but sustained)
- Drift allowance k=0.5σ, decision boundary h=4.0σ
- Fires → blocks → resets both accumulators

### PeakReservoir (legacy, transitional)
- Empirical quantile of forecast residuals
- Reservoir sampling lite: 512-element cap, modular eviction
- Used as spot alarm fallback until v0.4 removal

### HypothesisTracker (Bayesian brain)
- 4 hypotheses: H0=Normal (prior 0.90), H1=VolumetricDDoS (0.02), H2=SlowRampDoS (0.02), H3=FlashCrowd (0.05)
- 4 signal inputs per tick:
  - z-score (EWMA residual deviation)
  - CUSUM alarm (drift detection)
  - max threat (per-IP threat score from detection crate)
  - entropy delta (IP diversity change)
- Likelihood functions: log-likelihood ratios for each hypothesis × each signal
- Bayes' rule update → softmax normalization → decay toward baseline (0.98)
- Cold start: first 30 ticks use higher threshold (0.85 vs 0.75) to prevent premature action
- Decision: match highest posterior above threshold → dispatch enforcement

### BayesianDecision dispatch
| Hypothesis       | Action                                          |
|-----------------|-------------------------------------------------|
| Normal           | No-op (PeakReservoir legacy fallback)           |
| VolumetricDDoS   | Block by RPS spike, send enforcement command     |
| SlowRampDoS      | Block by sustained deviation, send enforcement   |
| FlashCrowd       | Log only — legitimate traffic, NOT blocked       |

## 18 tests
- HW forecast correctness, seasonal slot usage
- EWMA variance: phase adaptation, zero stddev, spike residual z-score
- CUSUM: drift detection, noise rejection, reset after alarm
- Bayesian: normal traffic, volumetric detection, slow ramp, flash crowd, cold start, posterior decay
- PeakReservoir: cold start, warm quantile, negative deviation handling
