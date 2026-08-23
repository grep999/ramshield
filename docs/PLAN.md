# Daily Plan — 2026-08-23

## State Assessment
**Pipeline incident:** this morning's facts-collector run wrote `/home/m/docs/FACTS.json` (workspace=`/home/m`) instead of the repo's `docs/FACTS.json`. Root cause: `~/.hermes/scripts/facts_collector.py` resolves workspace as `Path(__file__).parent.parent.parent` (= `/home/m`) when `GITHUB_WORKSPACE` is unset; the cron job's `Workdir:` field is ignored by the script. The stray FACTS scanned home-dir junk: 8 bogus TODOs from a foreign `src/enforcement/mod.rs`, git "unknown", empty deps. Repo `docs/FACTS.json` is stale (2026-08-22T16:57Z).

Valid ground truth from yesterday's FACTS.json: branch `operator` @ b8cbc3b, 9 Rust files / 1,934 LOC, 0 clippy warnings, **0 TODOs/FIXMEs**, no dead links, codebase healthy. No REVIEW.md exists yet — nothing to skip. `docs/PLAN.md` and `docs/CONTROL_CENTER.md` are missing from the repo docs tree. Duplicate roadmaps present: `docs/roadmap.md` AND `docs/ROADMAP.md`.

## Prioritized Tasks

### T1: Fix facts-collector workspace resolution
- Target: `~/.hermes/scripts/facts_collector.py` (line ~17)
- Action: Change the workspace fallback from `str(Path(__file__).resolve().parent.parent.parent)` to the repo path `'/home/m/vehicle_of_rationalism/ramshield/beta/rs'` (keep `GITHUB_WORKSPACE` override first).
- Verify: `python3 -W error /home/m/.hermes/scripts/facts_collector.py` exits ok AND `python3 -c "import json;print(json.load(open('docs/FACTS.json'))['workspace'])"` prints `/home/m/vehicle_of_rationalism/ramshield/beta/rs`; also `rm /home/m/docs/FACTS.json` (stray artifact from bad run).

### T2: Create CONTROL_CENTER.md
- Target: `docs/CONTROL_CENTER.md`
- Action: Create the human-readable overview the reviewer job is contracted to update: current state summary (from valid FACTS.json), agent/cron status table sourced from `docs/CRON_STATUS.md` (17 ok / 5 error as of last snapshot), links to AGENT_REPORT/DEPENDENCY_AUDIT/AUTOMATION_DASHBOARD.html.
- Verify: file exists, contains `## Agent Status` section and relative links that resolve (`test -f` each target).

### T3: Deduplicate roadmap files
- Target: `docs/roadmap.md`, `docs/ROADMAP.md`
- Action: Diff the two; keep `ROADMAP.md` as canonical, fold any unique content from `roadmap.md` into it, replace `roadmap.md` with a one-line pointer (`See [ROADMAP.md](ROADMAP.md).`) or delete it if identical.
- Verify: `grep -rl 'roadmap' docs --include='*.md' -i` shows no second divergent copy; FACTS collector next run reports roadmap tasks once (not doubled).

## No Work Needed
Not applicable — T1 above is urgent: every downstream agent (planner, reviewer, dashboard) consumes FACTS.json, which is currently being poisoned by the home-dir scan.
