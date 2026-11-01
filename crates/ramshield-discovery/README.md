# ramshield-learning

Passive production-environment discovery for RamShield. This crate is deliberately independent of the detection and enforcement crates.

## Responsibilities

- Aggregate bounded hourly traffic observations: packet/byte volume, protocol mix, approximate source/destination distributions and bounded destination-port heavy hitters.
- Learn recurring traffic shape by UTC hour-of-week, hour-of-month and a dedicated month-end profile.
- Maintain numerically stable online count moments and report Poisson / negative-binomial / underdispersed method-of-moments diagnostics.
- Provide a provisional descriptive hourly range; it is not a calibrated prediction interval or a production alert threshold.
- Return active operator-declared workload context alongside every assessment, without suppressing anomalies.
- Record operator-declared services and workloads before onboarding or scheduling them, including daily, weekly, monthly-day, month-end and N-day recurrence.
- Track capture/ingestion quality and host resource observations.
- Persist observations and calendar declarations atomically.

## Safety boundary

This crate has no dependency on ramshield-detection, ramshield-enforcement, ramshield-xdp or firewall APIs. It cannot block traffic, change network state, activate thresholds or reload live configuration. Workload declarations add explanatory context; they do not delete, hide or automatically whitelist events.

## Integration contract

The caller adapts existing capture/metrics events into TrafficSample and HealthSample. Address identifiers must be hashed by the caller before they are supplied; this crate does not store raw IP addresses. Call TrafficCalendar::observe_hour alongside traffic aggregation and use declare_workload to feed planned services/jobs into the calendar.

Persistence is explicit via flush; schedule it from a background task, never from the packet capture hot path. Store files should be owned by the RamShield service account and backed up with the deployment's normal state policy.

All calendar timestamps are Unix UTC seconds. Weekday numbering is Monday=0. The current calendar range uses mean +/- sigma as a descriptive heuristic only. The count-family diagnostic is a method-of-moments candidate, not a goodness-of-fit test or calibrated tail probability. It does not account for temporal autocorrelation and must not be used as a production alert threshold. Proper predictive intervals, calibration on chronological hold-out data, missing-window accounting, service-specific series and drift monitoring remain required before anomaly scoring can be considered statistically validated.
