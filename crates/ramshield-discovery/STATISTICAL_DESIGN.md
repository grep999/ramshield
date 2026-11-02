# RamShield Learning: Statistical Design Contract

Status: design specification for passive Stage 0. This document is not a claim that the current implementation is a validated anomaly detector.

## 1. Objective and decision boundary

RamShield Learning estimates expected production behaviour conditional on time, service and operator-declared workload context. It reports deviations and uncertainty. It does not decide whether traffic is malicious, whitelist declared workloads, change detection thresholds, or trigger enforcement.

Keep three outputs separate:

- Environment model: services, schedules and observed traffic profiles.
- Statistical evidence: baseline, sample size, dispersion, missingness and deviation.
- Operational context: declarations, ownership, schedule and provenance.

A declared workload is explanatory context, never an exemption. Observed deviations remain visible during declared workloads.

## 2. Observation contract

Use fixed UTC event-time windows. The initial collection resolution may be one minute, with hourly calendar summaries derived from completed windows. The current `WindowKey` identity is `(service_id, direction, window_start, window_width)`; each ledger entry carries packet/byte totals and quality state, not a separate metric key.

Represent at least these states distinctly. The bounded `WindowLedger` implements duplicate/conflict detection in memory and explicit snapshot persistence through `flush`/`reopen`. Capacity eviction advances a global event-time floor: it removes the entire oldest timestamp cohort and rejects all later keys at or below that timestamp, regardless of service or direction. This prevents equal-timestamp replays without unbounded tombstones, but conservatively rejects previously unseen service windows at or below the floor. Snapshot format version 3 stores this floor; older formats fail closed rather than being migrated. Reopen checks file metadata and also reads at most `limit + 1` bytes before JSON parsing, so concurrent growth after the metadata check cannot bypass the configured bound. The ledger is not a per-record WAL, so records since the last successful flush are not crash-durable. The ledger is not yet wired into `Discovery`'s D7/D8 profiles and does not contain protocol totals or endpoint keys needed to reconstruct every `TrafficSample` dimension. Runtime feeding and flush scheduling remain caller responsibilities. The D7/D8 per-service metric/cardinality profiles in `Discovery` are persisted in the same atomic snapshot as aggregate buckets. They are bounded to the current and immediately preceding absolute UTC hour because the current assessment compares only adjacent hours; older late-arriving events remain in aggregate history but do not update service profiles. The profile map is capped at 64 services. Schema-3 snapshots migrate to schema 4 with an empty profile map because schema 3 never persisted per-service profiles. Event-level `observe_with_quality` is not idempotent: retrying a `TrafficSample` counts it again. No completed-service-window API currently carries the full metric/cardinality payload needed to combine the generic `WindowLedger` with D7/D8 training. `TrafficCalendar` snapshots are capped at 64 MiB before JSON parsing and use an exclusive same-directory temporary file, file fsync, atomic rename and parent-directory fsync; concurrent writers to one path must still be serialized because replacement is last-writer-wins:

- `Observed(value)`: collector completed the window; zero is a valid value.
- `Missing(reason)`: no valid observation exists.
- `Degraded(value, quality)`: observation exists but capture/ingest loss or collector health makes it unsuitable for an ordinary baseline.

Never convert missing or degraded windows to zero. Record capture drops, ingest drops, queue saturation and parser rejects as quality covariates. Baseline training must be able to exclude or separately model degraded windows.

`MetricProfileStore::observe_protocol_packets` treats its five-element vector as complete: each zero is an observed zero and contributes a sample. Callers must not synthesize zero for an unavailable protocol counter; they must preserve the missing/degraded quality distinction.

All event timestamps are stored in UTC. The current `TrafficCalendar` recurrence implementation is UTC-only: workload declarations do not yet carry an IANA timezone or local recurrence. IANA timezone support, including daylight-saving gaps and repeated local times, remains a future requirement; do not represent a fixed UTC offset as a timezone.

## 3. Conditional calendar

Calendar keys are features, not independent models by themselves. Candidate features:

- hour-of-week;
- hour-of-month and day-of-month;
- month-end / month-start indicators;
- service identity;
- declared workload identity and active phase;
- deployment or maintenance annotations.

Use hierarchical fallback only when the selected level has adequate clean history. A sparse service-specific profile may borrow strength from its service class or global profile, but the report must identify the source and uncertainty. Do not present a broad fallback as service-specific evidence.

Monthly recurrence must be calendar-aware: month-end means the final civil day in the declared timezone, not a fixed day number.

## 4. Count model

Packet, byte, flow and connection counts are non-negative counts. A Gaussian mean +/- k standard-deviation band is not automatically a valid predictive interval for them.

Candidate marginal families:

- Poisson: `Var(X) = mu`, when dispersion diagnostics and residual checks support it.
- Negative binomial: `Var(X) = mu + alpha * mu^2`, when overdispersion is present.
- Underdispersed or zero-inflated observations: do not force them into a negative-binomial fit; use a separately validated model or report that the candidate family is unsupported.

The current `fit_count_model` is a method-of-moments diagnostic only. It is not a goodness-of-fit test, a predictive distribution, a p-value or an alert threshold. A future fit must test goodness-of-fit and residual dependence on chronological validation data.

Use Welford-style online moments to avoid subtracting large nearly equal raw moments. Persist finite, validated numeric state. Reject corrupted, non-finite or inconsistent state on load. A valid CountMoments state has a mean in the representable u64 count domain, non-negative finite M2, zero M2 for zero or one observation (and for a zero or maximum mean), and M2 no greater than n * mean * (u64::MAX - mean) within a small floating-point tolerance. Quality/sample counts are part of the same evidence invariant and must not be reset independently of the moments. Aggregate snapshot validation also checks cross-counter conservation: the saturating sum of protocol packet counters must equal the bucket packet total; hashed source and destination bucket totals must not exceed it; and retained destination-port counters must not exceed the number of samples. Individual counters being in range is not sufficient when their combined totals are impossible.

## 5. Dependence and predictive assessment

Traffic windows are temporally dependent and often bursty. A model that treats every hour as an independent sample can understate uncertainty and overstate evidence.

Before anomaly scoring:

1. Diagnose autocorrelation and burst/self-excitation in residuals.
2. Establish whether conditional Poisson/NB assumptions are adequate.
3. Construct predictive intervals or tail probabilities from the fitted model, not a normal approximation by default.
4. Calibrate scores on a later chronological calibration period.
5. Evaluate on a still-later untouched period.

Conformal calibration may be considered after dependence is addressed. Standard exchangeability guarantees must not be claimed for dependent, drifting time series without an appropriate method and stated assumptions.

## 6. Drift protection

An isolated anomaly and a persistent distribution change are different outputs. Drift monitoring must be separate from point anomaly scoring.

Do not automatically update the trusted baseline with an anomalous or degraded window. Use a quarantined candidate baseline, require sustained evidence and preserve the pre-change model. Promotion is an operator-reviewed action during Stage 0.

## 7. Workload context

Declarations include stable ID, description, owner, declared-at timestamp, timezone, recurrence, local start, duration, affected services and expected behavioural changes. Keep provenance and revision history. The current implementation records UTC recurrence only; it does not yet implement the timezone field. Workload execution records validate identifiers, metadata bounds, nonzero durations and representable expected end times on admission and reopen. Overrun transitions require the expected end to have passed; completion may follow either Active or Overrun and records total elapsed whole hours.

During a declaration, report both the statistical deviation and the declaration context. Repeated observations may eventually establish a recurring baseline, but the declaration itself must not alter the observed sample or suppress a finding.

## 8. Validation protocol

Use chronological splits, never random shuffling of time-series windows:

- training period: fit baseline parameters;
- calibration period: calibrate thresholds / predictive scores;
- test period: untouched evaluation.

Test stable traffic, daily and weekly cycles, month-end batches, legitimate service onboarding, bursty workloads, gradual ramps, volumetric floods, low-rate changes, new ports, source concentration, missing windows, collector loss, duplicated/replayed windows, clock skew and daylight-saving transitions.

Report precision, recall, false-positive rate, false alarms per service-day, detection delay, calibration error, drift delay, CPU, memory and persistence cost. Report results by service and traffic regime, not only as one aggregate score.

Use public network time-series data such as CESNET-TimeSeries24 as an external benchmark, but document its collection scope, filtering, missingness and label limitations. Add deterministic synthetic traces with known ground truth.

## 9. Stage 0 acceptance gates

- No enforcement or live configuration mutation.
- No training on missing or collector-degraded windows as if they were zero.
- Idempotent window ingestion and explicit duplicate handling.
- Service-specific series and workload provenance; D7/D8 service profiles persist atomically with aggregate state and retain the two adjacent absolute UTC hours used by the current assessment path.
- A lossless native/metrics adapter contract for packet counts, service identity and direction before service-specific capture integration.
- Timezone-correct recurrence tests.
- Count-family diagnostics validated against synthetic Poisson, overdispersed and underdispersed data.
- Predictive intervals and calibration tested on chronological holdouts.
- Drift cannot silently absorb an attack into the trusted baseline.
- Runtime feed is bounded and nonblocking, with dropped observations visible.
- CI evidence exists for the exact candidate commit.

## 10. Research basis

- Lambert et al., *Nonparametric self-exciting models for computer network traffic*, Statistics and Computing (2019): seasonal intensity and self-exciting bursts. https://link.springer.com/article/10.1007/s11222-019-09875-z
- *Adaptive Thresholds: Monitoring Streams of Network Counts Online* (2006): negative-binomial reference distributions, trends, cycles, outliers and missing data. https://www.nokia.com/bell-labs/publications-and-media/publications/adaptive-thresholds-monitoring-streams-of-network-counts-online/
- CESNET-TimeSeries24 (2025): long-duration network traffic time series, multiple aggregation levels and real missing-data examples. https://pmc.ncbi.nlm.nih.gov/articles/PMC11865510/
- Salmon, Schumacher and Höhle, *Monitoring Count Time Series in R* (2016): count regression, overdispersion, predictive aberration detection and CUSUM. https://www.jstatsoft.org/article/view/v070i10

These sources motivate candidate methods; they do not validate RamShield's implementation. Validation must be performed on RamShield's own data and evaluation harness.

## 11. Optional machine-learning and adaptive-collection extension

This section defines a future-compatible extension point. It does **not** claim that ML is implemented, enabled, trained or required by the current discovery crate. The deterministic collector and statistical assessments remain the source of truth.

### 11.1 Purpose

Use a small online model to improve the *allocation and compression* of telemetry, not to replace the observation contract. The model may recommend where more detail is valuable and where bounded summaries are sufficient:

- stable, well-understood services can use compact counters and sketches;
- novel, rapidly changing or uncertain services can temporarily receive higher-resolution windows or richer bounded feature summaries;
- repeated, similar service profiles may be grouped into candidate cohorts for shared baseline estimation, while retaining service-specific identity and reporting;
- redundant dimensions may be deprioritised only when the model records the resulting information loss and the core counters remain intact.

The intended gain is lower average CPU, memory and persistence cost for a given level of statistical coverage, with more detail collected around uncertainty and change. It is not permission to discard evidence of suspicious activity.

### 11.2 Proposed architecture

Keep the packet/event hot path deterministic and model-free. Add a periodic, bounded control-plane component (candidate name: `AdaptiveCollectionPolicy`) that consumes completed-window summaries and emits a collection plan for *future* windows.

The policy boundary should be a narrow trait or equivalent pure interface:

- input: bounded per-service/window features, collector-health state, current resource budget and model version;
- output: requested aggregation resolution, optional feature/sketch budgets and an explicit reason code;
- state: bounded, versioned model state, updated outside the capture hot path;
- fallback: deterministic fixed-budget policy whenever the model is disabled, unavailable, stale, invalid or over budget.

A plan must never rewrite completed observations. Every observation must carry the effective collection-plan version and quality metadata so later analysis can distinguish actual zero from unobserved or sampled data.

### 11.3 Candidate models, in increasing complexity

1. **Adaptive sketches first (no ML dependency):** retain fixed-cost packet/byte counters and bounded Space-Saving / Count-Min / HyperLogLog-style summaries for heavy hitters and cardinality. Size and update work are configured and hard-capped. This is likely to deliver the largest immediate efficiency gain with the least operational risk.
2. **Online novelty and drift signals:** use bounded robust baselines and change/novelty scores over completed-window feature vectors to request temporary higher detail. Keep drift detection separate from anomaly scoring; a drift signal must not automatically retrain or bless the new baseline.
3. **Optional lightweight online clustering:** evaluate bounded micro-clusters (for example, streaming k-means over normalised service-window features) to identify similar workload regimes and support hierarchical baseline sharing. Preserve service-level residuals and never let cohort membership hide a service-specific deviation.
4. **More complex models only after evidence:** consider streaming isolation/forecasting models only if benchmarks show a material improvement over the simpler methods under the same CPU, memory and false-alarm budget.

Do not add a heavyweight inference runtime or external model download to the default RamShield build. Any future implementation should be pure Rust or an optional feature with explicit dependency, binary-size and compile-time budgets.

### 11.4 Safety, poisoning and sampling rules

- ML is advisory. It cannot change firewall/XDP state, detection thresholds, enforcement policy or workload allowlists.
- Never reduce the fixed core telemetry needed for accounting, health, replay detection and minimum security visibility.
- Never treat sampled-out, missing or degraded measurements as zero. Mark sampling probability, omitted dimensions and quality on every affected window.
- Do not use adaptively sampled values as unbiased training data unless the estimator explicitly accounts for the sampling design; otherwise keep them out of trusted baseline fitting.
- A suspicious or novel window must be eligible for *more* detail, never less. Collection budgets and per-service fairness caps remain hard limits.
- Train/update only from eligible clean windows. Quarantine anomalous, degraded and low-confidence windows; protect model updates against poisoning and slow baseline absorption.
- Bound all model state, update frequency, inference time and output ranges. Validate persisted model versions and state before use.
- If model state fails validation or the model exceeds its budget, fall back to deterministic collection without blocking traffic.

### 11.5 Evaluation and rollout gates

Compare fixed collection against adaptive collection on identical chronological traces and equivalent resource budgets. Measure CPU per million observations, peak RSS, bytes persisted per service-day, sketch error, cardinality/heavy-hitter recall, detection/novelty recall, false alarms per service-day, detection delay, missingness and fairness across services.

Require synthetic ground-truth traces and representative production replay, including low-rate attacks, sudden floods, slow ramps, flash crowds, new-service onboarding, collector loss, adversarial feature churn and model-state corruption. Report performance by service and regime; aggregate averages must not conceal a degraded service.

Roll out in this order: shadow recommendations; compare proposed versus fixed collection plans; enable adaptive detail collection behind an explicit opt-in feature/configuration; retain a one-switch deterministic fallback. No automatic enforcement integration is in scope.

The extension is ready for implementation only when the fixed collector is stable, quality propagation and idempotent completed-window ingestion are sound, and benchmarks demonstrate a measurable resource/coverage improvement. Until then this is an architectural seam and evaluation contract, not an additional runtime dependency or Stage 0 merge requirement.
