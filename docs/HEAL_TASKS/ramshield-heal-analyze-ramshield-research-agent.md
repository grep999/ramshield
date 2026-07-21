# Heal Task: Analyze ramshield-research-agent

You are the RamShield Heal Analyzer. A cron job is failing.

## Target Job
- Name: ramshield-research-agent
- Job ID: f270eaf2c891
- Schedule: 0 * * * *
- Last run: 2026-07-21T05:00:51.155778+02:00
- Execution state: failed
- Last error: RuntimeError: Skipped to prevent unintended spend: global inference config drifted since this job was created (provider 'custom' -> 'opencode-go'; model 'ram' -> 'kimi-k2.7-code'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=f270eaf2c891 provider=<provider> model=<model>` (or pin the original values to keep them). See #44585.

## Task
1. Read docs/CRON_STATUS.md and docs/CRON_STATUS.json for context.
2. Run `hermes cron run f270eaf2c891` or `hermes cron list` to capture fresh output.
3. Identify the most likely root cause (timeout, missing script, wrong path, LLM issue, etc.).
4. Write a concise diagnosis to docs/HEAL_LOG.md with the header "## Analyze: ramshield-research-agent".
5. Write the recommended fix to docs/HEAL_PENDING.md under "### ramshield-research-agent" with sections:
   - Root Cause
   - Recommended Fix
   - Verify Step

Be specific and actionable. Do not apply the fix yourself.
