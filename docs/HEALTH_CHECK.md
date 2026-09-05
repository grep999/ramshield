# Health Check

## Status
OK — no jobs in Error state. Cron fleet: 2 ok, 0 errors, 8 running, 14 scheduled (24 tracked).

## Last run
2026-09-03 19:16 CEST — `ramshield-pulse` (`*/5 * * * *`) last completed.
Health snapshot timestamp: 2026-09-03 17:16 UTC (`docs/CRON_STATUS.md`).

## Metrics
- Source: `docs/CRON_STATUS.md` (live `hermes cron list` snapshot).
- Regeneration: every 5 minutes by `ramshield-cron-status` (`*/5 * * * *`).
- Repair loop: `ramshield-health-repair` (`0 * * * *`) runs hourly to fill missing
  artifacts (this file is one such artifact).
- Pipeline chain: facts → planner → dispatcher → workers → reviewer.
  Health-loop downstream of `ramshield-reviewer` (`0 3 * * *`).
- Backup: `ramshield-backup` (`0 2 * * *`) — last run 2026-08-30 08:33 UTC.

## Notes
- `docs/HEALTH_CHECK.md` is a low-cardinality artifact: full cron table lives in
  `docs/CRON_STATUS.md`; operator narrative in `docs/OPERATOR_LOG.md`.
- If `ramshield-health-repair` re-creates this file with stale values, ignore —
  next `ramshield-cron-status` tick (≤5 min) overwrites the upstream truth.
