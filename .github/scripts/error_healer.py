#!/usr/bin/env python3
"""Error Healer — tree-like self-healing scheduler.

Scans docs/ERRORS.md and docs/FACTS.json for active issues, then for each
issue spawns a 3-stage temp job chain:

    1. healer-analyze-{issue_id}   -> docs/HEALER_ANALYSIS_{issue_id}.md
    2. healer-solve-{issue_id}     -> docs/HEALER_SOLUTION_{issue_id}.md
    3. healer-verify-{issue_id}    -> docs/HEALER_STATUS_{issue_id}.md

If verify fails, the next root run will see the still-open issue and start
another cycle (up to MAX_CYCLES). The root job runs via cron with
no_agent=true; the leaf jobs are LLM agents with file/terminal.

Output: docs/HEALER_DISPATCH.md and appends to docs/OPERATOR_LOG.md
"""

import json
import os
import re
import subprocess
import uuid
from datetime import datetime, timezone, timedelta
from pathlib import Path

WORKSPACE = os.environ.get(
    "GITHUB_WORKSPACE",
    str(Path(__file__).resolve().parent.parent.parent)
)

# Time offsets for the 3-stage chain (minutes after root job runs)
STAGE_OFFSETS = {
    "analyze": 2,
    "solve": 12,
    "verify": 22,
}

# Limits to avoid cron spam
MAX_CYCLES = 3
MAX_ISSUES_PER_RUN = 8

# Sections in ERRORS.md that are NOT actual issue lists
IGNORE_SECTIONS = {
    "recovery playbook", "frozen elements", "errors & error-handling"
}


def now():
    return datetime.now(timezone.utc)


def read_json(path):
    p = Path(WORKSPACE) / path
    if not p.exists():
        return {}
    try:
        return json.loads(p.read_text(encoding="utf-8"))
    except Exception:
        return {}


def read_text(path):
    p = Path(WORKSPACE) / path
    if not p.exists():
        return ""
    try:
        return p.read_text(encoding="utf-8", errors="ignore")
    except Exception:
        return ""


def parse_errors_md():
    """Extract active issues from docs/ERRORS.md grouped by category.

    Skips summary/meta sections and the recovery playbook.
    Dashboard section false-positives are collapsed into one issue per
    category to avoid 13 separate chains for a single outdated checker.

    Also skips issues whose HEALER_STATUS file already marks them fixed,
    so stale one-shot healer jobs do not re-spawn endless chains.
    """
    text = read_text("docs/ERRORS.md")
    issues = []
    current_category = None
    category_items = {}

    for line in text.splitlines():
        cat_match = re.match(r"^##\s+(.+?)\s*(?:\(\d+\))?\s*$", line)
        if cat_match:
            current_category = cat_match.group(1).strip().lower()
            continue

        if current_category in IGNORE_SECTIONS:
            continue

        item_match = re.match(r"^-\s+(.+)", line)
        if item_match and current_category:
            body = item_match.group(1).strip()
            if body.startswith(("✅", "🐝", "Source:", "Recovery:")):
                continue
            category_items.setdefault(current_category, []).append(body)

    # Collapse high-volume false-positive categories
    collapse_categories = {"dashboard", "markdown", "facts", "frozen"}
    for cat, items in category_items.items():
        if cat in collapse_categories and len(items) > 1:
            combined = "; ".join(items[:5])
            if len(items) > 5:
                combined += f"; (+{len(items) - 5} more)"
            issue_id = re.sub(r"[^a-zA-Z0-9_-]+", "-", f"{cat}-{combined}")[:50].strip("-")
            if not issue_id:
                issue_id = f"{cat}-{uuid.uuid4().hex[:8]}"
            issues.append({
                "id": issue_id,
                "category": cat,
                "body": combined,
                "source": "ERRORS.md",
                "count": len(items),
            })
        else:
            for body in items:
                issue_id = normalize_issue_id(body, cat)
                issues.append({
                    "id": issue_id,
                    "category": cat,
                    "body": body,
                    "source": "ERRORS.md",
                    "count": 1,
                })

    # Skip issues already marked fixed in a previous healer cycle (case-insensitive)
    filtered = []
    for issue in issues:
        if is_already_fixed(issue["id"]):
            append_operator_log([f"SKIPPED: {issue['id']} already fixed (parse_errors_md)"])
            continue
        filtered.append(issue)

    return filtered


def parse_facts_errors():
    """Surface top-level facts problems as issues."""
    facts = read_json("docs/FACTS.json")
    issues = []
    clippy = facts.get("codebase", {}).get("clippy_warnings", 0)
    if clippy and clippy > 0:
        issues.append({
            "id": "facts-clippy-warnings",
            "category": "facts",
            "body": f"FACTS.json reports {clippy} clippy warning(s)",
            "source": "FACTS.json",
            "count": 1,
        })
    dead = facts.get("dead_links", [])
    if dead:
        combined = "; ".join(dead[:5])
        if len(dead) > 5:
            combined += f"; (+{len(dead) - 5} more)"
        issues.append({
            "id": "facts-dead-links",
            "category": "deadlink",
            "body": f"FACTS.json reports {len(dead)} dead link(s): {combined}",
            "source": "FACTS.json",
            "count": len(dead),
        })
    return issues


def load_cycle_history(issue_id):
    """Count how many times this issue has already been healed.

    Looks for an existing HEALER_STATUS file case-insensitively, because
    normalize_issue_id lowercases IDs while older status files may use
    uppercase issue IDs (e.g. EMPTY-PULSE_LOG...).
    """
    docs_dir = Path(WORKSPACE) / "docs"
    target = issue_id.lower()
    candidates = list(docs_dir.glob("HEALER_STATUS_*.md"))
    for status_file in candidates:
        name = status_file.stem.lower().replace("healer_status_", "")
        if name == target:
            text = status_file.read_text(encoding="utf-8", errors="ignore")
            cycles = re.findall(r"cycle:\s*(\d+)", text, re.I)
            return max([int(c) for c in cycles] + [0])
    return 0


def is_already_fixed(issue_id):
    """Return True if a previous status file marks this issue as fixed.

    Matches case-insensitively against existing HEALER_STATUS_*.md filenames.
    """
    docs_dir = Path(WORKSPACE) / "docs"
    target = issue_id.lower()
    for status_file in docs_dir.glob("HEALER_STATUS_*.md"):
        name = status_file.stem.lower().replace("healer_status_", "")
        if name == target:
            text = status_file.read_text(encoding="utf-8", errors="ignore").lower()
            return (
                "fixed? yes" in text
                or "fixed? true" in text
                or "status: resolved" in text
                or "status: fixed" in text
            )
    return False


def issue_id_safe(issue_id):
    """Ensure safe cron job names."""
    return re.sub(r"[^a-zA-Z0-9_-]+", "-", issue_id).strip("-").lower()[:55]


def normalize_issue_id(body, category):
    """Generate an issue ID from an error body, normalizing stale healer alerts.

    Stale one-shot healer jobs often appear in ERRORS.md as
    `DELAYED: healer-verify-<original-id>`. This function strips common
    prefixes (`DELAYED:`, `STUCK:`, `PAUSED:`, `STALE ONESHOT:`) and healer
    stage prefixes (`healer-analyze-`, `healer-solve-`, `healer-verify-`) so
    that such alerts map back to the original issue and reuse its cycle
    history / fixed status.
    """
    nid = body.lower()
    # Strip status prefixes
    for prefix in ("delayed:", "stuck:", "paused:", "stale oneshot:"):
        if nid.startswith(prefix):
            nid = nid[len(prefix):].strip()
    # Strip healer stage prefixes (may be nested, e.g. healer-verify-delayed-healer-verify-...)
    for _ in range(3):  # bounded nesting
        for stage in ("healer-analyze-", "healer-solve-", "healer-verify-"):
            if nid.startswith(stage):
                nid = nid[len(stage):].strip()
    # Sanitize
    nid = re.sub(r"[^a-zA-Z0-9_-]+", "-", nid)[:50].strip("-")
    if not nid:
        nid = f"{category}-{uuid.uuid4().hex[:8]}"
    return nid


def build_cron_create_cmd(name, schedule, prompt, skills, workdir, repeat=1):
    """Return a shell command that creates a cron job via hermes cron CLI."""
    # Prompt must be the second positional argument, immediately after schedule.
    prompt_escaped = prompt.replace("'", "'\"'\"'")
    skill_args = " ".join(f"--skill {s}" for s in skills)
    cmd = (
        f"hermes cron create '{schedule}' '{prompt_escaped}' "
        f"--name '{name}' {skill_args} "
        f"--workdir '{workdir}' --repeat {repeat} --deliver local"
    )
    return cmd


def schedule_healer_chain(issue, cycle):
    """Create the 3-stage temp cron jobs for one issue."""
    safe_id = issue_id_safe(issue["id"])
    issue_id = issue["id"]
    category = issue["category"]
    body = issue["body"]
    base = now()

    base_prompt = (
        f"You are operating on the RamShield project in {WORKSPACE}.\n"
        f"Issue ID: {issue_id}\n"
        f"Category: {category}\n"
        f"Description: {body}\n"
        f"Cycle: {cycle}\n"
    )

    analyze_name = f"healer-analyze-{safe_id}"
    solve_name = f"healer-solve-{safe_id}"
    verify_name = f"healer-verify-{safe_id}"

    analyze_prompt = (
        base_prompt +
        "\nSTAGE: ANALYZE\n"
        "1. Read docs/ERRORS.md and docs/FACTS.json to understand the issue.\n"
        "2. Examine the relevant files (scripts, docs, workflows, Rust code).\n"
        "3. Write a root-cause analysis to docs/HEALER_ANALYSIS_" + issue_id + ".md\n"
        "4. Keep it concise: problem, evidence, exact files/lines, and proposed fix.\n"
        "5. Do NOT modify any files in this stage.\n"
        "6. Print a one-line summary to stdout."
    )

    solve_prompt = (
        base_prompt +
        "\nSTAGE: SOLVE\n"
        "1. Read docs/HEALER_ANALYSIS_" + issue_id + ".md\n"
        "2. Apply the safest minimal fix using patch/write_file/terminal.\n"
        "3. Prefer script regeneration or small file edits; avoid risky refactors.\n"
        "4. Write a solution log to docs/HEALER_SOLUTION_" + issue_id + ".md\n"
        "5. Document exactly what changed and why.\n"
        "6. Print a one-line summary to stdout."
    )

    verify_prompt = (
        base_prompt +
        "\nSTAGE: VERIFY\n"
        "1. Read docs/HEALER_SOLUTION_" + issue_id + ".md and docs/HEALER_ANALYSIS_" + issue_id + ".md\n"
        "2. Re-run the relevant check to confirm the issue is fixed.\n"
        "   - For dead links: run `python3 .github/scripts/facts_collector.py` and inspect dead_links.\n"
        "   - For cron errors: run `hermes cron run <job_id>` or inspect `hermes cron list`.\n"
        "   - For markdown/dash issues: run `python3 .github/scripts/health_check_repair.py` or `html_dashboard_generator.py`.\n"
        "   - For frozen sources: check file mtime or re-run the collector.\n"
        "3. Write a verification report to docs/HEALER_STATUS_" + issue_id + ".md\n"
        "   Include: cycle number, fixed? yes/no, evidence, and next action.\n"
        "4. If NOT fixed, do NOT reschedule from here; just report it.\n"
        "5. Print a one-line summary to stdout."
    )

    jobs = []
    for stage, offset in STAGE_OFFSETS.items():
        ts = (base + timedelta(minutes=offset)).strftime("%Y-%m-%dT%H:%M:%S+00:00")
        if stage == "analyze":
            name = analyze_name
            prompt = analyze_prompt
        elif stage == "solve":
            name = solve_name
            prompt = solve_prompt
        else:
            name = verify_name
            prompt = verify_prompt

        cmd = build_cron_create_cmd(
            name=name,
            schedule=ts,
            prompt=prompt,
            skills=["autonomous-project-agents"],
            workdir=WORKSPACE,
            repeat=1,
        )
        jobs.append({
            "stage": stage,
            "name": name,
            "schedule": ts,
            "cmd": cmd,
        })

    return jobs


def run_shell(cmd, timeout=60):
    try:
        r = subprocess.run(
            cmd, shell=True, capture_output=True, text=True, timeout=timeout,
        )
        return r.returncode, r.stdout, r.stderr
    except Exception as e:
        return 1, "", str(e)


def ensure_status_file(issue_id):
    """Create a placeholder status file so downstream can append."""
    p = Path(WORKSPACE) / "docs" / f"HEALER_STATUS_{issue_id}.md"
    if not p.exists():
        p.write_text(f"# Healer Status: {issue_id}\n\n_Initialized_\n", encoding="utf-8")


def append_operator_log(lines):
    p = Path(WORKSPACE) / "docs" / "OPERATOR_LOG.md"
    p.parent.mkdir(parents=True, exist_ok=True)
    ts = now().strftime("%Y-%m-%dT%H:%M:%SZ")
    with p.open("a", encoding="utf-8") as f:
        for line in lines:
            f.write(f"{ts} {line}\n")


def main():
    os.chdir(WORKSPACE)
    ts = now().strftime("%Y-%m-%d %H:%M UTC")

    errors_md = parse_errors_md()
    facts_issues = parse_facts_errors()

    # Deduplicate by id
    seen = set()
    issues = []
    for i in errors_md + facts_issues:
        if i["id"] not in seen:
            seen.add(i["id"])
            issues.append(i)

    # Sort by severity-ish (cron/dash first) and cap
    priority = {"cron": 0, "dashboard": 1, "markdown": 2, "facts": 3, "deadlink": 4, "pulse": 5, "blindspot": 6}
    issues.sort(key=lambda x: priority.get(x["category"], 99))
    capped = issues[:MAX_ISSUES_PER_RUN]
    skipped = issues[MAX_ISSUES_PER_RUN:]

    dispatch_lines = [
        f"# Error Healer Dispatch — {ts}",
        "",
        f"Active issues detected: {len(issues)} (capped to {len(capped)} this run)",
        "",
    ]

    if not capped:
        dispatch_lines.append("✅ No active issues. Healer went back to sleep.")
    else:
        dispatch_lines.append("| Issue | Category | Cycle | Jobs scheduled |")
        dispatch_lines.append("|-------|----------|-------|----------------|")

    all_jobs = []
    skipped_fixed = []
    for issue in capped:
        if is_already_fixed(issue["id"]):
            skipped_fixed.append(issue["id"])
            msg = f"SKIPPED: {issue['id']} already fixed"
            append_operator_log([msg])
            continue

        cycle = load_cycle_history(issue["id"]) + 1
        if cycle > MAX_CYCLES:
            msg = f"ESCALATE: {issue['id']} exceeded {MAX_CYCLES} cycles — needs manual review"
            dispatch_lines.append(f"| {issue['id']} | {issue['category']} | {cycle} | ESCALATED |")
            append_operator_log([msg])
            continue

        ensure_status_file(issue["id"])
        chain = schedule_healer_chain(issue, cycle)
        all_jobs.extend(chain)
        for j in chain:
            rc, out, err = run_shell(j["cmd"], timeout=90)
            log = f"scheduled {j['stage']} job={j['name']} rc={rc}"
            if rc != 0:
                log += f" ERR={err[:200]}"
            append_operator_log([log])

        dispatch_lines.append(
            f"| {issue['id']} | {issue['category']} | {cycle} | analyze/solve/verify |"
        )

    if skipped_fixed:
        dispatch_lines.append("")
        dispatch_lines.append(f"Skipped {len(skipped_fixed)} already-fixed issue(s):")
        for s in skipped_fixed:
            dispatch_lines.append(f"- {s}")

    if skipped:
        dispatch_lines.append("")
        dispatch_lines.append(f"Skipped {len(skipped)} lower-priority issues until next run.")
        for s in skipped:
            dispatch_lines.append(f"- {s['id']} ({s['category']})")

    dispatch_lines += [
        "",
        f"**Total temp jobs scheduled:** {len(all_jobs)}",
        "",
        "## Scheduled jobs",
        "",
    ]
    for j in all_jobs:
        dispatch_lines.append(f"- `{j['name']}` at {j['schedule']} ({j['stage']})")

    out_path = Path(WORKSPACE) / "docs" / "HEALER_DISPATCH.md"
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text("\n".join(dispatch_lines), encoding="utf-8")

    summary = f"Healer dispatched {len(all_jobs)} jobs for {len(capped)} issues"
    append_operator_log([summary])
    print(summary)


if __name__ == "__main__":
    main()
