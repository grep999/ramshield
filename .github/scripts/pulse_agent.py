#!/usr/bin/env python3
"""RamShield Pulse Agent — deterministic no-agent heartbeat.

Reads the highest-priority open task from docs/BACKLOG.md (or docs/PLAN.md as
fallback) and appends a timestamped activity line to docs/PULSE_LOG.md. Keeps the
pulse log fresh without needing an LLM inference call, so it is immune to model
provider drift.
"""

import os
import re
import sys
from datetime import datetime, timezone
from pathlib import Path


WORKSPACE = Path(os.environ.get("GITHUB_WORKSPACE", Path(__file__).parent.parent.parent))
PULSE_LOG = WORKSPACE / "docs" / "PULSE_LOG.md"
BACKLOG = WORKSPACE / "docs" / "BACKLOG.md"
PLAN = WORKSPACE / "docs" / "PLAN.md"


def _find_open_task_in_backlog(path: Path) -> str:
    """Return the first open task [ ] text found in BACKLOG.md."""
    if not path.exists():
        return ""
    content = path.read_text(encoding="utf-8")
    # Match lines like: 1. [ ] Implement something
    # or: - [ ] Task description
    for line in content.splitlines():
        stripped = line.strip()
        match = re.match(r"^(?:\d+\.\s*|-\s*)\[\s*\]\s*(.+)$", stripped)
        if match:
            return match.group(1).strip()
    return ""


def _find_open_task_in_plan(path: Path) -> str:
    """Return a task summary from PLAN.md if available."""
    if not path.exists():
        return ""
    content = path.read_text(encoding="utf-8")
    for line in content.splitlines():
        stripped = line.strip()
        if stripped.startswith("### T"):
            return stripped.lstrip("# ").strip()
    return ""


def _select_task() -> str:
    task = _find_open_task_in_backlog(BACKLOG)
    if task:
        return task
    task = _find_open_task_in_plan(PLAN)
    if task:
        return task
    return "No open tasks found — backlog/plan empty or all complete"


def _append_pulse(task: str) -> None:
    PULSE_LOG.parent.mkdir(parents=True, exist_ok=True)
    ts = datetime.now(timezone.utc).strftime("%a %d %b %H:%M:%S UTC %Y")
    line = f"{ts}: Pulse — {task}\n"
    if PULSE_LOG.exists():
        existing = PULSE_LOG.read_text(encoding="utf-8")
        # If the file still contains the placeholder, replace it with a real entry.
        if "No pulse entries yet" in existing:
            existing = ""
    else:
        existing = ""
    PULSE_LOG.write_text(existing + line, encoding="utf-8")


def main() -> int:
    os.chdir(WORKSPACE)
    task = _select_task()
    _append_pulse(task)
    print(f"Pulse updated: {task}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
