# Atomic Backlog — Pickup Queue

Small, independently-executable tasks. Each is atomic: one file, one clear outcome.
Picked by the Daily Planner or Pulse agent into the daily plan when capacity allows.
Priority order: P0 (critical path) → P3 (nice-to-have).

## P0 — Critical Path (core functionality)
1. [x] Implement `src/learning/xgboost.rs` module stub with public `score()` fn
2. [x] Implement `src/learning/preprocess.rs` with `normalize()` stub
3. [x] Add `cargo clippy` CI gate to `.github/workflows/`
4. [ ] Fix dead link in `DOCUMENTATION.md` (empty target)
5. [ ] Write `src/engine/mod.rs` re-export for learning submodule
6. [ ] Add unit test skeleton for `xgboost::score`
7. [ ] Pin `tflite-rust` version in Cargo.toml
8. [x] Add `--version` flag to main binary
9. [ ] Document IPC protocol in `docs/IPC.md`
10. [ ] Add `make bench` target to Makefile

## P1 — Hardening & Observability
11. [ ] Add structured logging via `tracing` crate
12. [ ] Emit metrics to stdout in Prometheus format
13. [ ] Add `healthcheck` HTTP endpoint stub
14. [ ] Write integration test for engine startup
15. [ ] Add panic hook that logs to `stderr` with timestamp
16. [ ] Cover `preprocess::normalize` with property test
17. [ ] Add `RUST_LOG` env support
18. [ ] Benchmark `score()` with criterion
19. [ ] Add OpenTelemetry trace spans (no-op default)
20. [ ] Self-test script: `scripts/selftest.sh`

## P2 — Interface & UX
21. [ ] Add `status` subcommand to CLI
22. [ ] Render engine stats as JSON via `status --json`
23. [ ] Add `config validate` subcommand
24. [ ] Tab-completion script for bash/zsh
25. [ ] Man page stub in `docs/ramshield.1`
26. [ ] Colorized terminal output behind `--color=auto`
27. [ ] Add `interactive` REPL mode stub
28. [ ] Config file watcher (reload on change)
29. [ ] Add `explain` command for rule dump
30. [ ] History file for REPL (`~/.ramshield_history`)

## P3 — Outreach & Ecosystem
31. [ ] Write `CONTRIBUTING.md`
32. [ ] Add `CODE_OF_CONDUCT.md`
33. [ ] Security policy `SECURITY.md`
34. [ ] `CHANGELOG.md` initialized with v0.1.0
35. [ ] GitHub issue template (bug)
36. [ ] GitHub issue template (feature)
37. [ ] PR template with checklist
38. [ ] Add `FUNDING.yml` for sponsors
39. [ ] Write `docs/QUICKSTART.md`
40. [ ] Add `examples/minimal.toml` config

## P0-cross — Promotion & Growth (promo agent queue)
41. [x] Draft LinkedIn post: "Why we built RamShield in Rust"
42. [x] Write Hacker News show post draft
43. [x] Create `docs/PITCH.md` one-pager
44. [ ] Record 30s demo GIF of dashboard
45. [ ] Tweet thread on DDoS mitigation architecture
46. [x] Add `README` badges (build, clippy, coverage)
47. [ ] Submit to crates.io as `ramshield-core`
63| [x] Write dev.to article: "Zero-trust IPC in 50 lines"
64| [x] Add `docs/BENCHMARKS.md` from criterion runs
50. [ ] Schedule weekly blog post in `docs/BLOG_CALENDAR.md`
