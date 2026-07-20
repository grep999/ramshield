#!/usr/bin/env python3
"""Cron Status Collector — snapshots active cronjobs into docs/CRON_STATUS.md.

Pure data, no LLM. Runs via cronjob (no_agent=true). Downstream agents and the
dashboard generator read this markdown. Project-agnostic: works for any Hermes
workspace with the `hermes cron` CLI available.
"""
import subprocess
from datetime import datetime, timezone
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parent.parent.parent


def list_cron_jobs():
    try:
        out = subprocess.check_output(
            ["hermes", "cron", "list"], text=True, stderr=subprocess.DEVNULL
        )
        return out
    except Exception as e:
        return f"_Error listing cron jobs: {e}_"


def parse_jobs(raw: str):
    """Parse `hermes cron list` output. Returns list of dicts."""
    import re
    jobs = []
    # Pattern: <id> [state]\n  Name: X\n  Schedule: X\n...
    block_re = re.compile(
        r"(?P<id>[a-f0-9]+)\s+\[(?P<state>[^\]]+)\]\s*\n"
        r"\s*Name:\s+(?P<name>.+?)\n"
        r"\s*Schedule:\s+(?P<schedule>.+?)(?:\n|$)"
        r"(?:.*?\nLast Status:\s+(?P<last_status>.+?)(?:\n|$))?",
        re.S,
    )
    for m in block_re.finditer(raw):
        jobs.append({
            "name": m.group("name").strip(),
            "job_id": m.group("id").strip(),
            "schedule": m.group("schedule").strip(),
            "last_status": (m.group("last_status") or "never").strip(),
            "state": m.group("state").strip(),
        })
    return jobs


def main():
    os.chdir(WORKSPACE)
    raw = list_cron_jobs()
    jobs = parse_jobs(raw)
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

    lines = [f"# Cron Job Status — {ts}", ""]
    lines.append(f"**{len(jobs)} active jobs** across the automation pipeline.")
    lines.append("")
    lines.append("| Job | Schedule | Last Status | Next Run | State |")
    lines.append("| :--- | :--- | :--- | :--- | :--- |")
    for j in jobs:
        sched = j["schedule"]
        row = "| " + j["name"] + " | `" + sched + "` | " + j["last_status"] + " | " + j["job_id"] + " | " + j["state"] + " |"
        lines.append(row)
    lines.append("")
    lines.append("---")
    lines.append("")
    lines.append("Raw `hermes cron list` output:")
    lines.append("")
    lines.append("```")
    lines.append(raw if raw else "_No data_")
    lines.append("```")

    out = Path("docs") / "CRON_STATUS.md"
    out.parent.mkdir(parents=True, exist_ok=True)
    content = "\n".join(lines)
    out.write_text(content, encoding="utf-8")
    print(f"CRON_STATUS.md written: {len(jobs)} jobs, {len(content)} bytes")


if __name__ == "__main__":
    import os
    main()
