#!/usr/bin/env python3
"""RamShield operator console — tiny terminal window to drive the agent.

Stdlib only. Commands: status, errors, log, jobs, list, run, ask, help, quit.
"""
import json
import os
import subprocess
import sys
from datetime import datetime
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CRON_JSON = REPO / "docs" / "CRON_STATUS.json"
OPLOG = REPO / "docs" / "OPERATOR_LOG.md"
HERMES = os.environ.get("HERMES_BIN", "hermes")

CY = "\033[36m"; GR = "\033[32m"; RD = "\033[31m"; YL = "\033[33m"; DM = "\033[2m"; RS = "\033[0m"


def _c(text, color):
    return f"{color}{text}{RS}"


def load_jobs():
    try:
        return json.load(open(CRON_JSON))["jobs"]
    except Exception:
        return []


def cmd_status(_arg):
    jobs = load_jobs()
    n_err = sum(j["status"] == "error" for j in jobs)
    n_ok = sum(j["status"] in ("ok", "running", "scheduled") for j in jobs)
    color = GR if n_err == 0 else (YL if n_err < 5 else RD)
    print(f"fleet: {len(jobs)} jobs  {_c(f'{n_ok} healthy', GR)}  {_c(f'{n_err} error', color)}")
    # engine health
    try:
        import urllib.request
        with urllib.request.urlopen("http://127.0.0.1:9999/api/snapshot", timeout=2) as r:
            d = json.load(r)
        print(f"engine: up  blocked={d.get('pipeline', {}).get('blocked')} "
              f"ingested={d.get('events_ingested'):,} healthy={d.get('is_healthy')}")
    except Exception:
        print(_c("engine: down (127.0.0.1:9999 not responding)", DM))
    try:
        branch = subprocess.run(["git", "-C", str(REPO), "branch", "--show-current"],
                                capture_output=True, text=True).stdout.strip()
        commit = subprocess.run(["git", "-C", str(REPO), "log", "--oneline", "-1"],
                                capture_output=True, text=True).stdout.strip()
        dirty = subprocess.run(["git", "-C", str(REPO), "status", "--short"],
                               capture_output=True, text=True).stdout.count("\n")
        print(f"git: {branch or '?'} @ {commit.split()[0] if commit else '?'}  dirty_files={dirty}")
    except Exception:
        pass


def cmd_errors(arg):
    jobs = [j for j in load_jobs() if j["status"] == "error"]
    if not jobs:
        print(_c("no errors", GR))
        return
    for j in jobs[: int(arg or len(jobs))]:
        err = (j.get("last_error") or "").splitlines()[0][:90]
        print(f"{RD}{j['name']}{RS} [{j['job_id']}]\n  {DM}{err}{RS}")


def cmd_log(arg):
    try:
        lines = open(OPLOG).read().splitlines()
    except FileNotFoundError:
        print("no OPERATOR_LOG.md")
        return
    for ln in lines[-int(arg or 15):]:
        print(f"{DM}{ln}{RS}")


def cmd_jobs(_arg):
    for j in sorted(load_jobs(), key=lambda x: x["name"]):
        s = j["status"]
        col = GR if s in ("ok", "running") else (YL if s == "scheduled" else RD)
        print(f"{j['name']:34} {j['schedule']:14} {_c(s, col)}")


def cmd_list(_arg):
    for j in sorted(load_jobs(), key=lambda x: x["name"]):
        print(f"{j['job_id']}  {j['name']}")


def cmd_run(job_name_or_id):
    jobs = load_jobs()
    m = next((j for j in jobs if j["job_id"] == job_name_or_id), None) \
        or next((j for j in jobs if j["name"] == job_name_or_id), None)
    if not m:
        print(f"unknown job: {job_name_or_id}. use 'list'.")
        return 1
    r = subprocess.run([HERMES, "cron", "run", m["job_id"]], capture_output=True, text=True)
    print(r.stdout.strip() or r.stderr.strip() or "triggered")
    return r.returncode


def cmd_ask(text):
    if not text:
        print("usage: ask <prompt>")
        return 1
    print(f"{DM}[agent working…]{RS}")
    r = subprocess.run([HERMES, "-z", text], capture_output=True, text=True)
    out = (r.stdout or r.stderr or "").strip()
    print(out[-4000:] if out else "(no output)")
    return r.returncode


COMMANDS = {
    "status": cmd_status, "errors": cmd_errors, "log": cmd_log,
    "jobs": cmd_jobs, "list": cmd_list, "run": cmd_run, "ask": cmd_ask,
}

HELP = """commands:
  status          fleet counts + engine health + git state
  errors [n]      failing jobs w/ error text
  log [n]         tail OPERATOR_LOG.md (default 15)
  jobs            all jobs: name schedule status
  list            job ids + names (for run)
  run <id|name>   trigger a cronjob now
  ask <text>      anything else → hermes agent
  quit"""


def main():
    if not sys.stdin.isatty():
        # one-shot mode: operator_console.py <command> [arg]
        argv = sys.argv[1:]
        if not argv:
            print(HELP)
            return 1
        fn = COMMANDS.get(argv[0])
        if not fn:
            return cmd_ask(" ".join(argv))
        return fn(argv[1]) if len(argv) > 1 else fn(None)

    print(_c("ramshield operator console — 'help' for commands, 'quit' to exit", CY))
    while True:
        try:
            line = input(_c("op> ", CY)).strip()
        except (EOFError, KeyboardInterrupt):
            break
        if not line:
            continue
        if line in ("quit", "exit", "q"):
            break
        if line in ("help", "?"):
            print(HELP)
            continue
        parts = line.split(None, 1)
        fn = COMMANDS.get(parts[0])
        arg = parts[1].strip() if len(parts) > 1 else None
        if fn is None:
            cmd_ask(line)  # freeform → agent
        else:
            try:
                fn(arg)
            except Exception as e:
                print(f"{RD}{e}{RS}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
