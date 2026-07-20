# Daily Plan — 2026-07-20

## State Assessment
`ramshield-facts-collector` cron job is failing due to an incorrect script path. This issue prevents upstream agents from collecting necessary data. Roadmap tasks for Q1 Advanced Analytics are pending. The most urgent task is to restore the facts collector's functionality.

## Prioritized Tasks
### T1: Fix Facts Collector Cron Job
- Target: Hermes cron job `1cb5e490c826`
- Action: Update the script path for `ramshield-facts-collector` to be relative to its workdir: `.github/scripts/facts_collector.py`.
- Verify: `hermes cron list` shows the updated script path for job `1cb5e490c826`.

### T2: Create FACTS.json Placeholder
- Target: `docs/FACTS.json`
- Action: Create an empty `docs/FACTS.json` file as a placeholder to prevent downstream agents from failing on missing input.
- Verify: `docs/FACTS.json` exists and is an empty JSON object.

