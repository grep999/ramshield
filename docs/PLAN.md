# Daily Plan — 2026-08-31

## State Assessment
- Facts‑collector mis‑directed output to `/home/m/docs/FACTS.json` (wrong workspace); repo `docs/FACTS.json` stale and missing current git state.
- Daily planner ran and produced `docs/PLAN.md` (untracked in git).
- Dispatcher skipped this cycle due to config‑drift spend‑guard (unpinned LLM job); no workers spawned.
- Production code in `src/dashboard/mod.rs` and `src/dashboard/auth.rs` still contains `.unwrap()`/`.expect()` calls flagged by roadmap “Remove remaining .unwrap/.expect”.
- `docs/HEALTH_CHECK.md` missing from repo (output vanished from last health‑loop run).
- `docs/PLAN.md` untracked; not committed to git.
- Backup job `ramshield-backup` exited with code 1 at 10:50 UTC; needs investigation.
- Git history polluted with many `[skip ci]` helper‑agent commits; squash recommended before merge.

## Prioritized Tasks
### T1: Fix facts‑collector workspace resolution
- **Target:** `~/.hermes/scripts/facts_collector.py` (fallback workspace line)
- **Action:** Replace the `Path(__file__).parent.parent.parent` fallback with the explicit repo path `'/home/m/vehicle_of_rationalism/ramshield/beta/rs'` (or add `RAMSHIELD_WS` env‑var support).
- **Verify:** Run `python3 -W error .hermes/scripts/facts_collector.py` and confirm `docs/FACTS.json` reports `workspace: /home/m/vehicle_of_rationalism/ramshield/beta/rs` and that the stray `/home/m/docs/FACTS.json` is removed.

### T2: Remove `.unwrap()` from `src/dashboard/mod.rs` (production)
- **Target:** `src/dashboard/mod.rs` – replace the 15 `.unwrap()` calls in request‑handler bodies with `unwrap_or_else`/`Result` propagation or `unwrap_or_default` where appropriate.
- **Verify:** `cargo build --all-targets` succeeds with zero lints (`cargo clippy --all-targets -- -D warnings` passes).

### T3: Remove `.unwrap()` / `.expect()` from `src/dashboard/auth.rs` (production)
- **Target:** `src/dashboard/auth.rs` – replace the three `.unwrap()` calls at lines 145, 184 and the `.expect()` at line 221 (non‑test production path) with safe error handling (e.g., `unwrap_or_else` or propagate `Result`).
- **Verify:** `cargo build --all-targets` succeeds; `cargo clippy` reports no new warnings.

### T4: Commit `docs/PLAN.md` to git so it is tracked
- **Target:** `docs/PLAN.md`
- **Action:** `git add docs/PLAN.md && git commit -m "planner: daily plan 2026-08-31 [skip ci]"`
- **Verify:** `git log --oneline -- docs/PLAN.md` shows the new commit.

### T5: Create minimal `docs/HEALTH_CHECK.md` placeholder
- **Target:** `docs/HEALTH_CHECK.md`
- **Action:** Write a concise markdown file summarising current health‑loop status (OK/Error counts from `CRON_STATUS.md`, last run timestamp) so downstream agents have an artifact instead of a missing file.
- **Verify:** File exists and contains at least the three lines `## Status`, `## Last run`, `## Metrics` with plausible values.

## No Work Needed
- None of the remaining roadmap items (oss‑fuzz integration, protocol fuzz coverage, crash‑free fuzz runs, security audit) can be advanced in a single 15‑minute agent run; they require larger infrastructure or dedicated research cycles.