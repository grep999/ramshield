#!/usr/bin/env python3
"""Git Automation Script for Cron Jobs.

Handles: auto-commit, push, error reporting, sync.
Run via cron (no_agent=true).
"""
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parent.parent.parent
LOG_DIR = WORKSPACE / "docs" / "git_logs"
LOG_DIR.mkdir(parents=True, exist_ok=True)

def run_cmd(cmd, cwd=None, check=True):
    """Run command, return (success, stdout, stderr)."""
    try:
        r = subprocess.run(
            cmd, shell=True, cwd=cwd or WORKSPACE,
            capture_output=True, text=True, timeout=60
        )
        return r.returncode == 0, r.stdout.strip(), r.stderr.strip()
    except subprocess.TimeoutExpired:
        return False, "", "timeout"
    except Exception as e:
        return False, "", str(e)

def log_event(level, msg):
    """Append to git log file."""
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    log_file = LOG_DIR / "git_automation.log"
    existing = log_file.read_text(encoding="utf-8") if log_file.exists() else ""
    log_file.write_text(existing + f"\n[{ts}] {level}: {msg}", encoding="utf-8")

def git_status():
    """Check if there are uncommitted changes."""
    ok, out, _ = run_cmd("git status --porcelain")
    return out.strip() != ""

def git_auto_commit_push():
    """Commit all changes and push to origin."""
    if not git_status():
        log_event("INFO", "No changes to commit")
        return True, "No changes"

    # Stage all
    ok, _, err = run_cmd("git add -A")
    if not ok:
        log_event("ERROR", f"git add failed: {err}")
        return False, f"git add failed: {err}"

    # Commit
    msg = f"chore: auto-commit {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M')}"
    ok, _, err = run_cmd(f'git commit -m "{msg}"')
    if not ok:
        log_event("ERROR", f"git commit failed: {err}")
        return False, f"git commit failed: {err}"

    # Push
    ok, out, err = run_cmd("git push origin HEAD")
    if not ok:
        # Try to fetch and rebase if rejected
        log_event("WARN", f"push failed: {err}, trying fetch+rebase")
        run_cmd("git fetch origin")
        run_cmd("git rebase origin/main || git rebase origin/master")
        ok2, _, err2 = run_cmd("git push origin HEAD")
        if not ok2:
            log_event("ERROR", f"push after rebase failed: {err2}")
            return False, f"push failed: {err2}"
    log_event("OK", f"auto-commit pushed: {msg}")
    return True, "committed & pushed"

def git_sync_branches():
    """Ensure main and master are in sync."""
    for branch in ["main", "master"]:
        ok, _, _ = run_cmd(f"git rev-parse --verify origin/{branch}")
        if ok:
            run_cmd(f"git fetch origin {branch}:{branch} 2>/dev/null || true")

def main():
    os.chdir(WORKSPACE)
    log_event("START", "=== Git Automation Run ===")
    git_sync_branches()
    ok, msg = git_auto_commit_push()
    log_event("END" if ok else "FAIL", msg)
    sys.exit(0 if ok else 1)

if __name__ == "__main__":
    main()