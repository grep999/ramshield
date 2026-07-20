# HN Show Post Draft — RamShield

**Status:** [PENDING PUBLISH]

---

**Title:** Show HN: RamShield – Wire-speed DDoS mitigation in Rust, batch-first, zero external deps

**Body:**

Hey HN — we built RamShield, an open-source DDoS detection and mitigation engine in Rust.

The core problem: traditional mitigation pipelines process events one at a time. Under attack traffic, that means N syscall round-trips and GC pauses exactly when you need speed. RamShield uses a batch-first architecture — events accumulate for up to 50ms, then get scored in bulk. This cuts per-event overhead by orders of magnitude.

Key design decisions:
- **No GC in the hot path.** Rust + Tokio async. Memory stays flat under load.
- **Sharded state store.** DashMap with TTL wheel. O(1) insert/lookup/expiry per shard.
- **Adaptive detection.** EWMA baseline + Shannon entropy forecasting. Catches ramp-up attacks that static thresholds miss.
- **Zero external deps.** No Redis, no Kafka, no databases. Single binary, TCP JSON in, decisions out.
- **Live dashboard.** Axum HTTP + SSE streaming at port 9999.

Numbers so far:
- ~4,800 LOC across 24 Rust modules
- JSON IPC sustains ~5M events/s
- Sub-50ms scoring latency at p99

It's beta. We're still working on XGBoost scoring, tflite-rust inference, and TLS 1.3 for the IPC layer.

Repo: https://github.com/grep999/ramshield

Curious what the HN community thinks — especially around the batch-first vs streaming tradeoff. What are we missing?

---

**Tag:** Show HN
**Submissions:** [PENDING PUBLISH] — requires manual HN account post
