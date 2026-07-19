#!/usr/bin/env python3
"""Project Health Dashboard Agent

Generates a comprehensive dashboard of project health, including:
- Helper Agent's activity logs
- Outstanding tasks and directives from the user/agent
- Discovered insights and potential new ideas from 'research' scans
- Modifiable configuration for agent behavior (e.g., scan frequency, areas of focus)
"""

import os
import re
import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path

# --- Agent Configuration ---
# This dictionary can be modified by a special cron job or manual PR.
# Changes here directly influence the behavior of other helper agents.
AGENT_CONFIG = {
    "scan_frequency_minutes": 5,
    "todo_scan_paths": ["src", "docs", ".github"],
    "research_scan_keywords": ["ml", "ai", "rust", "security", "ddos", "performance", "async", "tokio", "axum", "observability", "metrics"],
    "new_ideas_output_file": "docs/NEW_IDEAS.md",
    "report_output_file": "docs/HEALTH_DASHBOARD.md",
    "helper_agent_log_file": ".github/logs/helper_agent.log",
    "dependency_audit_report": "docs/DEPENDENCY_AUDIT.md",
    "roadmap_file": "docs/ROADMAP.md"
}

# --- Utility Functions (shared logic) ---
def read_file_content(file_path):
    try:
        return Path(file_path).read_text(encoding='utf-8')
    except (FileNotFoundError, UnicodeDecodeError, OSError):
        return None

def write_file_content(file_path, content):
    Path(file_path).parent.mkdir(parents=True, exist_ok=True)
    Path(file_path).write_text(content, encoding='utf-8')

# --- Core Data Gathering Functions ---
def get_helper_agent_activity(log_file):
    """Summarize recent activity from the helper agent's log."""
    content = read_file_content(log_file)
    if not content:
        return "No recent activity log found."
    
    lines = content.split('\n')
    recent_activity = []
    for line in reversed(lines):
        if line.strip():
            recent_activity.append(line)
        if len(recent_activity) >= 5: # Show last 5 log entries
            break
    return "\n".join(reversed(recent_activity)) if recent_activity else "No detailed logs."

def get_outstanding_tasks():
    """Extract and prioritize tasks based on various markers/files."""
    tasks = []
    # From AGENT_REPORT (TODOs/FIXMEs)
    agent_report_path = Path('docs') / 'AGENT_REPORT.md'
    if agent_report_path.exists():
        content = read_file_content(agent_report_path)
        if content:
            todo_matches = re.findall(r'- \*\*(TODO|FIXME|HACK|XXX)\*\* in `([^`:]+):(\d+)` — (.+)', content)
            for kind, file, line, text in todo_matches:
                tasks.append(f"[{kind}] {text} (in {file}:{line})")
    
    # From ROADMAP (uncompleted items)
    roadmap_path = Path(AGENT_CONFIG["roadmap_file"])
    if roadmap_path.exists():
        content = read_file_content(roadmap_path)
        if content:
            roadmap_matches = re.findall(r'- \[\s+\]\s*(.+)', content)
            for task_text in roadmap_matches:
                tasks.append(f"[ROADMAP] {task_text}")

    return tasks if tasks else ["No explicit outstanding tasks identified."]

def scan_for_new_ideas(scan_paths, keywords):
    """Scans specific paths for new patterns, technologies, or concepts related to keywords."""
    new_ideas = []
    # This is a placeholder for a more advanced NLP/LLM-based scan
    # For now, it will simulate by checking markdown files for keywords
    
    # Simulate discovering new insights from existing documentation (e.g., TODO comments that suggest new features)
    for path in Path("docs").rglob("*.md"):
        content = read_file_content(path)
        if content:
            for keyword in keywords:
                if keyword in content.lower() and f"new idea: {keyword}" not in [idea.lower() for idea in new_ideas]:
                    new_ideas.append(f"Potential for {keyword} integration based on discussions in {path}")
    
    # Simulate discovering new ideas from research directory
    research_readme = read_file_content("research/README.md")
    if research_readme:
        for keyword in keywords:
            if keyword in research_readme.lower() and f"new idea: {keyword} research" not in [idea.lower() for idea in new_ideas]:
                 new_ideas.append(f"Explore {keyword} in depth from research/README.md")
    
    # Simulate discovering new ideas from CODE_OF_CONDUCT (e.g., community management tools)
    coc_content = read_file_content("CODE_OF_CONDUCT.md")
    if coc_content and "community" in keywords and "new idea: community management tools" not in [idea.lower() for idea in new_ideas]:
        new_ideas.append("Investigate automated community moderation/management tools.")

    return new_ideas if new_ideas else ["No new ideas discovered in this cycle."]

def get_config_summary():
    """Returns a formatted string of the current agent configuration."""
    summary = ""
    for key, value in AGENT_CONFIG.items():
        summary += f"- **{key}**: {value}\n"
    return summary

def generate_health_dashboard(workspace_root):
    """Generates the main project health dashboard."""
    os.chdir(workspace_root)
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

    report = f"""# RamShield Project Health Dashboard

**Generated:** {timestamp}

---

## ⚙️ Agent Configuration

This section displays the current operational parameters for the Helper Agent. These settings can be modified to direct the agent's focus and behavior.

```json
{json.dumps(AGENT_CONFIG, indent=2)}
```

---

## 🏃‍♀️ Helper Agent Activity Log

This log captures recent actions and observations from the Helper Agent's last runs.

```
{get_helper_agent_activity(AGENT_CONFIG['helper_agent_log_file'])}
```

---

## 📝 Outstanding Tasks & Directives

Prioritized list of known tasks, TODOs, FIXMEs, and uncompleted roadmap items.

"""
    for task in get_outstanding_tasks():
        report += f"- {task}\n"

    report += """
---

## 🌱 New Ideas & Research Insights

This section highlights potential new features, research directions, or areas for improvement identified by the Helper Agent through its codebase and documentation scans.

"""
    for idea in scan_for_new_ideas(AGENT_CONFIG["todo_scan_paths"], AGENT_CONFIG["research_scan_keywords"]):
        report += f"- {idea}\n"

    report += f"""
---

## 📈 Dependency Audit Status

[See full report here]({AGENT_CONFIG['dependency_audit_report']})

---

## 🗺️ Roadmap Progress Overview

[See full roadmap here]({AGENT_CONFIG['roadmap_file']})

---

*Generated by RamShield Project Health Dashboard Agent v0.3.0*
"""
    write_file_content(AGENT_CONFIG["report_output_file"], report)
    print(f"Updated {AGENT_CONFIG['report_output_file']}")

# --- Main Execution ---
def main():
    workspace = os.environ.get(
        'GITHUB_WORKSPACE',
        str(Path(__file__).parent.parent.parent)
    )
    generate_health_dashboard(workspace)

if __name__ == '__main__':
    main()