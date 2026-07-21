#!/usr/bin/env python3
"""RamShield Helper Agent (v0.3.0)

Combines Junior Engineer (codebase analysis) and Roadmap Engineer (milestone tracking)
into a unified autonomous agent. Runs every 5 minutes via GitHub Actions.

Capabilities:
- Scans codebase for TODOs, FIXMEs, metrics
- Tracks roadmap milestones progress and Research Audit Tree
- Generates reports
- Updates project status files
"""

import os
import re
import json
import subprocess
import logging
from datetime import datetime, timezone
from pathlib import Path


# ── Logging setup ──────────────────────────────────────────────────────────
LOG_DIR = Path(".github/logs")
LOG_FILE = LOG_DIR / "helper_agent.log"
MAX_LOG_BYTES = 2 * 1024 * 1024  # 2 MB
BACKUP_COUNT = 2


class _TzFormatter(logging.Formatter):
    """Formatter that always shows the current UTC offset in the timestamp."""
    def formatTime(self, record, datefmt=None):
        dt = datetime.fromtimestamp(record.created, tz=timezone.utc)
        if datefmt:
            return dt.strftime(datefmt)
        return dt.isoformat(timespec="milliseconds")


def setup_logging():
    """Configure local file logging with rotation and console output."""
    LOG_DIR.mkdir(parents=True, exist_ok=True)

    # Use WatchedFileHandler if available, otherwise RotatingFileHandler
    try:
        from logging.handlers import RotatingFileHandler
        file_handler = RotatingFileHandler(
            LOG_FILE, maxBytes=MAX_LOG_BYTES, backupCount=BACKUP_COUNT, encoding="utf-8"
        )
    except Exception:
        file_handler = logging.FileHandler(LOG_FILE, encoding="utf-8")

    fmt = "%(asctime)s | %(levelname)-8s | %(name)s | %(message)s"
    formatter = _TzFormatter(fmt)
    file_handler.setFormatter(formatter)

    console_handler = logging.StreamHandler()
    console_handler.setFormatter(formatter)

    root = logging.getLogger()
    root.setLevel(logging.DEBUG)
    root.handlers = []
    root.addHandler(file_handler)
    root.addHandler(console_handler)

    return logging.getLogger("helper_agent")


logger = setup_logging()


def log_progress(stage: str, message: str, extra: dict | None = None):
    """Emit a structured progress log for dashboards and humans."""
    payload = {"stage": stage}
    if extra:
        payload.update(extra)
    logger.info("[%(stage)s] %(message)s", {"stage": stage, "message": message, "extra": payload})


def find_todos(root_path="src", extensions=("rs", "md", "toml", "sh", "py")):
    """Scan files for TODO, FIXME, HACK, XXX markers."""
    log_progress("scan", f"Starting TODO scan under {root_path}", {"extensions": extensions})
    todos = []
    pattern = re.compile(r'(//|<!--|#)\s*(TODO|FIXME|HACK|XXX):?\s*(.+)', re.IGNORECASE)
    scanned = 0
    for ext in extensions:
        for path in Path(root_path).rglob(f"*.{ext}"):
            try:
                content = path.read_text(encoding='utf-8')
                scanned += 1
                for match in pattern.finditer(content):
                    comment, kind, text = match.groups()
                    todos.append({
                        "file": str(path),
                        "line": content[:match.start()].count('\n') + 1,
                        "kind": kind.upper(),
                        "text": text.strip()[:100]
                    })
            except (UnicodeDecodeError, OSError) as e:
                logger.warning("Skipping %s: %s", path, e)
    log_progress("scan", f"TODO scan complete", {"files_scanned": scanned, "markers": len(todos)})
    return todos


def parse_roadmap_tree():
    """Parse the Research Audit Tree and Extension Tree from ROADMAP.md."""
    log_progress("roadmap", "Parsing roadmap tree")
    roadmap_path = Path('docs/ROADMAP.md')
    if not roadmap_path.exists():
        logger.warning("Roadmap not found at %s", roadmap_path)
        return {"research": [], "extensions": [], "milestones": []}

    try:
        content = roadmap_path.read_text(encoding='utf-8')
    except (OSError, IOError) as e:
        logger.error("Cannot read roadmap: %s", e)
        return {"research": [], "extensions": [], "milestones": []}

    research = []
    extensions = []
    milestones = []

    current_section = None
    for line in content.split('\n'):
        if 'Research Audit Tree' in line:
            current_section = 'research'
            continue
        elif 'Extension Tree Expansion' in line:
            current_section = 'extensions'
            continue
        elif 'Feature Roadmap & Milestones' in line:
            current_section = 'milestones'
            continue

        if current_section == 'research' and line.strip().startswith('- **'):
            match = re.match(r'- \*\*(.+?)\*\*:?\s*(.*)', line)
            if match:
                research.append({"name": match.group(1).strip(), "desc": match.group(2).strip()})
        elif current_section == 'extensions' and line.strip().startswith('- ['):
            match = re.match(r'- \[.\]\s*(.+)', line)
            if match:
                extensions.append({"name": match.group(1).strip()})
        elif current_section == 'milestones' and ('Milestone' in line or '- [ ]' in line):
            if 'Milestone' in line:
                match = re.match(r'- \*Milestone:\*\s*(.+)', line)
                if match:
                    milestones.append({"text": match.group(1).strip()})

    log_progress("roadmap", "Roadmap tree parsed", {
        "research_nodes": len(research),
        "extensions": len(extensions),
        "milestones": len(milestones)
    })
    return {"research": research, "extensions": extensions, "milestones": milestones}


def count_metrics(root_path="src"):
    """Analyze Rust codebase structure and complexity."""
    log_progress("metrics", f"Counting metrics under {root_path}")
    metrics = {
        "files": 0, "lines": 0, "blank_lines": 0, "comment_lines": 0,
        "unsafe_count": 0, "todo_count": 0, "test_count": 0
    }
    for path in Path(root_path).rglob("*.rs"):
        try:
            content = path.read_text(encoding='utf-8')
            metrics["files"] += 1
            lines = content.split('\n')
            metrics["lines"] += len(lines)
            metrics["blank_lines"] += sum(1 for line in lines if not line.strip())
            metrics["comment_lines"] += sum(1 for line in lines if line.strip().startswith('//'))
            metrics["unsafe_count"] += content.count("unsafe ")
            metrics["test_count"] += content.count('#[test]')
            metrics["todo_count"] += len(re.findall(r'(?:TODO|FIXME|HACK|XXX)', content, re.IGNORECASE))
        except (UnicodeDecodeError, OSError) as e:
            logger.warning("Skipping metrics for %s: %s", path, e)
    log_progress("metrics", "Metrics counted", metrics)
    return metrics


def get_git_state():
    """Capture git repository state for the report."""
    log_progress("git", "Capturing git state")
    try:
        sha = subprocess.run(['git', 'rev-parse', '--short', 'HEAD'], capture_output=True, text=True, check=False).stdout.strip()
        branch = subprocess.run(['git', 'rev-parse', '--abbrev-ref', 'HEAD'], capture_output=True, text=True, check=False).stdout.strip()
        commits_today = subprocess.run(['git', 'log', '--since=24 hours ago', '--oneline'], capture_output=True, text=True, check=False).stdout.strip().split('\n')
        state = {"sha": sha, "branch": branch, "commits_24h": len([c for c in commits_today if c])}
        log_progress("git", "Git state captured", state)
        return state
    except (subprocess.SubprocessError, OSError) as e:
        logger.error("Git state capture failed: %s", e)
        return {"error": str(e)}


def append_operator_log(workspace_root, stage, message, extra=None):
    """Append a machine-readable event to the shared operator log."""
    log_path = Path(workspace_root) / "docs" / "OPERATOR_LOG.md"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    entry = {"ts": ts, "agent": "helper", "stage": stage, "message": message}
    if extra:
        entry["extra"] = extra
    # Keep markdown line with JSON so it renders and is parseable
    line = f"{ts} [helper/{stage}] {message}"
    if extra:
        line += f" | {json.dumps(extra, separators=(',', ':'))}"
    with log_path.open("a", encoding="utf-8") as f:
        f.write(line + "\n")


def generate_report(workspace_root):
    """Generate unified helper report including Research Audit Tree."""
    log_progress("report", "Starting report generation", {"workspace": workspace_root})
    os.chdir(workspace_root)
    append_operator_log(workspace_root, "start", "Helper agent run started")

    todos = find_todos()
    append_operator_log(workspace_root, "scan", "TODO scan complete", {"markers": len(todos)})

    metrics = count_metrics()
    append_operator_log(workspace_root, "metrics", "Codebase metrics collected", metrics)

    roadmap_tree = parse_roadmap_tree()
    append_operator_log(workspace_root, "roadmap", "Roadmap tree parsed", {
        "research_nodes": len(roadmap_tree["research"]),
        "extensions": len(roadmap_tree["extensions"]),
        "milestones": len(roadmap_tree["milestones"])
    })

    git_state = get_git_state()
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

    todo_count = sum(1 for t in todos if t['kind'] == 'TODO')
    fixme_count = sum(1 for t in todos if t['kind'] == 'FIXME')

    report = f"""# RamShield Helper Agent Report

**Generated:** {timestamp}
**Branch:** `{git_state.get('branch', 'unknown')}`
**Commit:** `{git_state.get('sha', 'unknown')}`
**Commits (24h):** {git_state.get('commits_24h', 0)}

---

## Codebase Metrics

| Metric | Value |
|--------|-------|
| Rust files | {metrics['files']} |
| Total lines | {metrics['lines']:,} |
| Comment lines | {metrics['comment_lines']:,} |
| Unsafe blocks | {metrics['unsafe_count']} |

---

## Research Audit Tree

| Domain | Innovation |
|--------|-----------|
"""
    for item in roadmap_tree['research']:
        report += f"| **{item['name']}** | {item['desc']} |\n"

    report += """
---

## Expansion Tree

"""
    for item in roadmap_tree['extensions']:
        report += f"- [ ] {item['name']}\n"

    report += """
---

## Roadmap Milestones

"""
    for item in roadmap_tree['milestones']:
        report += f"- 🏁 **{item['text']}**\n"

    report += f"""
---

## Technical Debt

- Open TODOs: **{todo_count}**
- Open FIXMEs: **{fixme_count}**

---

*Generated by RamShield Helper Agent v0.3.0*
"""
    output = Path('docs') / 'AGENT_REPORT.md'
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(report, encoding='utf-8')
    log_progress("report", f"Report written to {output}")
    append_operator_log(workspace_root, "done", "Helper agent run completed", {
        "todos": todo_count,
        "fixmes": fixme_count,
        "files": metrics['files'],
        "lines": metrics['lines']
    })
    print(f"Updated {output}")


def main():
    workspace = os.environ.get('GITHUB_WORKSPACE', str(Path(__file__).parent.parent.parent))
    log_progress("main", "Helper agent invoked", {"workspace": workspace})
    generate_report(workspace)


if __name__ == '__main__':
    main()