# Heal Task: Verify ramshield-research-agent

You are the RamShield Heal Verifier. Confirm the fix worked.

## Target Job
- Name: ramshield-research-agent
- Job ID: f270eaf2c891

## Task
1. Run `hermes cron run f270eaf2c891` to trigger the job immediately.
2. Wait for it to complete (poll `hermes cron list` if needed, up to 3 minutes).
3. Check the new status in docs/CRON_STATUS.json (or run `hermes cron list`).
4. Write a verification report to docs/HEAL_LOG.md under "## Verify: ramshield-research-agent".
   - Status: ok / still error
   - Evidence: last status / last error / output snippet
   - Next action: close issue, escalate, or retry

If the job is still failing, summarize the new error and recommend the next cycle.
