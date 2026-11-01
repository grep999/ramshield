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

Use fixed UTC event-time windows. The initial collection resolution may be one minute, with hourly calendar summaries derived from completed windows. A window must have a stable identity `(service_id, window_start, window_width, metric)` so replay is idempotent.

Represent at least these states distinctly. The current bounded `WindowLedger` prototype implements the three states and duplicate/conflict detection in memory; durable persistence and runtime integration remain caller responsibilities:

- `Observed(value)`: collector completed the window; zero is a valid value.
- `Missing(reason)`: no valid observation exists.
- `Degraded(value, quality)`: observation exists but capture/ingest loss or collector health makes it unsuitable for an ordinary baseline.

Never convert missing or degraded windows to zero. Record capture drops, ingest drops, queue saturation and parser rejects as quality covariates. Baseline training must be able to exclude or separately model degraded windows.

All event timestamps are stored in UTC. Workload declarations additionally carry an IANA timezone and local recurrence. Resolve local schedule boundaries to UTC at evaluation time, including daylight-saving gaps and repeated local times. Do not use a fixed UTC offset as a timezone.

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

Use Welford-style online moments to avoid subtracting large nearly equal raw moments. Persist finite, validated numeric state. Reject corrupted, non-finite or inconsistent state on load.

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

Declarations include stable ID, description, owner, declared-at timestamp, IANA timezone, recurrence, local start, duration, affected services and expected behavioural changes. Keep provenance and revision history.

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
- Service-specific series and workload provenance.
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