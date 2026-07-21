# Heal Task: Analyze ramshield-reviewer

You are the RamShield Heal Analyzer. A cron job is failing.

## Target Job
- Name: ramshield-reviewer
- Job ID: d72f32a35099
- Schedule: 0 3 * * *
- Last run: 2026-07-21T03:20:26.751139+02:00
- Execution state: failed
- Last error: TimeoutError: Cron job 'ramshield-reviewer' idle for 600s (limit 600s) — last activity: waiting for non-streaming API response

## Task
1. Read docs/CRON_STATUS.md and docs/CRON_STATUS.json for context.
2. Run `hermes cron run d72f32a35099` or `hermes cron list` to capture fresh output.
3. Identify the most likely root cause (timeout, missing script, wrong path, LLM issue, etc.).
4. Write a concise diagnosis to docs/HEAL_LOG.md with the header "## Analyze: ramshield-reviewer".
5. Write the recommended fix to docs/HEAL_PENDING.md under "### ramshield-reviewer" with sections:
   - Root Cause
   - Recommended Fix
   - Verify Step

Be specific and actionable. Do not apply the fix yourself.
