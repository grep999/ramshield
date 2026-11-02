# ramshield-learning

Passive production-environment discovery for RamShield. This crate is deliberately independent of the detection and enforcement crates.

## Responsibilities

- Aggregate bounded hourly traffic observations: packet/byte volume, protocol mix, approximate source/destination distributions and bounded destination-port heavy hitters.
- Learn recurring traffic shape by UTC hour-of-week, hour-of-month and a dedicated month-end profile.
- Maintain numerically stable online count moments and report Poisson / negative-binomial / underdispersed method-of-moments diagnostics.
- Provide a provisional descriptive hourly band; it is not a calibrated prediction interval or a production alert threshold.
- Return active operator-declared workload context alongside every assessment, without suppressing anomalies.
- Record operator-declared services and workloads before onboarding or scheduling them, including daily, weekly, monthly-day, month-end and N-day recurrence.
- Represent observed-zero, missing and degraded service windows as distinct states, with bounded idempotent ingestion.
- Track capture/ingestion quality and host resource observations.
- Persist observations and calendar declarations atomically.

See [STATISTICAL_DESIGN.md](STATISTICAL_DESIGN.md) for the model assumptions, validation protocol and Stage 0 acceptance gates.

## Safety boundary

This crate has no dependency on ramshield-detection, ramshield-enforcement, ramshield-xdp or firewall APIs. It cannot block traffic, change network state, activate thresholds or reload live configuration. Workload declarations add explanatory context; they do not delete, hide or automatically whitelist events.

## Integration contract

The existing native `ConnectionEvent` contract contains endpoint IP, timestamp, bytes, status/protocol metadata and optional L7 metadata; it does not contain packet counts, destination ports, service identity or authoritative traffic direction. The current metrics API likewise does not supply service attribution. There is therefore no lossless adapter from those contracts to every `TrafficSample` field yet. Do not invent packet counts or infer service identity/direction from endpoint hashes, IP addresses or ports. Until a producer supplies the missing dimensions, keep `service_id` absent and `direction` as `Unknown`; do not claim service-specific native learning is integrated.

The `WindowLedger` is a bounded, standalone service-window idempotency primitive. Its key is service, direction and fixed-window identity; its payload contains packet/byte totals and missing/degraded quality state, but not protocol totals or endpoint keys. It is not yet wired into `Discovery`'s D7/D8 profiles and is not a lossless replay source for all `TrafficSample` dimensions. It supports explicit snapshots through `flush` and `reopen`. Capacity eviction advances a global event-time floor: the entire oldest timestamp cohort is removed, and all subsequent windows at or below that timestamp are rejected across every service and direction. This prevents equal-timestamp replays without an unbounded tombstone registry, but conservatively rejects previously unseen services at or below the floor. Snapshot format version 3 stores the floor; older snapshot versions fail closed rather than being migrated. The ledger is not a per-record WAL, so records since the last successful flush can be lost on a crash. Schedule flushes from a background owner, never from the packet-capture hot path. `Discovery::flush` persists aggregate hourly buckets; the D7/D8 per-service metric/cardinality profiles are currently process-local and reset after restart. Address identifiers must be hashed by the caller before they are supplied; this crate does not store raw IP addresses. Call `TrafficCalendar::observe_hour` alongside aggregate traffic collection and use `declare_workload` to feed planned services/jobs into the calendar.

Workload execution records are bounded and validated on admission and reopen. An overrun is accepted only after the expected end time; `actual_duration_hours` records whole elapsed hours from the actual start (rounded down). Calendar recurrence is UTC-only today; IANA timezone and daylight-saving-aware scheduling are not implemented.

Store files should be owned by the RamShield service account and backed up with the deployment's normal state policy.

All calendar timestamps are Unix UTC seconds. Weekday numbering is Monday=0. The current calendar range uses mean +/- sigma as a descriptive heuristic only. The count-family diagnostic is a method-of-moments candidate, not a goodness-of-fit test or calibrated tail probability. It does not account for temporal autocorrelation and must not be used as a production alert threshold. Proper predictive intervals, calibration on chronological hold-out data, missing-window accounting, durable service-specific profiles, timezone-aware recurrence and drift monitoring remain required before anomaly scoring can be considered statistically validated.
