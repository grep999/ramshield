# RamShield Control Center

Human-readable overview of the autonomous agent fleet. Updated by the reviewer agent.

**Last review:** 2026-08-23 · **Branch:** `operator` @ `b8cbc3b` · **Pipeline state:** BLOCKED (dispatcher unpinned/skipped)

## Agent Status

| Agent | Schedule | Last Run | Status | Output Artifact |
| :--- | :--- | :--- | :--- | :--- |
| facts-collector | */30 min | 2026-08-23 11:02 ok | ⚠️ WRONG WORKSPACE — writes `/home/m/docs/FACTS.json` | repo `docs/FACTS.json` stale 08-22 |
| daily-planner | 0 1 * * * | 2026-08-23 11:07 ok | ✅ recovered | `docs/PLAN.md` (untracked) |
| dispatcher | 30 1 * * * | 2026-08-22 01:30 **skipped** | ❌ config-drift spend-guard, unpinned | no `DISPATCH_LOG.md` |
| workers | repeat:1 | never | ⬜ none dispatched | no `WORKER_STATUS.md` |
| reviewer | 0 3 * * * | 2026-08-23 (this run) | ✅ first completed review | `docs/REVIEW.md` |
| helper-agent | */10 min | 2026-08-23 11:00 **failed** | ❌ TERMINAL_CWD lock timeout (#79768) | git commits `[skip ci]` |
| health-loop | */15 min | 2026-08-22 20:45 ok | ⚠️ `HEALTH_CHECK.md` missing from docs/ | — |
| health-repair | hourly | 2026-08-22 20:06 ok | ✅ | — |
| error-healer | */30 min | 2026-08-22 20:30 ok | ✅ | `docs/HEALER_DISPATCH.md` |
| cron-status | */5 min | 2026-08-22 20:55 ok | ✅ | `docs/CRON_STATUS.{md,json}` |
| pulse | */5 min | 2026-08-22 20:55 ok | ✅ | `docs/PULSE_LOG.md` |
| research-agent | hourly | 2026-08-22 20:05 ok | ✅ | `docs/RESEARCH.md` |
| git-automation | */15 min | 2026-08-22 20:45 ok | ✅ | feature branch commits |
| backup | 0 2 * * * | 2026-08-23 10:50 **error exit 1** | ❌ investigate | — |
| promotion fleet (10 jobs) | staggered | 2026-08-23 11:02 ok | ✅ | `docs/PROMOTION_LOG.md` |

## Open Blockers

1. **Config-drift spend-guard**: unpinned LLM jobs (dispatcher `c0d0d4bc8275`, reviewer `d72f32a35099`) silently skip every cycle. Fix: `hermes cron update job_id=<id> provider=custom model=zombobobo`.
2. **facts-collector workspace bug**: template fallback resolves to `/home/m`. Fix: set job workdir to repo path.
3. **HEALTH_CHECK.md vanished** from `rs/docs/` — T1 plan task unsatisfiable until regenerated.

Full detail: [`REVIEW.md`](REVIEW.md) · [`AGENT_REPORT.md`](AGENT_REPORT.md) · [`DEPENDENCY_AUDIT.md`](DEPENDENCY_AUDIT.md) · Dashboard: [`AUTOMATION_DASHBOARD.html`](AUTOMATION_DASHBOARD.html) · Raw data: `docs/FACTS.json` · Fleet snapshot: `docs/CRON_STATUS.json`
