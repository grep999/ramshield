# RamShield — One-Pager

**Wire-speed DDoS mitigation in Rust. Batch-first. Zero-trust IPC.**

## Problem
Volumetric and application-layer attacks overwhelm defenses that inspect packets one-at-a-time. Python/Go pipelines add GC pauses and per-event overhead exactly when traffic spikes.

## Solution
RamShield is an async, batch-first traffic defense engine written in Rust. It scores threats in-line, adapts with lightweight ML, and streams live stats — with no garbage collector in the hot path.

## How It Works
- **Engine** — Async Tokio, batch-first pipeline (amortizes per-event cost)
- **Storage** — Sharded `DashMap` with a TTL wheel for O(1) expiry
- **Detection** — EWMA baseline + promotion filter to cut false positives
- **Forecasting** — Holt-Winters + Shannon entropy for anomaly lead time
- **Dashboard** — Axum HTTP + SSE live streaming
- **IPC** — TCP JSON with batch APIs (roadmap: TLS 1.3, SHA-256 signed audit log)

## Why It's Different
| | RamShield | Typical stack |
|---|---|---|
| Hot path | No GC | GC pauses under load |
| Model | Batch-first | Per-event |
| Detection | EWMA + entropy forecasting | Static thresholds |
| IPC | Zero-trust (roadmap) | Plaintext |

## Numbers
- 24 Rust modules, ~4,800 LOC
- JSON IPC sustains ~5M events/s (Protobuf under evaluation)
- Target: 2x faster inference than Python baseline

## Status
Beta on `feature/ramshield-advanced`. Roadmap: XGBoost scoring, tflite-rust inference, TLS 1.3 IPC, K8s operator, WASM custom rules.

## Links
- Repo: https://github.com/grep999/ramshield
- Crate: https://crates.io/crates/ramshield *(pending publish)*

_One-pager for outreach. Update numbers from `docs/BENCHMARKS.md` once criterion runs land._
