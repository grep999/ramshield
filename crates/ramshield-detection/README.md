# ramshield-detection

Consumes bounded connection telemetry, aggregates events, evaluates configured
detection signals, and emits enforcement decisions. It does not own the final
block state or the XDP dataplane; those belong to `ramshield-enforcement`.

## Processing Phases

1. **Ingest** — Raw telemetry events enter via the IPC channel into the
   `DetectionEngine`. Events are hashed into per-IP pre-aggregation buffers
   (`DashMap<IpAddr, IpAgg>`) under 64 shards (soft cap 8192 per worker),
   bounded by `batch_channel_capacity` (64k). Events outside the cap are
   dropped with observable metrics.

2. **Pre-aggregation flush** — On time (`batch_window_ms`, default 50ms) or
   size (`batch_max_events`, default 4096) triggers, the shared pre-agg map
   is flushed into per-IP state in the `Store`. The flush measures from the
   batch start instant (`pre_aggs_batch_start_ns`), not from idle scheduler
   ticks. Empty flushes only advance the timeout scheduler.

3. **Signal evaluation** — After flush, the following signals are evaluated
   per-IP and per-subnet:
   - **EWMA + CUSUM** — Rate deviation from baseline, sustained drift
   - **Pulse correlation** — Distinct over-threshold observations in a sliding
     window (configurable window size and sample threshold)
   - **Subnet batch** — Requires both a unique-IP threshold
     (`subnet_batch_threshold`) AND an event-volume threshold
     (`subnet_batch_min_events`). A single high-volume address is NOT
     equivalent to a distributed swarm.
   - **Relative baseline** (opt-in) — Qualified against representative
     traffic before use; has sample, factor, floor, and consecutive-breach
     controls.
   - **Application telemetry** — Optional bounded L7 and HTTP/2 metadata,
     evaluated only when enabled and when producers provide the fields.

4. **Emergency burst path** — The emergency per-IP burst path may emit an
   in-flight decision before a normal batch flush. Decision rate is bounded
   by pre-aggregation limits and flush intervals.

5. **Enforcement emission** — Detected decisions (`EnforceCommand`) are
   sent via mpsc channel to the `EnforcementService`. The detector does NOT
   own the final block state or XDP dataplane.

6. **CGNAT integration** — `CgnatGuard::classify_subnet(subnet, host_count,
   rate)` evaluates RFC 6598 address space (100.64.0.0/10) for graduated
   tiers: `Allow` (0), `Challenge` (1), `Block` (3). Rate + host-density
   checks combine with detection signals.

## Key Types

- `ConnectionEvent` — IP, timestamp, bytes, status code, proto fingerprint,
  optional L7 metadata
- `BlockDecision` — IP + reason + TTL + batch subnet
- `EnforceCommand` — decision payload with `action: Block/Unblock`, evidence
  source (`LocalSignals`, `FleetSignals`, `Operator`)
- `BlockReason` — wire vocabulary: `high_rps`, `subnet_burst`,
  `forecast_anomaly`, `entropy_anomaly`, `manual`, `l7_high_rps`, `l7_cost`,
  `http2_stream_abuse`

## Uniqueness

- **Deduplication** — `processed_results` HashMap (`decision_id → original
  EnforceResult`) prevents the same block from being applied twice during
  rapid detection. The `processed_order` VecDeque evicts oldest entries when
  the set exceeds 65K.
- **Subnet gate** — Dual gate requires both `subnet_batch_threshold` hosts AND
  `subnet_batch_min_events` events across a 4s boundary window.
- **Cooldown suppression** — Re-emission for a key inside its cooldown
  (ttl/2 or 30s for permanent blocks) is suppressed.

## Dependencies

- `ramshield-config` — `DetectionConfig` (thresholds, windows, bloom filter
  size, L7/HTTP2 detection)
- `ramshield-storage` — `Store` (per-IP state, subnet tables), traffic
  counters
- `ramshield-types` — `BlockReason`, `IpNetwork`, `ConnectionEvent`
- `ramshield-metrics` — `Metrics`
- `ramshield-cgnat` — `CgnatGuard`
- `ramshield-wal` — WAL recovery / repair

## Testing

87 unit tests plus root package integration tests that load the BPF program
and verify block/unblock behavior through `AyaXdpApplier`.