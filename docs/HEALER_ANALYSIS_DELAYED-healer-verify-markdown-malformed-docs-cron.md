# Healer Analysis — DELAYED-healer-verify-markdown-malformed-docs-cron

## Issue
- DELAYED: healer-verify-markdown-malformed-docs-cron_status-md-missing-tab — 1.5h overdue, last_status=never
- Category: cron

## Problem

### 1. The underlying markdown issue is already fixed
`docs/CRON_STATUS.md` already contains the GitHub-style `| :--- | :--- |` alignment rows that the health checker expects (`health_check_repair.py:178`). `docs/HEALTH_CHECK.md` (2026-07-21 08:18 UTC) reports:

```
## 3. Markdown Structure
✅ All critical markdown files OK.
```

This means the original `MALFORMED: docs/CRON_STATUS.md missing table alignment row` condition no longer exists.

### 2. The healer-verify job itself is stuck, not the markdown
The `healer-verify-markdown-malformed-docs-cron_status-md-missing-tab` one-shot job was scheduled for 2026-07-21 06:31 UTC but remains `pending`/`claimed` and never executed. Evidence from `docs/CRON_STATUS.md`:

```
| healer-verify-markdown-malformed-docs-cron_status-md-missing-tab | once at 2026-07-21 06:31 | ⏳ pending | claimed |
```

### 3. Root cause: global inference config drift for unpinned LLM jobs
`docs/CRON_STATUS.md` raw output shows multiple recurring LLM cron jobs failing with the exact same error:

- `ramshield-helper-agent` (e3652296ba99): `RuntimeError: Skipped to prevent unintended spend: global inference config drifted since this job was created (provider 'custom' -> 'opencode-go'; model 'ram' -> 'kimi-k2.7-code'), and this job is unpinned.`
- `ramshield-research-agent` (f270eaf2c891): same config-drift skip
- `ramshield-reviewer` (d72f32a35099): timed out waiting for API response

The healer-verify jobs are also LLM agent jobs (`Skills: autonomous-project-agents`) and are unpinned. They are therefore being skipped silently by the spend-guard, leaving them in `claimed`/`pending` state indefinitely. This is the same failure mode documented in the skill under *Pitfalls & Fixes*:

> **LLM Agent Cron Job Skipped with "config drift" spend-guard** → Either **pin the job** to current provider/model or **convert it to a `no_agent=true` script**.

### 4. Stale dispatcher state causes repeated false-positive delayed alerts
Because the stuck verify job never reports completion, `ramshield-error-healer` keeps detecting it as overdue and dispatching new analyze/solve/verify cycles. The second batch (scheduled for 08:13/08:23/08:33) is also at risk of the same config-drift skip.

## Evidence

| Source | Observation |
|--------|-------------|
| `docs/CRON_STATUS.md` lines 40, 44, 49 | `healer-verify-markdown-malformed-...` scheduled 06:31, `pending`/`claimed` |
| `docs/CRON_STATUS.md` raw output lines 110, 178 | `ramshield-helper-agent` and `ramshield-research-agent` skipped due to unpinned config drift |
| `docs/HEALTH_CHECK.md` line 17 | `✅ All critical markdown files OK.` — proves the markdown issue is gone |
| `docs/HEALER_SOLUTION_markdown-MALFORMED-docs-CRON_STATUS-md-missing-tab.md` | Cycle 1 already regenerated CRON_STATUS.md and verified the fix |
| `docs/HEALER_STATUS_markdown-MALFORMED-docs-CRON_STATUS-md-missing-tab.md` | Verify status reports the issue as fixed |
| `docs/HEALER_DISPATCH.md` | New analyze/solve/verify cycle dispatched at 08:13 because the verify job never completed |

## Exact Files / Lines

| File | Lines | Problem |
|------|-------|---------|
| `docs/CRON_STATUS.md` | 40, 44, 49 | Stuck healer-verify jobs at 06:31 and 06:35, never executed |
| `docs/CRON_STATUS.md` raw output | 110, 178 | Recurring LLM jobs skipped due to unpinned config drift |
| `docs/HEALTH_CHECK.md` | 7, 17 | Reports DELAYED verify job, but markdown structure is OK |
| `docs/HEALER_SOLUTION_markdown-MALFORMED-docs-CRON_STATUS-md-missing-tab.md` | 10-12 | Confirms CRON_STATUS alignment was already fixed |
| Hermes cron infra | N/A | Unpinned LLM jobs skipped when global provider/model drifts |

## Proposed Fix

1. **Pin the healer jobs** to the current provider/model so the spend-guard stops skipping them:
   ```bash
   hermes cron update job_id=<healer-verify-id> provider=opencode-go model=kimi-k2.7-code
   ```
   Also pin the recurring LLM jobs that are currently erroring:
   - `ramshield-helper-agent` (e3652296ba99)
   - `ramshield-research-agent` (f270eaf2c891)
   - `ramshield-reviewer` (d72f32a35099)

2. **Cancel the stale stuck verify jobs** from 06:31/06:35 so `ramshield-error-healer` stops re-dispatching for an already-resolved issue:
   ```bash
   hermes cron remove <id-of-stuck-verify-job>
   ```

3. **Convert deterministic collectors to `no_agent=true` scripts** where possible (already done for `facts_collector.py` and `cron_status_collector.py`) to reduce reliance on pinned LLM cron jobs for data-only tasks.

## One-line Summary
The markdown issue is already resolved; the verify job is delayed because it is an unpinned LLM cron job being silently skipped by the config-drift spend-guard, a failure mode shared with `ramshield-helper-agent` and `ramshield-research-agent`.
