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
from datetime import datetime, timezone
from pathlib import Path


def find_todos(root_path="src", extensions=("rs", "md", "toml", "sh", "py")):
    """Scan files for TODO, FIXME, HACK, XXX markers."""
    todos = []
    pattern = re.compile(r'(//|<!--|#)\s*(TODO|FIXME|HACK|XXX):?\s*(.+)', re.IGNORECASE)
    for ext in extensions:
        for path in Path(root_path).rglob(f"*.{ext}"):
            try:
                content = path.read_text(encoding='utf-8')
                for match in pattern.finditer(content):
                    comment, kind, text = match.groups()
                    todos.append({
                        "file": str(path),
                        "line": content[:match.start()].count('\n') + 1,
                        "kind": kind.upper(),
                        "text": text.strip()[:100]
                    })
            except (UnicodeDecodeError, OSError):
                continue
    return todos


def parse_roadmap_tree():
    """Parse the Research Audit Tree and Extension Tree from ROADMAP.md."""
    roadmap_path = Path('docs/ROADMAP.md')
    if not roadmap_path.exists():
        return {"research": [], "extensions": [], "milestones": []}

    try:
        content = roadmap_path.read_text(encoding='utf-8')
    except (OSError, IOError):
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

    return {"research": research, "extensions": extensions, "milestones": milestones}


def count_metrics(root_path="src"):
    """Analyze Rust codebase structure and complexity."""
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
        except (UnicodeDecodeError, OSError):
            continue
    return metrics


def get_git_state():
    """Capture git repository state for the report."""
    try:
        sha = subprocess.run(['git', 'rev-parse', '--short', 'HEAD'], capture_output=True, text=True, check=False).stdout.strip()
        branch = subprocess.run(['git', 'rev-parse', '--abbrev-ref', 'HEAD'], capture_output=True, text=True, check=False).stdout.strip()
        commits_today = subprocess.run(['git', 'log', '--since=24 hours ago', '--oneline'], capture_output=True, text=True, check=False).stdout.strip().split('\n')
        return {"sha": sha, "branch": branch, "commits_24h": len([c for c in commits_today if c])}
    except (subprocess.SubprocessError, OSError) as e:
        return {"error": str(e)}


def generate_report(workspace_root):
    """Generate unified helper report including Research Audit Tree."""
    os.chdir(workspace_root)
    todos = find_todos()
    metrics = count_metrics()
    roadmap_tree = parse_roadmap_tree()
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
    print(f"Updated {output}")


def main():
    workspace = os.environ.get('GITHUB_WORKSPACE', str(Path(__file__).parent.parent.parent))
    generate_report(workspace)


if __name__ == '__main__':
    main()