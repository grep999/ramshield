# Healer Analysis — DELAYED-healer-solve-markdown-malformed-docs-cron_

## Issue
- DELAYED: healer-solve-markdown-malformed-docs-cron_status-md-missing-tab — 1.6h overdue, last_status=never
- Category: cron

## Problem

This is the **SOLVE stage** for the same markdown-malformed issue already analyzed in `docs/HEALER_ANALYSIS_DELAYED-healer-verify-markdown-malformed-docs-cron.md`.

### 1. The underlying markdown issue is already fixed
`docs/CRON_STATUS.md` already contains GitHub-style `| :--- | :--- |` alignment rows that `health_check_repair.py:178` expects. `docs/HEALTH_CHECK.md` (2026-07-21 08:18 UTC) reports:

```
## 3. Markdown Structure
✅ All critical markdown files OK.
```

So the `cron_status-md-missing-tab` condition no longer exists.

### 2. The solve job itself is stuck, not the markdown
The `healer-solve-delayed-healer-solve-markdown-malformed-docs-cron_` one-shot job was scheduled for 2026-07-21 08:23 UTC but is listed in `docs/CRON_STATUS.md` as `❓ unknown` with no execution record. The same is true for the analyze and verify jobs in this second-generation cycle (scheduled 08:13/08:23/08:33).

### 3. Root cause: global inference config drift for unpinned LLM jobs
`docs/CRON_STATUS.md` raw output shows the recurring LLM jobs failing with the same config-drift spend-guard error:

- `ramshield-helper-agent` (e3652296ba99): `RuntimeError: Skipped to prevent unintended spend: global inference config drifted since this job was created (provider 'custom' -> 'opencode-go'; model 'ram' -> 'kimi-k2.7-code'), and this job is unpinned.`
- `ramshield-research-agent` (f270eaf2c891): same config-drift skip
- `ramshield-reviewer` (d72f32a35099): timed out waiting for API response

The healer jobs are LLM agent jobs (`Skills: autonomous-project-agents`) and are unpinned. They are therefore being skipped silently by the spend-guard, leaving them in `claimed`/`pending`/`unknown` state indefinitely. This matches the failure mode documented in the skill under *Pitfalls & Fixes*.

### 4. Stale dispatcher state causes repeated false-positive delayed alerts
Because the original verify job (06:31) never completed, `ramshield-error-healer` re-dispatched analyze/solve/verify cycles at 08:13/08:23/08:33. These second-generation jobs are also at risk of the same config-drift skip. The current issue (`DELAYED-healer-solve-markdown-malformed-docs-cron_`) is one of those second-generation jobs.

## Evidence

| Source | Observation |
|--------|-------------|
| `docs/HEALTH_CHECK.md` line 17 | `✅ All critical markdown files OK.` — proves the markdown issue is gone |
| `docs/CRON_STATUS.md` lines 53-55 | `healer-solve-delayed-healer-solve-markdown-malformed-docs-cron_` scheduled 08:23, `❓ unknown` |
| `docs/CRON_STATUS.md` raw output lines 110, 178 | `ramshield-helper-agent` and `ramshield-research-agent` skipped due to unpinned config drift |
| `docs/HEALER_ANALYSIS_DELAYED-healer-verify-markdown-malformed-docs-cron.md` | Already identified the same root cause for the verify-stage counterpart |
| `docs/HEALER_DISPATCH.md` | New analyze/solve/verify cycle dispatched at 08:13 because the 06:31 verify job never completed |

## Exact Files / Lines

| File | Lines | Problem |
|------|-------|---------|
| `docs/CRON_STATUS.md` | 53-55 | Stuck healer-solve job at 08:23, never executed |
| `docs/CRON_STATUS.md` raw output | 110, 178 | Recurring LLM jobs skipped due to unpinned config drift |
| `docs/HEALTH_CHECK.md` | 7, 17 | Reports DELAYED solve job, but markdown structure is OK |
| Hermes cron infra | N/A | Unpinned LLM jobs skipped when global provider/model drifts |

## Proposed Fix

1. **Pin the healer jobs** to the current provider/model so the spend-guard stops skipping them:
   ```bash
   hermes cron update job_id=<healer-solve-id> provider=opencode-go model=kimi-k2.7-code
   ```
   Also pin the recurring LLM jobs that are currently erroring:
   - `ramshield-helper-agent` (e3652296ba99)
   - `ramshield-research-agent` (f270eaf2c891)
   - `ramshield-reviewer` (d72f32a35099)

2. **Cancel the stale stuck healer jobs** from 06:31/06:35 and the 08:13/08:23/08:33 cycle so `ramshield-error-healer` stops re-dispatching for an already-resolved issue:
   ```bash
   hermes cron remove <id-of-stuck-solve-job>
   ```

3. **Convert deterministic collectors to `no_agent=true` scripts** where possible (already done for `facts_collector.py` and `cron_status_collector.py`) to reduce reliance on pinned LLM cron jobs for data-only tasks.

## One-line Summary
The markdown issue is already resolved; the solve-stage healer job is delayed because it is an unpinned LLM cron job being silently skipped by the config-drift spend-guard, a failure mode shared with `ramshield-helper-agent` and `ramshield-research-agent`.
