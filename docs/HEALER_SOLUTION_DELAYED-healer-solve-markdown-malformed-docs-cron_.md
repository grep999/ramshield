# Healer Solution Log — DELAYED-healer-solve-markdown-malformed-docs-cron_

## Issue
- DELAYED: healer-solve-markdown-malformed-docs-cron_status-md-missing-tab — 1.6h overdue, last_status=never
- Category: cron
- Cycle: 1

## Root Cause
The underlying markdown-malformed issue is already resolved:

- `docs/HEALTH_CHECK.md` (2026-07-21 09:12 UTC) reports **✅ All critical markdown files OK.**
- `docs/CRON_STATUS.md` already contains GitHub-style `| :--- | :--- |` alignment rows, matching what `health_check_repair.py:178` expects.
- `docs/HEALER_SOLUTION_DELAYED-healer-verify-markdown-malformed-docs-cron.md` documented the same resolution for the verify-stage counterpart.

The `healer-solve-delayed-healer-solve-markdown-malformed-docs-cron_` one-shot cron job is listed as overdue with `last_status=never` because it is an unpinned LLM agent cron job. The global Hermes inference config has drifted from `provider=custom, model=ram` to `provider=opencode-go, model=kimi-k2.7-code`, so the spend-guard silently skips unpinned LLM jobs. This is the same failure mode affecting `ramshield-helper-agent`, `ramshield-research-agent`, `ramshield-pulse`, and the other second-generation healer jobs.

## Changes Applied
- No repository source edits were needed; the markdown issue is already fixed.
- Wrote this solution log to record the diagnosis and the fact that the delayed alert is a stale stuck healer job, not an active repository problem.
- Updated `docs/HEALER_STATUS_DELAYED-healer-solve-markdown-malformed-docs-cron_.md` to **Fixed? Yes**.
- Re-ran `python3 -W error .github/scripts/health_check_repair.py` to confirm the markdown check still passes (1 unrelated dead-link issue remains in a different healer solution file).

## Verification
- `docs/HEALTH_CHECK.md` (2026-07-21 09:12 UTC):
  - Markdown Structure: ✅ All critical markdown files OK.
  - Dashboard Sections: ✅ All 13 dashboard sections present.
  - FACTS.json Health: ✅ FACTS.json valid and fresh.
- `docs/CRON_STATUS.md` contains the required `| :--- | :--- |` alignment row.
- `docs/HEALER_STATUS_DELAYED-healer-solve-markdown-malformed-docs-cron_.md` now marks this issue as fixed.

## Remaining Cron-Layer Action
The real fix belongs at the Hermes cron infrastructure layer, not in repo files:

1. Pin the recurring LLM jobs so the spend-guard stops skipping them:
   ```bash
   hermes cron update job_id=e3652296ba99 provider=opencode-go model=kimi-k2.7-code
   hermes cron update job_id=f270eaf2c891 provider=opencode-go model=kimi-k2.7-code
   hermes cron update job_id=076a9de35470 provider=opencode-go model=kimi-k2.7-code
   hermes cron update job_id=18e3993ed6a0 provider=opencode-go model=kimi-k2.7-code
   ```

2. Cancel the stale stuck healer one-shot jobs for this already-resolved markdown issue:
   - `healer-solve-delayed-healer-solve-markdown-malformed-docs-cron_` (42f51206db88)
   - `healer-verify-delayed-healer-solve-markdown-malformed-docs-cron_` (8277444c53d5)
   - Related third-generation cycle jobs scheduled at 08:46/08:56/09:06 should also be reviewed for cancellation if they target the same resolved markdown issue.

## One-line Summary
Resolved stale delayed-healer alert by confirming the markdown issue is already fixed and documenting that the stuck solve job was caused by the unpinned LLM cron config-drift spend-guard.
