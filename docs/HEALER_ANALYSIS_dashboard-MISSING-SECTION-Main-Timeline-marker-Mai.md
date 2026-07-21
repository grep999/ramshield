# Healer Analysis: dashboard-MISSING-SECTION-Main-Timeline-marker-Mai

**Issue:** Multiple dashboard sections reported missing by `health_check_repair.py`.
**Category:** Dashboard
**Cycle:** 1

## Problem

`check_dashboard_sections()` in `.github/scripts/health_check_repair.py` (lines 225-251) validates `docs/AUTOMATION_DASHBOARD.html` against a hard-coded list of 13 section markers. These markers no longer exist in the generated HTML because the dashboard was redesigned. The validation list predates the current generator layout.

## Evidence

- **Checker markers (legacy):**
  - `📅 Main Timeline`
  - `🎯 Priority Alignment`
  - `📊 Cycle Progress`
  - `⏰ Cron Jobs`
  - `📦 Atomic Backlog`
  - `💓 Pulse`
  - `📣 Promotion`
  - `🔬 Research`
  - `🏥 Health Loop`
  - `🔗 Dead Links Report`
  - `📋 Daily Work Plan`
  - `👷 Worker Status`
  - `🔍 Review & Assessment`

- **Current `html_dashboard_generator.py` sections (lines 657-779):**
  - `Operator Console` header
  - `Operator Log`
  - `Autonomous Pipeline` (job chain)
  - `Cron Fleet Status`
  - `Module Consoles` (Error Healer, Facts Collector, Daily Plan, Worker Status, Review, Health Loop, Promotion, Research, Pulse)
  - `Project Health` / `Error Ledger` / `Backlog` / `Control Center`
  - `Roadmap` / `Dependency Audit`
  - `Cron Scaling & Recommendations` / `Self-Healing Ledger`

- The current generator intentionally groups legacy concepts differently (e.g., cron is "Cron Fleet Status", plan/worker/review are cards under "Module Consoles", health is split across cards). None of the 13 legacy emoji markers are emitted.

- `docs/ERRORS.md` shows 13 `MISSING SECTION` errors plus frozen `AUTOMATION_DASHBOARD.html` because the dashboard is stale relative to the checker.

## Root Cause

The health checker and the dashboard generator have diverged. The checker is enforcing a previous dashboard layout. The dashboard itself is generated successfully but fails post-generation validation because the validation targets old headings.

This is exactly the pitfall documented in the `autonomous-project-agents` skill: *"Dashboard checker flags missing sections after generator redesign. Update `check_dashboard_sections()` to match the current generator output rather than adding legacy markers back."*

## Affected Files / Lines

- `.github/scripts/health_check_repair.py`
  - `check_dashboard_sections()`: lines 225-251
  - `required` list: lines 233-247
- `.github/scripts/html_dashboard_generator.py`
  - Current sections: lines 657-779 (no changes needed)
- `docs/ERRORS.md`: generated from the mismatch

## Proposed Fix

1. Update `check_dashboard_sections()` in `.github/scripts/health_check_repair.py` to match the current dashboard output.
2. Use markers that the generator actually emits, e.g.:
   - `Operator Log`
   - `Autonomous Pipeline`
   - `Cron Fleet Status`
   - `Module Consoles`
   - `Project Health`
   - `Error Ledger`
   - `Backlog`
   - `Control Center`
   - `Roadmap`
   - `Dependency Audit`
   - `Cron Scaling & Recommendations`
   - `Self-Healing Ledger`
3. Regenerate `docs/AUTOMATION_DASHBOARD.html` and re-run `health_check_repair.py` to confirm zero dashboard section errors.
4. Do not add the legacy emoji markers back to the generator, as that would reintroduce the old layout.
