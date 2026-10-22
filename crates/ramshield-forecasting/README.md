# ramshield-forecasting

```text
store.traffic ──→ Forecaster::run()  (1 Hz)
  events_last_second        │
  unique_ips_window         │
  drain_threat_sample()     │
  subnet_window (256 /24)   │
                            ↓
        Holt-Winters → residual → EWMA z-score → CUSUM
                            ↓
                 entropy delta (ΔH), threat
                            ↓
                    Bayesian hypothesis
                            ↓
              H1 / H2 ──→ EnforceCommand
              H3       ──→ no block
```

**The daemon runs this crate.** `Forecaster` is exported from
`ramshield`'s lib, and `boot_pipeline()` constructs one when
`forecasting.enabled` is set, so the running daemon produces
`EnforceCommand`s from a forecast. The diagram above is the crate's own
loop, which the daemon's task runs.

## Why it exists

The detection crate catches obvious attacks — high rate, many IPs, status
code anomalies — and its own CUSUM already catches **per-IP** drift that
stays below the rate threshold. What detection has no equivalent of is
**aggregate-rate** drift across the whole stream and cross-window trend
modelling: a slow ramp that no single IP triggers.

## How it works

### The tick cycle

`Forecaster::run()` ticks every second
(`tokio::time::interval(Duration::from_secs(1))` in `forecaster.rs`); each
tick calls `tick_hw`, which reads the counters the storage layer already
keeps — `store.traffic.events_last_second`,
`store.traffic.unique_ips_window`, `store.traffic.drain_threat_sample()`
and the 256-bucket `store.traffic.subnet_window` — and feeds them through
**chained** models:

```
Holt-Winters → residual → EWMA z-score → CUSUM → Bayesian hypothesis
```

The forecaster has no public `tick()` method and reads no batch history:
the loop is `run()`, and the data is the live traffic counters. A tick
makes no decision at all when fewer than 10 unique IPs are in the window.

### Holt-Winters triple exponential smoothing

A seasonal trend model with three smoothing parameters:

- `alpha` (level smoothing): how quickly the baseline adjusts to new data.
- `beta` (trend smoothing): how quickly the trend direction changes.
- `gamma` (seasonal smoothing): how strongly the seasonal cycle influences
  the forecast.
- `period`: the seasonal cycle length in ticks.

`HoltWinters::update(y)` returns the one-step-ahead forecast. The
forecast reads the *future* seasonal slot rather than the one it just
updated, so residuals do not collapse to near-zero on a regular cycle.

```rust
pub struct HoltWinters {
    pub level: f64,
    pub trend: f64,
    pub seasonal: Vec<f64>,
    pub period: usize,
    // private: alpha, beta, gamma, tick
}
```

The period comes from `forecasting.seasonality_period`, whose default is
3,600 ticks — 60 minutes at 1 Hz (the config validator caps it at
86,400).

### EWMA variance tracker

`EwmAVar` tracks the mean and variance of the **forecast residual**
(`rps - forecast`), not of the event rate. It is O(1) memory — four
fields, 32 bytes: `ewma`, `var_ewma`, `count` and `alpha` — and
`update()` returns the residual z-score directly. Its σ is not exposed;
the `sigma()` accessor is test-only.

`EwmAVar` is a public type, so it can be named from outside the crate,
but its fields and constructors are crate-private, so it cannot be built
or used there.

### CUSUM (cumulative sum) detector

A change-point detector, separate from the detection crate's CUSUM. This
one operates on the **Holt-Winters residual z-score** (one tick = one
second), not on a per-IP rate:

```rust
pub struct CusumState {
    // private: s_upper, s_lower, k (slack), h (decision boundary)
}
```

`k` is the slack and `h` the decision boundary, both in sigma units. The
forecaster builds it with `CusumState::new(0.5, 4.0)`. The detection
crate's CUSUM catches individual IPs drifting upward; this one catches
the aggregate rate drifting.

`CusumState` is a public type with crate-private fields and constructors.

### Bayesian hypothesis tracker

`HypothesisTracker` classifies the current traffic state into one of four
hypotheses:

```rust
pub enum Hypothesis {
    Normal,        // H0: baseline traffic
    VolumetricDoS, // H1: fast flood
    SlowRampDoS,   // H2: coordinated slow ramp attack
    FlashCrowd,    // H3: legitimate spike (product launch, viral link)
}
```

H1 (volumetric) and H2 (slow ramp) trigger a preemptive block; H3
(flash crowd) does not, because a flash crowd is legitimate traffic.

`bayesian_update(z, delta_h, threat, cusum_alarm)` computes posterior
probabilities for all four hypotheses with Bayes' rule. The model uses
hand-written log-likelihood functions with fixed weights — not learned
parameters and not lookup tables — so it needs no training data. The
posteriors decay back toward the baseline priors (0.98 decay) every tick.

`best_above_threshold()` returns the hypothesis with the highest
posterior above the threshold: 0.75 once warm, and 0.85 during the first
60 cold-start ticks. It returns `None` when the best hypothesis is
`Normal`.

A flash crowd and a slow ramp can look alike in the event rate. The
tracker separates them by the entropy delta and the CUSUM state: CUSUM is
the primary signal for H2, entropy for H3. The entropy is Shannon over
the 256 **/24 subnet** buckets, not over source IPs — so a flash crowd
spreads its events over many subnets (high subnet entropy) while a slow
ramp concentrates them (low subnet entropy).

The posteriors themselves are not exported to metrics; they live inside
the tracker.

### Peak reservoir

A fixed-capacity reservoir of absolute forecast residuals — 512 entries,
evicted round-robin. It is not a top-N sampler. It feeds the legacy
spot alarm below as a self-calibrated extreme-residual threshold
(`extreme_quantile(0.001)`); the source marks it legacy and transitional,
and nothing in the dashboard reads it.

### Output

When a hypothesis clears the threshold, `Forecaster` sends an
`EnforceCommand` over the `mpsc::Sender<EnforceCommand>` it was given.
The crate has no dependency on `ramshield-enforcement`; whatever actor
owns the receiver applies the command, and enforcement applies the same
block/unblock logic regardless of which subsystem triggered it.

A legacy spot alarm still fires, but only when no hypothesis fired and the
residual z-score is above `anomaly_zscore`. The threshold itself is the
reservoir's extreme quantile once the reservoir has warmed, and the
z-score threshold before that. It is marked for removal in v0.4.

## Uniqueness

**One posterior decides.** Holt-Winters catches seasonal anomalies, CUSUM
catches slow drift, and the Bayesian classification catches the *type* of
anomaly. But a single model decides: the Bayesian posterior alone gates
the block, with CUSUM and entropy as its inputs. The models are not
independent voters and there is no cross-model agreement requirement.

**Flash crowd vs. attack distinction.** A product launch and a DDoS can
look identical in event volume. The entropy-aware tracker uses the
distribution of events over the **/24 subnets** in the last flush window,
not the count alone.

**Public types, private internals.** `EwmAVar` and `CusumState` are
public types whose fields and constructors are crate-private.

## Dependencies

**Depends on:** `ramshield-types` (`EnforceCommand`, `EnforceAction`),
`ramshield-storage` (`Store::traffic`), `ramshield-metrics`,
`ramshield-config` (`ForecastingConfig`), `tokio`, `tracing`, `uuid`.
The crate emits the wire string reason `"forecast_anomaly"`, not a
`BlockReason` value.

**Written to:** `ramshield-metrics` (`set_forecast_hw`, `set_entropy`) and
the injected `mpsc::Sender<EnforceCommand>`.

**Read by:** the root `ramshield` package, which re-exports `Forecaster`
and constructs one in `boot_pipeline()` when `forecasting.enabled` is set.

## Configuration

`ForecastingConfig::default()`: `enabled` true, `ewma_alpha` 0.3,
`hw_beta` 0.1, `hw_gamma` 0.1, `seasonality_period` 3,600,
`anomaly_zscore` 2.5, `min_entropy` 2.0. The EWMA span (120 ticks) and
the CUSUM slack and boundary (0.5 and 4.0) are hard-coded in the
forecaster, not configurable. `min_entropy` is the entropy floor: below
it the traffic is concentrated on a few subnets, so an entropy drop is
an anomaly (a narrowing) rather than a flash crowd (a legitimate
spread-out surge). The forecaster reports the two cases differently.

## Benchmarks

There is no benchmark for this crate. The workspace `hot_paths` bench
exercises it, and the Holt-Winters update is O(1) — one seasonal slot per
tick — with the period defaulting to 3,600 ticks.

## Testing

19 unit tests in `src/tests/mod.rs`, plus the README-claim guards in
`tests/readme_claims.rs`. The unit tests cover the Holt-Winters forecast
on synthetic seasonal data, the CUSUM drift detection, the Bayesian
classification, and the edge cases.
