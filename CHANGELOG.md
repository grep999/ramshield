# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **IPC frame authentication** — HMAC-SHA256 per-frame auth for TCP clients; Prometheus `/metrics` export on the dashboard.
- **Dashboard admin auth** — Argon2-hashed admin password + session-cookie middleware (`admin_password_hash` config or env).
- **Operator console** — standalone web dashboard (fleet, jobs, engine health, git, promo, bench panels) plus a terminal REPL; documented in `docs/OPERATOR_DOCS.md`.
- **Bench harness: `subnet_ddos_5min` profile** — 30 unique /24s rotated every 15 s per worker; loopback-only wrapper `scripts/subnet_ddos_bench.sh`.
- **SPOT-lite extreme-quantile alarm** (forecasting P2) — empirical tail estimation complements Holt-Winters z-score.
- **Inst-rate EWMA sample + capped CUSUM with debounce** (detection P1) — sharper burst response without cold-start false positives.
- Configurable block-log size (`[dashboard] block_log_size`, default 1000).

### Changed
- Subnet batch blocking now keys on **distinct source IPs** — a single host bursting 10 events no longer blocks its whole /24. *(behavior change)*
- Subnet-burst blocks get their own short TTL (120 s instead of inheriting the 1 h per-IP TTL).
- CUSUM warm-up allowance — benign cold-start ramps no longer accumulate evidence.
- Protocol requests use `deny_unknown_fields` — TTL typos fail loudly instead of silently blocking forever. *(breaking)*
- Deleted dead binary codec and legacy single-file modules; workspace unified into nine domain crates.

### Fixed
- Oversize IPC connections now receive a typed 413 error frame before close.
- Lock-poisoning handling hardened across storage paths.

## [0.2.0] - 2026-07-31

### Added
- Created `AGENTS.md` to establish and enforce coding standards and project structure.
- Implemented a "Self-Healing Protocol" requiring `build`, `clippy`, and `test` to pass before completing tasks.
- Added `CHANGELOG.md` to track notable changes between versions.

### Changed
- Updated project guidelines based on analysis of mature Rust projects like `jcode`.
- Refined the build process and verification steps for better CI/CD practices.

### Fixed
- Resolved build failure by removing a dead import (`ConnectionEventRecord`) from `src/engine/mod.rs`.
- Fixed clippy error by removing an unused constant (`CONNECTION_EVENT_HISTORY`) from `src/metrics/mod.rs`.
- Corrected working directory issue in agent's verification script execution.
