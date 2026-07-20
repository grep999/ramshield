# Deep Dive: RamShield's DDoS Mitigation Architecture

*Blog draft — Week 3, July 2026 (Blog Calendar #50). [PENDING PUBLISH]*

## TL;DR

RamShield is a Rust-native DDoS mitigation engine. It does connection
tracking, rate limiting, and anomaly scoring in-process — no sidecar, no
Userspace↔kernel bounce per packet. This post walks the data path from
socket to verdict.

## The problem with appliance-style mitigation

Traditional appliances sit *out of band* and react after the flood reaches
you. Per-packet inspection in userspace means syscalls and copies dominate
the cost. At 1M pps you burn a core just moving bytes.

## RamShield's data path

1. **Capture** — packets enter via an `axum`/`tower` front end for the
   control plane; the data plane tracks flows in a `dashmap` sharded by
   `ahash` of the 4-tuple. Sharding avoids a global lock on the hot path.
2. **Rate limit** — token buckets per flow, backed by `arc-swap` config so
   thresholds reload without restarting the engine.
3. **Score** — `learning::xgboost::score()` produces an anomaly score from
   normalized features (`preprocess::normalize`). Low-latency path; the
   model is pinned (`tflite-rust = "=0.3.0"`).
4. **Verdict** — allow / throttle / drop decided in one pass, written back
   to the flow table.

## Why Rust

- `dashmap` + `ahash`: lock-free concurrent flow table.
- `arc-swap`: atomic config swaps, zero-downtime policy updates.
- `tokio` full runtime for the control plane; the data plane stays on
  tight, allocation-light hot loops.
- `crc32fast` / `lz4_flex` for cheap integrity + compression on telemetry.

## Numbers

See `docs/BENCHMARKS.md` (criterion runs). Roughly: flow-table insert +
score + verdict stays under X µs per packet on a single core. (Fill exact
figure from latest bench before publish.)

## What's next

Next week: "RamShield vs. Traditional Firewalls" — head-to-head on the same
hardware.

---
*Draft status: [ ] Drafted [x] / [ ] Reviewed / [ ] Scheduled / [ ] Published*
