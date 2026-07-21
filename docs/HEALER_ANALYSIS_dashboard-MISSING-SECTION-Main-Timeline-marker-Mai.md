# Healer Analysis — dashboard-MISSING-SECTION-Main-Timeline-marker-Mai

**Issue ID:** dashboard-MISSING-SECTION-Main-Timeline-marker-Mai  
**Category:** dashboard  
**Cycle:** 1  
**Date:** 2026-07-21 06:08 UTC  

## Problem

`health_check_repair.py` reports 13 "MISSING SECTION" dashboard errors because the generated `docs/AUTOMATION_DASHBOARD.html` does not contain the exact emoji-marked section headers it expects. The dashboard itself renders correctly and is newer than the checker’s expected markers, so this is a false-positive mismatch between the checker’s expectations and the actual dashboard generator output.

## Evidence

1. **Checker expectations** (`docs/HEALTH_CHECK.md` §4 / `.github/scripts/health_check_repair.py` lines 233-246) look for literal markers:
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

2. **Generator output** (`docs/AUTOMATION_DASHBOARD.html`) has no occurrences of these markers.  
   Confirmed by `search_files` grep for each marker emoji across the repo (only found in `README.md`, `docs/ERRORS.md`, `docs/HEALTH_DASHBOARD.md`, `docs/HEALTH_CHECK.md`).

3. **Generator sections actually present** (`.github/scripts/html_dashboard_generator.py` lines 662-779):
   - `Operator Log`
   - `Autonomous Pipeline` (job chain)
   - `Operations` / `Cron Fleet Status`
   - `Module Consoles` (Facts Collector, Daily Plan, Worker Status, Review, Health Loop, Promotion, Research, Pulse, Error Healer)
   - `Health & Backlog` (Project Health, Error Ledger, Backlog, Control Center)
   - `Growth & Discovery` (Roadmap, Dependency Audit)
   - `Systems Engineering` (Cron Scaling & Recommendations, Self-Healing Ledger)

   The concepts from the checker list are covered, but under different labels and without the required emoji markers.

4. **Stale output**:
   - `docs/AUTOMATION_DASHBOARD.html` refreshed at `2026-07-21 06:07 UTC` (line 223), but `docs/HEALTH_CHECK.md` still generated at `2026-07-21 05:58 UTC`.
   - `docs/CRON_STATUS.md` and `docs/FACTS.json` are flagged frozen (163–170 min old), indicating the facts collector still fails, so the dashboard generator is running on old data and the checker is running against old outputs.

## Affected Files / Lines

| File | Relevance |
|------|-----------|
| `.github/scripts/health_check_repair.py` | Lines 233-246 define the outdated required-marker list; line 248-250 raises errors when markers are absent. |
| `.github/scripts/html_dashboard_generator.py` | Lines 662-779 render the actual dashboard sections with different labels/structure. |
| `docs/AUTOMATION_DASHBOARD.html` | Generated file; missing the expected markers. |
| `docs/ERRORS.md` | Mirrors the checker output (lines 9-21). |
| `docs/HEALTH_CHECK.md` | Same dashboard-section findings (lines 17-29). |

## Root Cause

The health-checker’s required-section list predates the current production-grade operator console redesign (commit `3fd5f33` "feat(dashboard): operator log stream + actionable scaling commands").  
Two valid fixes exist:

1. **Preferred:** Update the checker in `health_check_repair.py` to align with the new dashboard structure — i.e., replace the legacy emoji-marker list with the actual sections/IDs now rendered (`Operator Log`, `Autonomous Pipeline`, `Cron Fleet Status`, `Module Consoles`, `Health & Backlog`, `Growth & Discovery`, `Systems Engineering`).
2. **Alternative:** Update `html_dashboard_generator.py` to emit the legacy emoji markers, either as visible section headings or hidden HTML comments, so the existing checker passes without changing semantics.

The dashboard is functionally complete; the reported errors are structural false positives.

## Proposed Fix

Apply option 1: update `health_check_repair.py` `check_dashboard_sections()` to validate the current dashboard layout.  
Concretely:

- Replace the `required` list (lines 233-247) with markers/IDs that exist in the generated HTML, e.g.:
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
- Re-run `html_dashboard_generator.py` to ensure `docs/AUTOMATION_DASHBOARD.html` is fresh.
- Re-run `health_check_repair.py` and confirm `docs/HEALTH_CHECK.md` §4 reports no missing sections and `docs/ERRORS.md` dashboard entries are gone.

## One-line Summary

Dashboard section errors are false positives: `health_check_repair.py` expects legacy emoji-marked headings that the current operator console no longer emits; update the checker to match the new dashboard structure.
