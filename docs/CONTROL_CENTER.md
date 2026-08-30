# RamShield Control Center

Human-readable overview of the autonomous agent fleet. Updated by the reviewer agent.

**Last review:** 2026-08-30 · **Branch:** `prod-up` @ `fcd8f8f` · **Pipeline state:** OPERATIONAL

## Agent Status

| Agent | Schedule | Last Run | Status | Output Artifact |
| :--- | :--- | :--- | :--- | :--- |
| facts-collector | */30 min | 2026-08-30 ok | ✅ repo-bound | `docs/FACTS.json` |
| daily-planner | 0 1 * * * | 2026-08-30 ok | ✅ | `docs/PLAN.md` |
| dispatcher | 30 1 * * * | 2026-08-30 ok | ✅ pinned | `docs/DISPATCH_LOG.md` |
| workers | repeat:1 | 2026-08-30 | ✅ dispatched | `docs/WORKER_STATUS.md` |
| reviewer | 0 3 * * * | 2026-08-30 ok | ✅ | `docs/REVIEW.md` |
| helper-agent | */10 min | 2026-08-30 ok | ✅ | git commits `[skip ci]` |
| health-loop | */15 min | 2026-08-30 ok | ✅ | `docs/HEALTH_CHECK.md` |
| health-repair | hourly | 2026-08-30 ok | ✅ | — |
| error-healer | */30 min | 2026-08-30 ok | ✅ | `docs/HEALER_DISPATCH.md` |
| cron-status | */5 min | 2026-08-30 ok | ✅ | `docs/CRON_STATUS.{md,json}` |
| pulse | */5 min | 2026-08-30 ok | ✅ | `docs/PULSE_LOG.md` |
| research-agent | hourly | 2026-08-30 ok | ✅ | `docs/RESEARCH.md` |
| git-automation | */15 min | 2026-08-30 ok | ✅ | feature branch commits |
| backup | 0 2 * * * | 2026-08-30 ok | ✅ | — |
| promotion fleet (10 jobs) | staggered | 2026-08-30 ok | ✅ | `docs/PROMOTION_LOG.md` |

## Open Blockers

None. All previously blocking issues (config-drift spend-guard pinning, facts-collector workspace, missing `HEALTH_CHECK.md`) resolved as of 2026-08-30.

Full detail: [`REVIEW.md`](REVIEW.md) · [`AGENT_REPORT.md`](AGENT_REPORT.md) · [`DEPENDENCY_AUDIT.md`](DEPENDENCY_AUDIT.md) · Dashboard: [`AUTOMATION_DASHBOARD.html`](AUTOMATION_DASHBOARD.html) · Raw data: `docs/FACTS.json` · Fleet snapshot: `docs/CRON_STATUS.json`
