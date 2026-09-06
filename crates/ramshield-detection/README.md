# ramshield-detection

Per-IP event aggregation, subnet batching, rate tracking, and threat scoring.

## Architecture

```
IPC events → batch::PreAggregator (50ms window)
                │
                ▼
          IpAgg (per-IP counters: count, bytes, status_dist, proto_fp)
                │
                ▼  (flush)
          detection::Engine
                ├── promote_to_store() — hot IP → Store
                ├── subnet detection (24h window, dual-gate: 50 IPs + 100 events)
                ├── pulse tracking (burst pattern detection)
                └── threat scoring (per-IP, multi-signal)
```

## Key Components

### batch::PreAggregator
- DashMap<IpAddr, IpAgg> — lock-free concurrent aggregation
- 50ms flush window, bounded channel (16K capacity)
- `IpAgg::absorb()`: per-event update (count, bytes, status bucket, proto_fp)
- `flush()`: drain to Engine + spawn subnet window update

### detection::Engine
- `process_event_into_pre_aggs()`: main entry point
- Subnet detection: tracks /24 (v4) or /64 (v6) windows
  - Dual gate: 50 unique IPs + 100 events in 2s window
  - TTL: 10s (short, prevents stale subnet blocks)
  - Uses `STATUS_BUCKET` const table for status code classification
- Rate tracker: per-IP EWMA rate with CUSUM drift detection
- Pulse tracker: burst pattern detection across time windows
- Threat scoring: multi-signal (volume, rate, status distribution)

### rate_tracker
- Per-IP EWMA rate tracking with configurable smoothing
- CUSUM drift detector for sustained rate increases
- Tripwire: max threat score clamping to prevent single-signal dominance
- Bounded step: caps rate delta to prevent outlier distortion

## Subnet Detection Logic
```
for each /24 (v4) or /64 (v6):
    if unique_ips >= 50 AND events >= 100 in 2s window:
        → BLOCK entire subnet
    if window expires (10s TTL):
        → UNBLOCK
```

## 30 tests
- batch: aggregate counts, byte order, subnet key roundtrip
- detection: IP tracking, IPv6 swarm, pulse detection, rate convergence
- CUSUM: quiet stays zero, sustained drift fires, noise rejection
- subnet: dual-gate (50+100), cold IP not stored, hot IP promotion
- storage integration: block/unblock, TTL expiry, concurrent transitions
