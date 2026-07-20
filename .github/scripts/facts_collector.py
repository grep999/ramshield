#!/usr/bin/env python3
"""Facts Collector — single source of truth for RamShield automation pipeline.

Gathers all project state into a structured JSON file. No LLM, pure data.
Runs via cronjob with no_agent=true. Downstream LLM agents read this file.
Output: docs/FACTS.json
"""

import json
import os
import re
import subprocess
from datetime import datetime, timezone
from pathlib import Path

WORKSPACE = os.environ.get(
    'GITHUB_WORKSPACE',
    str(Path(__file__).resolve().parent.parent.parent)
)


def git_log(since="24 hours ago"):
    try:
        out = subprocess.check_output(
            ["git", "log", f"--since={since}", "--oneline", "--no-merges"],
            cwd=WORKSPACE, text=True, stderr=subprocess.DEVNULL
        )
        commits = [l.strip() for l in out.splitlines() if l.strip()]
        return {"count": len(commits), "commits": commits[:20]}
    except Exception:
        return {"count": 0, "commits": [], "error": "git unavailable"}


def git_branch():
    try:
        out = subprocess.check_output(
            ["git", "branch", "--show-current"],
            cwd=WORKSPACE, text=True, stderr=subprocess.DEVNULL
        )
        return out.strip()
    except Exception:
        return "unknown"


def git_commit_short():
    try:
        out = subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"],
            cwd=WORKSPACE, text=True, stderr=subprocess.DEVNULL
        )
        return out.strip()
    except Exception:
        return "unknown"


def count_rust_files():
    src = Path(WORKSPACE) / "src"
    if not src.exists():
        return 0
    return len(list(src.rglob("*.rs")))


def count_lines():
    src = Path(WORKSPACE) / "src"
    if not src.exists():
        return 0
    total = 0
    for f in src.rglob("*.rs"):
        try:
            total += len(f.read_text(encoding="utf-8").splitlines())
        except Exception:
            pass
    return total


def count_clippy_warnings():
    try:
        out = subprocess.check_output(
            ["cargo", "clippy", "--all-targets", "--message-format=json"],
            cwd=WORKSPACE, text=True, stderr=subprocess.DEVNULL, timeout=120
        )
        warnings = 0
        for line in out.splitlines():
            try:
                msg = json.loads(line)
                if msg.get("reason") == "compiler-message" and msg.get("message", {}).get("level") == "warning":
                    warnings += 1
            except json.JSONDecodeError:
                pass
        return warnings
    except Exception:
        return -1  # unavailable


def count_todos():
    pattern = re.compile(r'(TODO|FIXME|HACK|XXX)[(:]?\s*(.*)', re.IGNORECASE)
    todos = []
    src = Path(WORKSPACE) / "src"
    for f in src.rglob("*.rs"):
        try:
            for i, line in enumerate(f.read_text(encoding="utf-8").splitlines(), 1):
                m = pattern.search(line)
                if m:
                    todos.append({
                        "kind": m.group(1).upper(),
                        "file": str(f.relative_to(WORKSPACE)),
                        "line": i,
                        "text": m.group(2).strip() or "(no description)"
                    })
        except Exception:
            pass
    return todos


def parse_cargo_deps():
    cargo = Path(WORKSPACE) / "Cargo.toml"
    if not cargo.exists():
        return []
    deps = []
    in_section = False
    for line in cargo.read_text(encoding="utf-8").splitlines():
        if line.strip() == "[dependencies]":
            in_section = True
            continue
        if in_section and line.strip().startswith("["):
            in_section = False
        if in_section and "=" in line and not line.strip().startswith("#"):
            m = re.match(r'^([\w-]+)\s*=\s*"([^"]+)"', line.strip())
            if m:
                deps.append({"name": m.group(1), "version": m.group(2)})
    return deps


def read_roadmap_tasks():
    path = Path(WORKSPACE) / "docs" / "ROADMAP.md"
    if not path.exists():
        return []
    tasks = []
    content = path.read_text(encoding="utf-8")
    for m in re.finditer(r'- \[ \]\s*(.+)', content):
        tasks.append(m.group(1).strip())
    return tasks


def read_report_summaries():
    """Extract key numbers from existing reports."""
    summaries = {}
    for name in ["AGENT_REPORT.md", "HEALTH_DASHBOARD.md", "CONTROL_CENTER.md"]:
        path = Path(WORKSPACE) / "docs" / name
        if path.exists():
            summaries[name] = path.read_text(encoding="utf-8")[:2000]  # first 2KB
    return summaries



def parse_review_status(review_md: str):
    """Parses REVIEW.md and extracts task statuses."""
    statuses = {}
    if review_md.startswith("_Report not found"):
        return statuses
    
    table_re = re.compile(r'\|\s*Task\s*\|\s*Status\s*\|\s*Evidence\s*\|\s*Notes\s*\|\n\|---+\|---+\|---+\|---+\|\n((?:\|[^|]*\|[^|]*\|[^|]*\|[^|]*\|\n)+)')
    match = table_re.search(review_md)
    
    if match:
        for line in match.group(1).strip().split('\n'):
            parts = [p.strip() for p in line.split('|') if p.strip()]
            if len(parts) >= 2: # At least Task and Status
                task_id = parts[0]
                status = parts[1].lower()
                statuses[task_id] = status
    return statuses

def check_local_links():
    """Checks for dead local Markdown links in the docs/ directory."""
    dead_links = []
    docs_path = Path(WORKSPACE) / "docs"
    
    if not docs_path.exists():
        return dead_links
        
    all_docs = {str(p.relative_to(docs_path)) for p in docs_path.rglob("*.md") if p.is_file()}

    for doc_file in docs_path.rglob("*.md"):
        content = doc_file.read_text(encoding="utf-8", errors="ignore")
        for match in re.finditer(r'\[.*?\]\((?!https?://)(.*?)(?:#.*?|\))\)', content):
            link_target = match.group(1).split('#')[0] # Remove anchors
            if link_target.startswith('/'): # Absolute path
                # Treat as relative to workspace, then relative to docs for check
                abs_target_path = (Path(WORKSPACE) / link_target).resolve()
                if abs_target_path.exists():
                    try:
                        relative_to_docs = str(abs_target_path.relative_to(docs_path))
                        if relative_to_docs not in all_docs:
                            # Might be a directory or non-.md file
                            if not abs_target_path.is_file():
                                dead_links.append(f"{doc_file.name}: Broken link to non-file '{link_target}'")
                    except ValueError: # Link points outside docs/
                         if not abs_target_path.exists():
                             dead_links.append(f"{doc_file.name}: Broken external relative link '{link_target}'")
                else:
                    dead_links.append(f"{doc_file.name}: Broken link to absolute path '{link_target}'")
            else: # Relative path
                target_path = (doc_file.parent / link_target).resolve()
                try:
                    relative_to_docs = str(target_path.relative_to(docs_path))
                    if relative_to_docs not in all_docs:
                        if not target_path.is_file(): # Check if it's a file
                            dead_links.append(f"{doc_file.name}: Broken link to '{link_target}'")
                except ValueError: # Link points outside docs/, but relative
                    if not target_path.exists():
                        dead_links.append(f"{doc_file.name}: Broken relative link outside docs '{link_target}'")
            
    return dead_links

def load_plan_review_worker_link():
    """Load existing plan/review/worker status/link report if any."""
    result = {}
    # Full content for BACKLOG.md (needed for counting)
    for name in ["PLAN.md", "REVIEW.md", "WORKER_STATUS.md", "LINK_REPORT.md", "CRON_STATUS.md", "PROMOTION_LOG.md", "RESEARCH.md", "PULSE_LOG.md", "HEALTH_LOOP.md"]:
        path = Path(WORKSPACE) / "docs" / name
        if path.exists():
            result[name] = path.read_text(encoding="utf-8")[:3000]
        else:
            result[name] = f"_Report not found: {name}_"
    # Backlog needs full content for accurate counting
    backlog_path = Path(WORKSPACE) / "docs" / "BACKLOG.md"
    if backlog_path.exists():
        result["BACKLOG.md"] = backlog_path.read_text(encoding="utf-8")
    else:
        result["BACKLOG.md"] = "_Report not found: BACKLOG.md_"
    return result

def count_backlog_remaining(backlog_md: str):
    """Count [ ] items remaining in BACKLOG.md (numbered list format)."""
    if backlog_md.startswith("_Report not found"):
        return None
    # Match "N. [ ]" or "- [ ]" or "* [ ]"
    import re
    return len(re.findall(r'(?:^|\n)\s*(?:\d+\.|[-*])\s+\[ \]', backlog_md))

def main():
    os.chdir(WORKSPACE)
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

    # Load reports needed for facts
    plan_review_worker_link = load_plan_review_worker_link()
    review_statuses = parse_review_status(plan_review_worker_link["REVIEW.md"])
    backlog_remaining = count_backlog_remaining(plan_review_worker_link["BACKLOG.md"])

    facts = {
        "generated_at": timestamp,
        "workspace": WORKSPACE,
        "git": {
            "branch": git_branch(),
            "commit_short": git_commit_short(),
            "recent_commits": git_log("48 hours ago"),
        },
        "codebase": {
            "rust_files": count_rust_files(),
            "lines_of_code": count_lines(),
            "clippy_warnings": count_clippy_warnings(),
        },
        "todos": count_todos(),
        "dependencies": parse_cargo_deps(),
        "roadmap_open_tasks": read_roadmap_tasks(),
        "report_summaries": read_report_summaries(),
        "plan_review_worker_link": plan_review_worker_link,
        "review_statuses": review_statuses,
        "dead_links": check_local_links(),
        "backlog_remaining": backlog_remaining,
    }

    out = Path("docs") / "FACTS.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(facts, indent=2), encoding="utf-8")
    print(f"FACTS.json written: {len(json.dumps(facts))} bytes, "
          f"{len(facts['todos'])} TODOs, "
          f"{len(facts['roadmap_open_tasks'])} roadmap tasks, "
          f"{len(facts['dead_links'])} dead links detected")


if __name__ == "__main__":
    main()