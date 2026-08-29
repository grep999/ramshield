# RamShield Roadmap

Dated milestones. Every item is a concrete, verifiable outcome — not a feature
request. When an item is marked **done**, the date is frozen.

---

## 0.3 — Fuzz & hardening (in progress)

Target: end of Q4 2026.

- [ ] **oss-fuzz integration** — corpus submission, continuous fuzzing on merged PRs.
  Signals: oss-fuzz project page shows green, no unfixed bugs > 90 days.
- [ ] **protocol fuzz coverage ≥ 90%** — `cargo-fuzz coverage` report shows
  ≥ 90% branch coverage on `Request` and `Response` deserialization.
- [ ] **Crash-free fuzz runs** — zero panics or aborts after 10M iterations
  across all harnesses.
- [ ] **Security audit** — third-party audit engagement signed. Report published
  or embargo until next minor. Tracked in issue #125.
- [ ] **Remove remaining production `.unwrap()/.expect()`** — `rg` returns only
  in test modules. Started in 0.2 (lock-poisoning case in `engine::boot_pipeline`),
  ~16 instances remain across 9 files. Tracked in issue #127.

---

## 1.0 — General Availability

**GA means**: no breaking changes without a major version bump, migration
path documented, change-logged. It does **not** mean "certified for every
compliance framework" — that follows enterprise adoption.

### Hard blockers (all must be done)

| Blocker | Verification |
|---------|--------------|
| Zero production `.unwrap()/.expect()` in `src/` | `rg '\.(unwrap\|expect)(' src/*.rs src/**/*.rs` returns only in test modules |
| WAL handles shared-backend (PostgreSQL / S3) | Integration test spins up Postgres, writes block, kills process, restarts, block present |
| Config hot-reload without restart | `SIGHUP` or `/api/reload` — old connections drain, new config applies |
| End-to-end test suite runs in < 60 s on a 4-core VM | `scripts/suite.py e2e` exit 0 in < 60 s |
| Docs cover every config field | Every key in `config.toml` has a doc comment in `src/config.rs` |
| CHANGELOG entries for all 0.x → 1.0 changes | `git log --oneline 0.2.0..HEAD --grep="feat\|fix\|perf"` |
| Published container image per release | `ghcr.io/grep999/ramshield:X.Y.Z` resolves; CI builds on tag |

### Soft requirements (nice to have before GA, hard blockers after)

| Item | Notes |
|------|-------|
| K8s operator | Required only if multi-replica with shared WAL is needed. Current manifests are sufficient for 95% of users. |
| `CHANGELOG.md` for 0.2 → 0.3 | Release note quality matching [Keep a Changelog](https://keepachangelog.com/) |
| crates.io download count ≥ 10k/month | Adoption signal, not a quality gate |
| 3+ production case studies | Anonymized is fine |

---

## Post-1.0

These ship after GA, no schedule set:

- **Multi-replica mode** — shared WAL backends (Postgres, S3, etcd)
- **Istio / Envoy Wasm filter** — native integration without a sidecar TCP proxy
- **Metric alerts** — Prometheus alerting rules for block storms, WAL lag, RAM pressure
- **Terraform / Pulumi provider** — declarative config management

---

*This document is source-controlled. If it contradicts CHANGELOG.md,
CHANGELOG.md wins — it records what actually shipped.*
