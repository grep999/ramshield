# RamShield Control Center

Human-readable overview of the autonomous agent fleet. Updated by the reviewer agent.

**Last review:** 2026-09-03 · **Branch:** test11 @ d6cb2e4 · **Pipeline state:** DISPATCHER-SKIPPED (4th cycle) · **Planner stale since 2026-08-31**

| Agent | Schedule | Last Run | Status | Output Artifact |
| :--- | :--- | :--- | :--- | :--- |
| facts-collector | */30 min | 2026-09-03 17:16 UTC | ✅ repo-bound | `docs/FACTS.json` |
| daily-planner | 0 1 * * * | 2026-08-31 19:11 UTC | ⚠️ stale (no run since Aug 31) | `docs/PLAN.md` |
| dispatcher | 30 1 * * * | 2026-09-03 08:33 UTC | ⏭️ skipped (spend-guard ×4) | `docs/DISPATCH_LOG.md` |
| workers | repeat:1 | 2026-08-30 | ✅ dispatched (prior cycle) | `docs/WORKER_STATUS.md` |
| reviewer | 0 3 * * * | 2026-09-03 03:00 UTC | ✅ this run | `docs/REVIEW.md` |
| helper-agent | */10 min | 2026-09-03 | ✅ | git commits `[skip ci]` |
| health-loop | */15 min | 2026-09-03 | ✅ (writes to `~/.hermes/docs/`, not repo) | `~/.hermes/docs/HEALTH_CHECK.md` |
| health-repair | hourly | 2026-09-03 | ✅ | — |
| error-healer | */30 min | 2026-09-03 | ✅ | `docs/HEALER_DISPATCH.md` |
| cron-status | */5 min | 2026-09-03 | ✅ | `docs/CRON_STATUS.{md,json}` |
| pulse | */5 min | 2026-09-03 | ✅ | `docs/PULSE_LOG.md` |
| research-agent | hourly | 2026-09-03 | ✅ | `docs/RESEARCH.md` |
| git-automation | */15 min | 2026-09-03 | ✅ | feature branch commits |
| backup | 0 2 * * * | 2026-09-03 | ✅ | — |
| promotion fleet (10 jobs) | staggered | 2026-09-03 | ✅ | `docs/PROMOTION_LOG.md` |

## Open Blockers

1. **Dispatcher spend-guard skip ×4** — unpinned LLM job silently dropped every cycle. Fix: `hermes cron update job_id=c0d0d4bc8275 provider=custom model=zombobobo`
2. **Planner stale** — last ran 2026-08-31. May be disabled or also hitting spend-guard. Verify and pin if needed.
3. **`docs/HEALTH_CHECK.md` missing from repo** — `health_check_repair.py` resolves workspace to `~/.hermes/` via `Path(__file__).parent.parent.parent`. Needs explicit path or `RAMSHIELD_WS` env var.
4. **auth.rs uncommitted** — `.unwrap()` → `.expect()` changes in working tree, not committed.

## Pending Code Changes

Uncommitted diff in `src/dashboard/auth.rs`: 2 prod `.unwrap()` → `.expect()` with ponytail comments (lines 148, 187). Ready to commit.

Full detail: [`REVIEW.md`](REVIEW.md) · [`AGENT_REPORT.md`](AGENT_REPORT.md) · [`DEPENDENCY_AUDIT.md`](DEPENDENCY_AUDIT.md) · Dashboard: [`AUTOMATION_DASHBOARD.html`](AUTOMATION_DASHBOARD.html) · Raw data: `docs/FACTS.json` · Fleet snapshot: `docs/CRON_STATUS.json`
