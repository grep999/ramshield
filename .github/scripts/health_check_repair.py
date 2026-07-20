#!/usr/bin/env python3
"""Health Check & Repair Agent for RamShield Dashboard.

Analyzes each dashboard section independently, detects mishaps (dead jobs,
dead links, malformed markdown, stale data), applies safe auto-fixes, and
writes a structured report to docs/HEALTH_CHECK.md.

Designed to run via cron (hourly) or manually.
"""
import os
import re
import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parent.parent.parent


# ── 1. Cron Jobs ──────────────────────────────────────────────────────────

def get_cron_status():
    """Run `hermes cron list` and parse the text output into job dicts."""
    try:
        r = subprocess.run(
            ["hermes", "cron", "list"],
            capture_output=True, text=True, timeout=30,
        )
        raw = r.stdout or r.stderr
        jobs = []
        current = None
        for line in raw.split("\n"):
            # New job block: leading spaces + hex id + [state]
            m = re.match(r"^\s+([a-f0-9]+)\s+\[([^\]]+)\]", line)
            if m:
                current = {
                    "id": m.group(1), "state": m.group(2),
                    "name": "", "schedule": "", "last_status": "never",
                    "next_run": "", "last_run": "",
                }
                jobs.append(current)
                continue
            if current is None:
                continue
            if m := re.match(r"\s*Name:\s+(.+)", line):
                current["name"] = m.group(1).strip()
            elif m := re.match(r"\s*Schedule:\s+(.+)", line):
                current["schedule"] = m.group(1).strip()
            elif m := re.match(r"\s*Next run:\s+(.+)", line):
                current["next_run"] = m.group(1).strip()
            elif m := re.match(r"\s*Last run:\s+(.+?)\s+(\w+)\s*$", line):
                current["last_run"] = m.group(1).strip()
                current["last_status"] = m.group(2).strip().lower()
        return jobs, None
    except Exception as e:
        return [], str(e)


def check_cron_jobs(jobs):
    """Analyze cron job health. Return (issues, fixes_applied)."""
    issues = []
    fixes = []
    now = datetime.now(timezone.utc)

    if not jobs:
        issues.append("No cron jobs found — hermes cron list returned empty.")
        return issues, fixes

    for j in jobs:
        name = j.get("name", "?")
        status = j.get("last_status", "never")
        next_run = j.get("next_run", "")

        # Check for stuck jobs: scheduled but next_run is in the past
        if next_run:
            try:
                nr = datetime.fromisoformat(next_run.replace("Z", "+00:00"))
                if nr < now and status != "ok":
                    hours_overdue = (now - nr).total_seconds() / 3600
                    if hours_overdue > 2:
                        issues.append(
                            f"STUCK: {name} — next_run was {hours_overdue:.1f}h ago, "
                            f"last_status={status}"
                        )
                    elif hours_overdue > 0.5:
                        issues.append(
                            f"DELAYED: {name} — {hours_overdue:.1f}h overdue, "
                            f"last_status={status}"
                        )
            except Exception:
                pass

        # Check for paused jobs
        if j.get("state") == "paused":
            issues.append(f"PAUSED: {name} — job is paused, should it be resumed?")

    return issues, fixes


# ── 2. Dead Links ─────────────────────────────────────────────────────────

def check_dead_links():
    """Scan docs/*.md for broken internal links. Return (unique_issues, fixable_symlinks).

    Key fix: scan docs/ ONCE (not inside os.walk loop). Deduplicate results.
    Skip anchor-only links (#section) — those are false positives.
    """
    issues = []
    symlink_fixes = []
    seen = set()  # dedupe

    # 2a. Broken symlinks anywhere in repo (exclude .git, node_modules, __pycache__)
    for root, dirs, files in os.walk(WORKSPACE):
        dirs[:] = [d for d in dirs if d not in (".git", "node_modules", "__pycache__", "target")]
        for f in files:
            p = Path(root) / f
            try:
                if p.is_symlink() and not p.exists():
                    key = str(p)
                    if key not in seen:
                        seen.add(key)
                        symlink_fixes.append(str(p))
                        issues.append(f"Broken symlink: {p.relative_to(WORKSPACE)}")
            except Exception:
                pass

    # 2b. Broken markdown links in docs/ — scan ONCE
    docs_dir = WORKSPACE / "docs"
    if docs_dir.exists():
        for d in docs_dir.iterdir():
            if not (d.is_file() and d.suffix in (".md", ".markdown")):
                continue
            try:
                content = d.read_text(encoding="utf-8", errors="ignore")
            except Exception:
                continue
            for link in re.findall(r"\[.*?\]\(([^)]+)\)", content):
                if link.startswith(("http", "https", "mailto:")):
                    continue
                if link.startswith("#"):
                    continue  # anchor-only, skip (false positive)
                # Resolve relative to docs/
                # Handle both "docs/X" and plain "X" paths
                link_clean = link.split("#")[0]
                if link_clean.startswith("docs/"):
                    abs_target = (WORKSPACE / link_clean).resolve()
                else:
                    abs_target = (d.parent / link_clean).resolve()
                if not abs_target.exists():
                    key = f"{d.name}:{link}"
                    if key not in seen:
                        seen.add(key)
                        issues.append(f"Dead link in {d.name}: {link}")
    return issues, symlink_fixes


def fix_dead_symlinks(symlink_paths):
    """Remove broken symlinks. Return list of removed paths."""
    fixed = []
    for s in symlink_paths:
        p = Path(s)
        if p.is_symlink():
            try:
                p.unlink()
                fixed.append(str(p.relative_to(WORKSPACE)))
            except Exception:
                pass
    return fixed


# ── 3. Markdown Structure ─────────────────────────────────────────────────

def check_markdown_tables():
    """Verify critical markdown files exist and have expected structure."""
    issues = []
    checks = [
        ("docs/CRON_STATUS.md", "| :--- |", "table alignment row"),
        ("docs/BACKLOG.md", r"^\s*\d+\.\s+\[ \]", "numbered checklist items"),
        ("docs/PLAN.md", "### T1", "T1 task section"),
        ("docs/ROADMAP.md", "### Q", "quarterly sections"),
        ("docs/FACTS.json", '"generated_at"', "generated_at timestamp"),
    ]
    for fpath, pattern, desc in checks:
        p = WORKSPACE / fpath
        if not p.exists():
            issues.append(f"MISSING: {fpath}")
            continue
        content = p.read_text(encoding="utf-8", errors="ignore")
        if pattern.startswith("|") or pattern.startswith('"'):
            # literal match
            if pattern not in content:
                issues.append(f"MALFORMED: {fpath} missing {desc}")
        else:
            # regex match
            if not re.search(pattern, content, re.M):
                issues.append(f"MALFORMED: {fpath} missing {desc}")
    return issues


def repair_missing_docs():
    """Regenerate missing docs by running collectors."""
    fixes = []
    scripts = {
        "docs/CRON_STATUS.md": ".github/scripts/cron_status_collector.py",
        "docs/FACTS.json": ".github/scripts/facts_collector.py",
    }
    for doc, script in scripts.items():
        if not (WORKSPACE / doc).exists():
            sp = WORKSPACE / script
            if sp.exists():
                try:
                    subprocess.run(
                        ["python3", str(sp)],
                        cwd=str(WORKSPACE), timeout=30,
                        capture_output=True,
                    )
                    fixes.append(f"Regenerated {doc} via {script}")
                except Exception as e:
                    fixes.append(f"Failed to regenerate {doc}: {e}")
    return fixes


# ── 4. Dashboard Sections ──────────────────────────────────────────────────

def check_dashboard_sections():
    """Verify the generated HTML dashboard has all expected sections."""
    issues = []
    html_path = WORKSPACE / "docs" / "AUTOMATION_DASHBOARD.html"
    if not html_path.exists():
        issues.append("MISSING: docs/AUTOMATION_DASHBOARD.html — run html_dashboard_generator.py")
        return issues
    content = html_path.read_text(encoding="utf-8", errors="ignore")
    required = [
        ("Main Timeline", "📅 Main Timeline"),
        ("Priority Alignment", "🎯 Priority Alignment"),
        ("Cycle Progress", "📊 Cycle Progress"),
        ("Cron Jobs", "⏰ Cron Jobs"),
        ("Atomic Backlog", "📦 Atomic Backlog"),
        ("Pulse", "💓 Pulse"),
        ("Promotion", "📣 Promotion"),
        ("Research", "🔬 Research"),
        ("Health Loop", "🏥 Health Loop"),
        ("Dead Links Report", "🔗 Dead Links Report"),
        ("Daily Work Plan", "📋 Daily Work Plan"),
        ("Worker Status", "👷 Worker Status"),
        ("Review & Assessment", "🔍 Review & Assessment"),
    ]
    for name, marker in required:
        if marker not in content:
            issues.append(f"MISSING SECTION: {name} (marker '{marker}' not found)")
    return issues


def regenerate_dashboard():
    """Regenerate the HTML dashboard."""
    fixes = []
    gen = WORKSPACE / ".github" / "scripts" / "html_dashboard_generator.py"
    if gen.exists():
        try:
            r = subprocess.run(
                ["python3", str(gen)],
                cwd=str(WORKSPACE), timeout=30,
                capture_output=True, text=True,
            )
            if r.returncode == 0:
                fixes.append("Regenerated AUTOMATION_DASHBOARD.html")
            else:
                fixes.append(f"Dashboard regeneration failed: {r.stderr[:200]}")
        except Exception as e:
            fixes.append(f"Dashboard regeneration error: {e}")
    return fixes


# ── 5. FACTS.json Health ───────────────────────────────────────────────────

def check_facts_json():
    """Validate FACTS.json structure and freshness."""
    issues = []
    fp = WORKSPACE / "docs" / "FACTS.json"
    if not fp.exists():
        issues.append("MISSING: docs/FACTS.json")
        return issues
    try:
        facts = json.loads(fp.read_text())
    except Exception as e:
        issues.append(f"MALFORMED: FACTS.json JSON parse error: {e}")
        return issues

    required_keys = ["generated_at", "codebase", "roadmap_open_tasks", "backlog_remaining"]
    for k in required_keys:
        if k not in facts:
            issues.append(f"MISSING KEY: FACTS.json missing '{k}'")

    # Check freshness: if older than 2 hours, flag stale
    gen_at = facts.get("generated_at", "")
    if gen_at:
        try:
            gen_dt = datetime.fromisoformat(gen_at.replace("Z", "+00:00"))
            age_h = (datetime.now(timezone.utc) - gen_dt).total_seconds() / 3600
            if age_h > 2:
                issues.append(f"STALE: FACTS.json is {age_h:.1f}h old (regenerate)")
        except Exception:
            issues.append("STALE: FACTS.json has invalid generated_at timestamp")

    return issues


def regenerate_facts():
    """Regenerate FACTS.json."""
    fixes = []
    collector = WORKSPACE / ".github" / "scripts" / "facts_collector.py"
    if collector.exists():
        try:
            r = subprocess.run(
                ["python3", str(collector)],
                cwd=str(WORKSPACE), timeout=30,
                capture_output=True, text=True,
            )
            if r.returncode == 0:
                fixes.append("Regenerated FACTS.json")
            else:
                fixes.append(f"FACTS regeneration failed: {r.stderr[:200]}")
        except Exception as e:
            fixes.append(f"FACTS regeneration error: {e}")
    return fixes


# ── 6. Backlog Health ──────────────────────────────────────────────────────

def check_backlog():
    """Check backlog file exists, has items, and has remaining work."""
    issues = []
    bp = WORKSPACE / "docs" / "BACKLOG.md"
    if not bp.exists():
        issues.append("MISSING: docs/BACKLOG.md")
        return issues
    content = bp.read_text(encoding="utf-8", errors="ignore")
    unchecked = len(re.findall(r"\[ \]", content))
    checked = len(re.findall(r"\[x\]", content, re.I))
    if unchecked == 0 and checked == 0:
        issues.append("EMPTY: BACKLOG.md has no checklist items")
    elif unchecked == 0:
        issues.append("EXHAUSTED: All 50 backlog items checked — need new batch")
    elif unchecked < 10:
        issues.append(f"LOW: Only {unchecked} backlog items remaining — consider replenishing")
    return issues


# ── 7. Pulse Log Health ────────────────────────────────────────────────────

def check_pulse_log():
    """Check if pulse log is fresh and has recent entries."""
    issues = []
    pp = WORKSPACE / "docs" / "PULSE_LOG.md"
    if not pp.exists():
        issues.append("MISSING: docs/PULSE_LOG.md — pulse agent hasn't run yet")
        return issues
    content = pp.read_text(encoding="utf-8", errors="ignore")
    if "No pulse" in content or len(content.strip()) < 50:
        issues.append("EMPTY: PULSE_LOG.md is empty — pulse agent hasn't produced output")
    return issues


# ── 7b. Frozen / Blind-Spot Element Detection ──────────────────────────────

# Dashboard sources and their staleness thresholds (minutes)
FROZEN_SOURCES = {
    "docs/CRON_STATUS.md": 10,
    "docs/FACTS.json": 90,
    "docs/PROMOTION_LOG.md": 60,
    "docs/RESEARCH.md": 90,
    "docs/PULSE_LOG.md": 30,
    "docs/HEALTH_LOOP.md": 30,
    "docs/PROMOTION/REVIEW.md": 60,
    "docs/AUTOMATION_DASHBOARD.html": 30,
}


def check_frozen_elements():
    """Find dashboard source files that haven't been updated recently.

    A "frozen" element is one whose mtime is older than its staleness
    threshold — these are blind spots because the dashboard silently serves
    stale data. Detection makes them visible so the health-repair job or a
    human can refresh them.
    """
    issues = []
    now = datetime.now(timezone.utc).timestamp()
    for rel, threshold_min in FROZEN_SOURCES.items():
        p = WORKSPACE / rel
        if not p.exists():
            # Missing is also a frozen element
            issues.append(f"MISSING: {rel}")
            continue
        age_min = (now - p.stat().st_mtime) / 60
        if age_min > threshold_min:
            issues.append(
                f"FROZEN: {rel} not updated in {age_min:.0f}m (threshold {threshold_min}m)"
            )
    return issues


def handle_frozen_elements(frozen):
    """Auto-refresh the most critical frozen sources.

    Only refreshes known-mutable collectors. Static hand-written docs (PLAN.md,
    ROADMAP.md) are left alone — they shouldn't be force-regenerated.
    """
    issues = []
    fixes = []
    refresh_map = {
        "docs/CRON_STATUS.md": ".github/scripts/cron_status_collector.py",
        "docs/FACTS.json": ".github/scripts/facts_collector.py",
        "docs/AUTOMATION_DASHBOARD.html": ".github/scripts/html_dashboard_generator.py",
    }
    for line in frozen:
        src = line.split(": ", 1)[-1].split(" ")[0]
        if src in refresh_map and (WORKSPACE / refresh_map[src]).exists():
            try:
                r = subprocess.run(
                    ["python3", refresh_map[src]],
                    cwd=str(WORKSPACE), timeout=30,
                    capture_output=True, text=True,
                )
                if r.returncode == 0:
                    fixes.append(f"Force-refreshed {src}")
                else:
                    issues.append(f"REFRESH FAILED: {src}")
            except Exception as e:
                issues.append(f"REFRESH ERROR: {src} ({e})")
        # Else: same as issues — surface to Hermes for triage
    return issues, fixes


# ── 7c. Errors & Error-Handling Report ────────────────────────────────────

def write_errors_report(all_issues, frozen):
    """Aggregate every detected error into docs/ERRORS.md for the dashboard."""
    err_path = WORKSPACE / "docs" / "ERRORS.md"
    err_path.parent.mkdir(parents=True, exist_ok=True)
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    grouped = {}
    for msg, cat in all_issues:
        grouped.setdefault(cat, []).append(msg)
    lines = [f"# Errors & Error-Handling — {ts}", ""]
    if not grouped and not frozen:
        lines.append("✅ **0 active errors** — pipeline healthy.")
    else:
        lines.append(f"⚠️ **{sum(len(v) for v in grouped.values())} active errors**, "
                     f"{len(frozen)} frozen element(s)")
    for cat, msgs in grouped.items():
        lines.append(f"## {cat.title()} ({len(msgs)})")
        for m in msgs:
            lines.append(f"- {m}")
        lines.append("")
    if frozen:
        lines.append(f"## Frozen Elements ({len(frozen)})")
        for f in frozen:
            lines.append(f"- {f}")
        lines.append("")
    lines.append("## Recovery Playbook")
    lines.append("- See `docs/GIT_AUTOMATION_MANUAL.md` for known patterns")
    lines.append("- Auto-fixers: symlink removal, FACTS.json regen, cron status refresh")
    lines.append(f"- Generated by `health_check_repair.py` every health-repair tick")
    err_path.write_text("\n".join(lines), encoding="utf-8")


# ── Main ──────────────────────────────────────────────────────────────────

def main():
    os.chdir(WORKSPACE)
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    log_path = WORKSPACE / "docs" / "HEALTH_CHECK.md"
    log_path.parent.mkdir(parents=True, exist_ok=True)

    lines = [f"# Health Check — {ts}", ""]
    total_issues = 0
    total_fixes = 0

    # ── 1. Cron Jobs ──
    lines.append("## 1. Cron Jobs")
    jobs, err = get_cron_status()
    if err:
        lines.append(f"❌ Error: {err}")
        total_issues += 1
    else:
        lines.append(f"Found {len(jobs)} scheduled jobs.")
        ran_ok = sum(1 for j in jobs if j["last_status"] == "ok")
        never_ran = sum(1 for j in jobs if j["last_status"] == "never")
        lines.append(f"- Ran successfully: {ran_ok}")
        lines.append(f"- Never ran: {never_ran}")
        issues, fixes = check_cron_jobs(jobs)
        total_issues += len(issues)
        total_fixes += len(fixes)
        for i in issues:
            lines.append(f"  - ⚠️ {i}")
        for f in fixes:
            lines.append(f"  - 🔧 {f}")
        if not issues:
            lines.append("  ✅ All jobs healthy.")
    lines.append("")

    # ── 2. Dead Links ──
    lines.append("## 2. Dead Links")
    dead_issues, symlink_paths = check_dead_links()
    total_issues += len(dead_issues)
    if not dead_issues:
        lines.append("✅ No dead links found.")
    else:
        lines.append(f"Found {len(dead_issues)} unique dead link(s):")
        for i in dead_issues:
            lines.append(f"  - 🔗 {i}")
        # Auto-fix broken symlinks
        fixed = fix_dead_symlinks(symlink_paths)
        if fixed:
            total_fixes += len(fixed)
            lines.append(f"  - 🔧 Removed {len(fixed)} broken symlink(s): {', '.join(fixed)}")
    lines.append("")

    # ── 3. Markdown Structure ──
    lines.append("## 3. Markdown Structure")
    md_issues = check_markdown_tables()
    total_issues += len(md_issues)
    if not md_issues:
        lines.append("✅ All critical markdown files OK.")
    else:
        for i in md_issues:
            lines.append(f"  - ⚠️ {i}")
        # Auto-repair missing docs
        repaired = repair_missing_docs()
        total_fixes += len(repaired)
        for r in repaired:
            lines.append(f"  - 🔧 {r}")
    lines.append("")

    # ── 4. Dashboard Sections ──
    lines.append("## 4. Dashboard Sections")
    dash_issues = check_dashboard_sections()
    total_issues += len(dash_issues)
    if not dash_issues:
        lines.append("✅ All 13 dashboard sections present.")
    else:
        for i in dash_issues:
            lines.append(f"  - ⚠️ {i}")
        # Auto-regenerate
        regenerated = regenerate_dashboard()
        total_fixes += len(regenerated)
        for r in regenerated:
            lines.append(f"  - 🔧 {r}")
    lines.append("")

    # ── 5. FACTS.json ──
    lines.append("## 5. FACTS.json Health")
    facts_issues = check_facts_json()
    total_issues += len(facts_issues)
    if not facts_issues:
        lines.append("✅ FACTS.json valid and fresh.")
    else:
        for i in facts_issues:
            lines.append(f"  - ⚠️ {i}")
        # Auto-regenerate if stale or missing
        if any("STALE" in i or "MISSING" in i or "MALFORMED" in i for i in facts_issues):
            regenerated = regenerate_facts()
            total_fixes += len(regenerated)
            for r in regenerated:
                lines.append(f"  - 🔧 {r}")
    lines.append("")

    # ── 6. Backlog ──
    lines.append("## 6. Backlog Health")
    backlog_issues = check_backlog()
    total_issues += len(backlog_issues)
    if not backlog_issues:
        lines.append("✅ Backlog healthy.")
    else:
        for i in backlog_issues:
            lines.append(f"  - ⚠️ {i}")
    lines.append("")

    # ── 7. Pulse Log ──
    lines.append("## 7. Pulse Log")
    pulse_issues = check_pulse_log()
    total_issues += len(pulse_issues)
    if not pulse_issues:
        lines.append("✅ Pulse log present.")
    else:
        for i in pulse_issues:
            lines.append(f"  - ⚠️ {i}")
    lines.append("")

    # ── 8. Frozen/Blind-Spot Detection (dashboard element health) ──
    lines.append("## 8. Frozen / Blind-Spot Detection")
    frozen = check_frozen_elements()
    blindspot_issues, blindspot_fixes = handle_frozen_elements(frozen)
    total_issues += len(blindspot_issues)
    total_fixes += len(blindspot_fixes)
    if not frozen:
        lines.append("✅ All dashboard sources fresh.")
    else:
        lines.append(f"Detected {len(frozen)} stale/frozen element(s):")
        for f in frozen:
            lines.append(f"  - ❄️ {f}")
        for fix in blindspot_fixes:
            lines.append(f"  - 🔧 {fix}")
    lines.append("")

    # ── 9. Errors & Error-Handling (write to ERRORS.md for dashboard) ──
    cron_issues = issues if 'issues' in locals() else []
    all_issues = (
        [(i, "cron") for i in cron_issues] +
        [(i, "deadlink") for i in dead_issues] +
        [(i, "markdown") for i in md_issues] +
        [(i, "dashboard") for i in dash_issues] +
        [(i, "facts") for i in facts_issues] +
        [(i, "backlog") for i in backlog_issues] +
        [(i, "pulse") for i in pulse_issues] +
        [(i, "blindspot") for i in blindspot_issues]
    )
    write_errors_report(all_issues, frozen)
    lines.append("## 9. Errors & Error-Handling")
    active = sum(1 for _, _ in all_issues)
    lines.append(f"- Active errors tracked: **{active}**")
    lines.append("- Source: `docs/ERRORS.md` (visible on dashboard)")
    lines.append("- Recovery: see `docs/GIT_AUTOMATION_MANUAL.md`")
    lines.append("")

    # ── Summary ──
    lines.append("---")
    lines.append(f"## Summary")
    lines.append(f"- Issues found: {total_issues}")
    lines.append(f"- Fixes applied: {total_fixes}")
    status = "✅ HEALTHY" if total_issues == 0 else ("⚠️ ISSUES" if total_issues < 5 else "❌ CRITICAL")
    lines.append(f"- Status: {status}")

    log_path.write_text("\n".join(lines), encoding="utf-8")
    print(f"Health check complete: {total_issues} issues, {total_fixes} fixes → {log_path}")


if __name__ == "__main__":
    main()
