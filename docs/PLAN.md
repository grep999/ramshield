# Daily Plan — 2026-07-31

## State Assessment
10 days since last plan. T1 (fix facts collector) + T2 (create FACTS.json placeholder) completed per REVIEW.md (2026-07-24). Facts collector now runs clean: 22833 bytes, 22 roadmap tasks, 0 dead links. HEALTH_CHECK.md (2026-07-21) shows 5 frozen files but 0 active errors. Backlog: 18 items remaining (P2 #24-30 = 7 items, P3 #31-40 = 10 items, promo items = 1). Git shows only helper agent [skip ci] commits — no real code changes. Frozen HEALTH_LOOP.md is 10+ days stale. Next priorities: unstuck frozen telemetry, then P2 UX backlog tasks.

## Prioritized Tasks
### T1: Diagnose and fix frozen HEALTH_LOOP.md
- Target: `.github/scripts/health_loop.py`, `docs/HEALTH_LOOP.md`
- Action: Verify `health_loop.py` compiles and runs; if broken, fix. If cron job is down, note the cron job ID. Append a fresh health row to HEALTH_LOOP.md.
- Verify: `docs/HEALTH_LOOP.md` has a timestamp row from today (2026-07-31).

### T2: Implement bash/zsh tab-completion script
- Target: `scripts/completion.sh` (new) or `scripts/completion.bash`
- Action: Write a tab-completion script for the `ramshield` binary: list subcommands (status, config, explain, etc.) and flags. Sourceable via `. completion.sh`.
- Verify: `bash -n scripts/completion.sh` passes syntax check, and the script contains `complete -F` for `ramshield`.

### T3: Create man page stub in docs/ramshield.1
- Target: `docs/ramshield.1`
- Action: Write a minimal troff man page for the `ramshield` binary: NAME, SYNOPSIS, DESCRIPTION, OPTIONS (--version, --help, status, config), SEE ALSO.
- Verify: `man -l docs/ramshield.1 2>&1 | head -5` renders without errors.

### T4: Batch-add 6 project documentation files
- Target: `docs/CONTRIBUTING.md`, `docs/CODE_OF_CONDUCT.md`, `docs/SECURITY.md`, `CHANGELOG.md`, `.github/ISSUE_TEMPLATE/bug.md`, `.github/ISSUE_TEMPLATE/feature.md`
- Action: Create minimal but functional project docs from standard templates. CONTRIBUTING: build instructions + PR process. CODE_OF_CONDUCT: standard Contributor Covenant. SECURITY: reporting process. CHANGELOG: v0.1.0 placeholder. Issue templates: YAML frontmatter with sections.
- Verify: All 6 files exist and contain more than 200 meaningful bytes (not just placeholders).

### T5: Add colorized terminal output behind `--color=auto`
- Target: `src/cli.rs` or equivalent entry point
- Action: Add a `--color` flag (default `auto`: use color when stderr is a tty). Wrap existing stderr/status messages in `\x1b[...m` escape codes. Use `owo-colors` if available, else ANSI escapes directly.
- Verify: `cargo check --no-default-features` passes; `--help` shows the `--color` flag.