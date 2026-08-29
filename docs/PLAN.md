# Daily Plan — 2026-08-29

## State Assessment
FACTS.json healthy: branch `operator` @ `1be62b1`, 9 Rust files / 1,934 LOC, **0 clippy warnings, 0 TODOs, 0 dead links**. Pipeline recovered — facts-collector workspace bug fixed (T1 from 8/23 done: fallback is now hardcoded repo path, FACTS now scans the right tree). `docs/CONTROL_CENTER.md` exists (T2 done). `docs/roadmap.md` is a one-line pointer to `ROADMAP.md` (T3 done). 22 roadmap tasks tracked, all long-horizon (ML scoring, TLS 1.3, WASM, etc.) — none is single-cycle work.

Outstanding non-code items from 8/23 REVIEW still open: dispatcher/reviewer LLM jobs unpinned (cron-layer, config-drift spend-guard); `ramshield-backup` exit 1; helper-agent TERMINAL_CWD contention; 50+ `[skip ci]` helper commits polluting history. These are infra/git-hygiene tasks — out of scope for single LLM worker, belong to cron/maintainer.

No code-level defects detected. Plan focuses on cheap, high-value single-agent improvements: closing the loop on the backup failure (debug), adding one missing doc artifact, and a small dashboard refinement.

## Prioritized Tasks

### T1: Diagnose ramshield-backup exit 1
- Target: `~/.hermes/scripts/backup_project.sh` (or equivalent; locate via `which` / `ls ~/.hermes/scripts/`)
- Action: Run the script manually, capture stderr, identify the failing step (mount? tar? rsync? disk full?). Apply the minimum fix (one-line guard, path, or flag). If the script is missing entirely, create a minimal `backup_project.sh` that tars `rs/` to `~/backups/rs-$(date +%F).tar.gz` with rotation (keep last 7).
- Verify: re-run script, exit 0; `ls -lt ~/backups/rs-*.tar.gz | head -1` shows fresh artifact; next `ramshield-backup` cron tick (`hermes cron list` filter) shows `ok`.

### T2: Add a dead-link self-check to facts-collector
- Target: `~/.hermes/scripts/facts_collector.py`
- Action: Currently `dead_links: []` is always empty (FACTS shows the field but no collection). Add a tiny doc-tree walk that lists `docs/*.md`, extracts `[text](path)` links (relative only, skip `http(s)://` and anchors), and writes `dead_links: ["<file>: Broken link to '<target>'", ...]` for any target that does not exist under `docs/`. Skip if target starts with `http` or `#`. Dedupe via `set`. Cap at 50 entries.
- Verify: `python3 -W error ~/.hermes/scripts/facts_collector.py`; `python3 -c "import json;print(json.load(open('docs/FACTS.json'))['dead_links'])"` shows a list (possibly empty); introduce a deliberate broken link in a temp `docs/_probe.md`, re-run, confirm it surfaces, then remove probe.

### T3: Document the helper-agent TERMINAL_CWD workaround
- Target: `docs/CONTROL_CENTER.md` (append a "Known Issues" subsection)
- Action: Add a 3-line note: helper-agent cron job times out on TERMINAL_CWD read-lock (#79768) when a previous `terminal(background=true)` is still alive in the shared session. Mitigation: stagger helper-agent schedule (`*/13 * * * *` instead of `*/10`) or set `workdir: /tmp` to drop out of the contested cwd. Cite the review finding.
- Verify: file still parses as Markdown; `grep -c "TERMINAL_CWD" docs/CONTROL_CENTER.md` >= 1.

## No Work Needed
Not applicable — T1/T2/T3 are all < 15-min single-agent tasks, and T1 has been a known failure since 8/23.
