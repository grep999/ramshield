#!/usr/bin/env python3
"""HTML Automation Dashboard Generator — Production-grade operator console.

Renders a single-file, no-build HTML dashboard for the RamShield autonomous
agent fleet. Features:
- Sticky command bar + section overview
- Live cron state panel with scaling analysis
- Job chain visualization (facts → planner → dispatcher → workers → reviewer)
- Per-module status cards with log tails
- Slick & Edgy dark theme (neon cyan, Space Grotesk)
"""

import json
import os
import re
from datetime import datetime, timezone
from pathlib import Path


# ═══════════════════════════════════════════════════════════════════════════
# CSS
# ═══════════════════════════════════════════════════════════════════════════
CSS = """
:root {
  --bg:#07070b; --surface:#0f0f16; --surface-2:#15151f; --surface-3:#1b1b27;
  --border:#252536; --border-strong:#33334a;
  --text:#f0f0f7; --muted:#8b8ba7; --muted-2:#5c5c75;
  --accent:#00f5d4; --accent-dim:#00c9a8; --accent-glow:rgba(0,245,212,.12);
  --warn:#ffb800; --danger:#ff3d5a; --ok:#00d47e; --info:#4da6ff; --purple:#a78bfa;
  --grid:rgba(0,245,212,.04);
}
* { box-sizing:border-box; }
html { scroll-behavior:smooth; }
body {
  margin:0; min-height:100vh;
  font-family:'Space Grotesk',system-ui,-apple-system,sans-serif;
  background:var(--bg); color:var(--text);
  background-image:
    linear-gradient(var(--grid) 1px, transparent 1px),
    linear-gradient(90deg, var(--grid) 1px, transparent 1px);
  background-size:44px 44px;
  background-attachment:fixed;
  line-height:1.55;
}
a { color:var(--accent); text-decoration:none; }
a:hover { text-shadow:0 0 10px var(--accent); }

.container { max-width:1480px; margin:0 auto; padding:24px; }

/* Header / Command bar */
.cmdbar {
  position:sticky; top:0; z-index:100;
  background:rgba(7,7,11,.85); backdrop-filter:blur(12px);
  border-bottom:1px solid var(--border);
  padding:14px 0; margin-bottom:24px;
}
.cmdbar-inner { display:flex; align-items:center; justify-content:space-between; gap:16px; flex-wrap:wrap; }
.cmdbar-title { display:flex; align-items:center; gap:12px; }
.cmdbar-title h1 {
  margin:0; font-size:clamp(1.3rem,2.5vw,1.7rem); font-weight:700; letter-spacing:-.02em;
  background:linear-gradient(90deg,var(--accent),var(--info));
  -webkit-background-clip:text; background-clip:text; color:transparent;
}
.cmdbar-meta { display:flex; gap:10px; align-items:center; flex-wrap:wrap; }
.heartbeat {
  width:10px; height:10px; border-radius:50%; background:var(--ok);
  box-shadow:0 0 0 0 rgba(0,212,126,.4);
  animation:pulse 2s infinite;
}
@keyframes pulse {
  0% { box-shadow:0 0 0 0 rgba(0,212,126,.4); }
  70% { box-shadow:0 0 0 8px rgba(0,212,126,0); }
  100% { box-shadow:0 0 0 0 rgba(0,212,126,0); }
}
.badge {
  font-size:.68rem; font-weight:600; text-transform:uppercase; letter-spacing:.06em;
  padding:4px 10px; border-radius:999px; border:1px solid transparent; white-space:nowrap;
}
.badge-ok { background:rgba(0,212,126,.12); border-color:var(--ok); color:var(--ok); }
.badge-warn { background:rgba(255,184,0,.12); border-color:var(--warn); color:var(--warn); }
.badge-danger { background:rgba(255,61,90,.12); border-color:var(--danger); color:var(--danger); }
.badge-info { background:rgba(77,166,255,.12); border-color:var(--info); color:var(--info); }
.badge-neutral { background:rgba(122,122,154,.12); border-color:var(--muted); color:var(--muted); }
.badge-purple { background:rgba(167,139,250,.12); border-color:var(--purple); color:var(--purple); }

/* Section overview nav */
.overview {
  display:flex; gap:10px; flex-wrap:wrap; margin-bottom:28px;
  padding:14px; background:var(--surface); border:1px solid var(--border); border-radius:12px;
}
.overview a {
  display:flex; align-items:center; gap:8px;
  padding:8px 14px; border-radius:8px; background:var(--surface-2); border:1px solid var(--border);
  color:var(--muted); font-size:.78rem; font-weight:600; text-transform:uppercase; letter-spacing:.05em;
  transition:all .15s;
}
.overview a:hover { background:var(--surface-3); color:var(--text); border-color:var(--accent-dim); }
.overview-dot { width:7px; height:7px; border-radius:50%; background:var(--muted-2); }

/* Cards */
.section-label {
  font-size:.65rem; color:var(--muted); letter-spacing:.2em; font-weight:600; text-transform:uppercase;
  margin:32px 0 12px 6px; padding-left:8px; border-left:2px solid var(--accent-dim);
}
.grid { display:grid; gap:18px; }
@media (min-width: 900px) {
  .grid-2 { grid-template-columns:repeat(2,1fr); }
  .grid-3 { grid-template-columns:repeat(3,1fr); }
  .grid-4 { grid-template-columns:repeat(4,1fr); }
}
.panel {
  background:var(--surface); border:1px solid var(--border); border-radius:14px; overflow:hidden;
  transition:border-color .2s, box-shadow .2s, transform .2s;
}
.panel:hover { border-color:var(--border-strong); box-shadow:0 0 0 1px var(--accent-glow); }
.panel-header {
  display:flex; align-items:center; justify-content:space-between; gap:12px;
  padding:14px 18px; border-bottom:1px solid var(--border); background:rgba(0,0,0,.15);
}
.panel-header h2 { margin:0; font-size:.92rem; font-weight:600; letter-spacing:.01em; }
.panel-header h2 span { color:var(--muted); font-weight:400; }
.panel-body { padding:16px 18px 18px; }

/* Metrics row */
.metrics { display:grid; grid-template-columns:repeat(auto-fit,minmax(140px,1fr)); gap:12px; margin-bottom:22px; }
.metric {
  background:var(--surface); border:1px solid var(--border); border-radius:12px; padding:14px;
  display:flex; flex-direction:column; gap:4px;
}
.metric-value { font-size:1.6rem; font-weight:700; color:var(--text); }
.metric-label { font-size:.72rem; color:var(--muted); text-transform:uppercase; letter-spacing:.06em; }
.metric-delta { font-size:.72rem; color:var(--muted-2); }

/* Tables */
table { width:100%; border-collapse:collapse; font-size:.78rem; }
th, td { padding:8px 10px; text-align:left; border-bottom:1px solid var(--border); }
th { color:var(--muted); font-weight:600; font-size:.65rem; text-transform:uppercase; letter-spacing:.06em; }
tr:hover td { background:rgba(0,245,212,.04); }
td code { background:rgba(0,245,212,.1); color:var(--accent); padding:2px 6px; border-radius:4px; font-size:.7rem; }

/* Job chain */
.chain { display:flex; align-items:center; flex-wrap:wrap; gap:8px; padding:14px; background:var(--surface-2); border-radius:10px; border:1px solid var(--border); }
.chain-step {
  display:flex; align-items:center; gap:10px; padding:10px 14px; border-radius:8px;
  background:var(--surface-3); border:1px solid var(--border); min-width:140px;
}
.chain-step.ok { border-color:rgba(0,212,126,.4); background:rgba(0,212,126,.06); }
.chain-step.error { border-color:rgba(255,61,90,.4); background:rgba(255,61,90,.06); }
.chain-step.running { border-color:rgba(77,166,255,.4); background:rgba(77,166,255,.06); }
.chain-step.pending { border-color:rgba(255,184,0,.3); background:rgba(255,184,0,.05); }
.chain-dot { width:8px; height:8px; border-radius:50%; background:var(--muted-2); flex-shrink:0; }
.chain-dot.ok { background:var(--ok); box-shadow:0 0 8px var(--ok); }
.chain-dot.error { background:var(--danger); box-shadow:0 0 8px var(--danger); }
.chain-dot.running { background:var(--info); box-shadow:0 0 8px var(--info); }
.chain-dot.pending { background:var(--warn); box-shadow:0 0 8px var(--warn); }
.chain-title { font-weight:600; font-size:.82rem; }
.chain-meta { font-size:.68rem; color:var(--muted); }
.chain-arrow { color:var(--muted-2); font-size:1.1rem; }

/* Module cards */
.module-grid { display:grid; grid-template-columns:repeat(auto-fill,minmax(320px,1fr)); gap:18px; }
.module-status { display:flex; align-items:center; gap:8px; margin-bottom:10px; }
.module-status .status-dot { width:8px; height:8px; border-radius:50%; }
.module-status .status-ok { background:var(--ok); box-shadow:0 0 8px var(--ok); }
.module-status .status-warn { background:var(--warn); box-shadow:0 0 8px var(--warn); }
.module-status .status-danger { background:var(--danger); box-shadow:0 0 8px var(--danger); }
.module-status .status-info { background:var(--info); box-shadow:0 0 8px var(--info); }
.module-status .status-neutral { background:transparent; border:1px solid var(--muted-2); }
.log-tail {
  background:#05050a; border:1px solid var(--border); border-radius:8px; padding:10px 12px;
  font-family:'SF Mono',ui-monospace,monospace; font-size:.72rem; color:var(--muted);
  max-height:110px; overflow:auto; white-space:pre-wrap; margin:12px 0;
}
.log-line { display:block; padding:2px 0; border-bottom:1px solid rgba(255,255,255,.03); }
.log-time { color:var(--muted-2); margin-right:6px; }
.operator-log {
  background:#05050a; border:1px solid var(--border); border-radius:10px; padding:14px;
  font-family:'SF Mono',ui-monospace,monospace; font-size:.74rem; color:var(--muted);
  max-height:220px; overflow:auto; white-space:pre-wrap;
}
.operator-log .log-entry { display:block; padding:3px 0; border-bottom:1px solid rgba(255,255,255,.03); }
.operator-log .log-entry:hover { color:var(--text); }
.operator-log .ts { color:var(--muted-2); margin-right:8px; }

/* Code blocks for copy-paste commands */
pre.cmd {
  background:#05050a; border:1px solid var(--border); border-radius:8px; padding:10px 12px;
  font-family:'SF Mono',ui-monospace,monospace; font-size:.72rem; color:var(--accent);
  overflow-x:auto; margin:8px 0 0;
}

.collapsible {
  background:none; border:none; color:var(--accent); font-size:.72rem; cursor:pointer;
  display:flex; align-items:center; gap:6px; padding:0; font-family:inherit;
}
.collapsible:hover { color:var(--text); }
.collapsible svg { width:12px; height:12px; transition:transform .2s; }
.collapsible[aria-expanded="true"] svg { transform:rotate(90deg); }
.collapsible-content { overflow:hidden; max-height:0; transition:max-height .3s ease; }
.collapsible-content.open { max-height:2000px; }

/* Recommendations */
.rec { display:flex; gap:12px; padding:12px; border-radius:10px; background:var(--surface-2); border:1px solid var(--border); margin-bottom:10px; }
.rec-icon { font-size:1.1rem; flex-shrink:0; }
.rec-title { font-weight:600; font-size:.82rem; margin-bottom:2px; }
.rec-body { font-size:.76rem; color:var(--muted); }

/* Footer */
footer { text-align:center; padding:32px 24px; color:var(--muted); font-size:.72rem; }

/* Markdown body inside modules */
.markdown-body { line-height:1.6; font-size:.82rem; }
.markdown-body h3 { font-size:.95rem; margin:14px 0 8px; color:var(--accent); }
.markdown-body p { margin:8px 0; }
.markdown-body ul, .markdown-body ol { margin:8px 0 8px 18px; }
.markdown-body li { margin:4px 0; }
.markdown-body code { background:rgba(0,245,212,.1); color:var(--accent); padding:2px 6px; border-radius:4px; font-size:.78em; }
.markdown-body pre { background:#05050a; border:1px solid var(--border); border-radius:8px; padding:12px; overflow-x:auto; font-size:.75rem; }
.markdown-body pre code { background:none; color:inherit; padding:0; }
.markdown-body hr { border:none; border-top:1px solid var(--border); margin:14px 0; }
.markdown-body strong { color:var(--accent); }
.markdown-body blockquote { border-left:3px solid var(--accent); padding-left:12px; color:var(--muted); margin:12px 0; font-style:italic; }
.markdown-body table { font-size:.75rem; margin:10px 0; }
"""


# ═══════════════════════════════════════════════════════════════════════════
# Markdown → HTML
# ═══════════════════════════════════════════════════════════════════════════
def md_to_html(md: str) -> str:
    if not md or md.startswith("_"):
        return f'<p style="color:var(--muted);font-style:italic;">{md}</p>'
    html = md
    html = re.sub(r'```(\w+)?\n(.*?)```', r'<pre><code>\2</code></pre>', html, flags=re.S)
    html = re.sub(r'^###\s+(.+)$', r'<h3>\1</h3>', html, flags=re.M)
    html = re.sub(r'^##\s+(.+)$', r'<h3>\1</h3>', html, flags=re.M)
    html = re.sub(r'\*\*(.+?)\*\*', r'<strong>\1</strong>', html)
    html = re.sub(r'`([^`]+)`', r'<code>\1</code>', html)

    def table_repl(m):
        rows = [r.strip() for r in m.group(0).strip().split('\n') if r.strip()]
        if len(rows) < 2:
            return m.group(0)
        header = [c.strip() for c in rows[0].split('|') if c.strip()]
        body = [[c.strip() for c in r.split('|') if c.strip()] for r in rows[2:]]
        out = ['<table><thead><tr>']
        out += [f'<th>{h}</th>' for h in header]
        out.append('</tr></thead><tbody>')
        for row in body:
            out.append('<tr>')
            out += [f'<td>{c}</td>' for c in row]
            out.append('</tr>')
        out.append('</tbody></table>')
        return '\n'.join(out)

    html = re.sub(r'(?:\|.+\|\n)+(?:\|[-:]+\|+\n)+(?:\|.+\|\n?)+', table_repl, html)
    blocks = html.split('\n\n')
    out = []
    for b in blocks:
        b = b.strip()
        if not b:
            continue
        if b.startswith('<'):
            out.append(b)
        else:
            b = b.replace('\n', '<br>')
            out.append(f'<p>{b}</p>')
    return '\n'.join(out)


def read_report(path: str) -> str:
    try:
        return Path(path).read_text(encoding='utf-8')
    except Exception:
        return f"_Report not found: {path}_"


def read_json(path: str) -> dict:
    try:
        return json.loads(Path(path).read_text(encoding='utf-8'))
    except Exception as e:
        return {}


def read_operator_log(path: str) -> str:
    try:
        text = Path(path).read_text(encoding='utf-8')
        lines = [l for l in text.splitlines() if l.strip()]
        return '\n'.join(lines[-30:])  # last 30 lines
    except Exception:
        return "_Operator log is empty._"


def rel_time(iso: str) -> str:
    if not iso:
        return "never"
    try:
        dt = datetime.fromisoformat(iso)
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        delta = datetime.now(timezone.utc) - dt
        if delta.total_seconds() < 60:
            return f"{int(delta.total_seconds())}s ago"
        if delta.total_seconds() < 3600:
            return f"{int(delta.total_seconds()/60)}m ago"
        if delta.total_seconds() < 86400:
            return f"{int(delta.total_seconds()/3600)}h ago"
        return f"{int(delta.total_seconds()/86400)}d ago"
    except Exception:
        return iso[:16]


def parse_log_tail(text: str, n: int = 4) -> list:
    lines = [l for l in text.splitlines() if l.strip() and not l.startswith('#')]
    return lines[-n:]


def module_status(report_text: str) -> str:
    if report_text.startswith("_Report not found"):
        return "neutral"
    lowered = report_text.lower()
    if "error" in lowered or "failed" in lowered or "timeout" in lowered:
        return "danger"
    if "warning" in lowered or "warn" in lowered:
        return "warn"
    if "done" in lowered or "completed" in lowered or "ok" in lowered:
        return "ok"
    return "info"


# ═══════════════════════════════════════════════════════════════════════════
# Cron analysis
# ═══════════════════════════════════════════════════════════════════════════
def analyze_cron(jobs: list) -> dict:
    recommendations = []
    llm_jobs = [j for j in jobs if not j.get("script") and not j.get("no_agent")]
    script_jobs = [j for j in jobs if j.get("script")]
    error_jobs = [j for j in jobs if j.get("status") == "error"]
    freq_5m = [j for j in jobs if j.get("schedule", "").startswith("*/5")]
    freq_10m = [j for j in jobs if j.get("schedule", "").startswith("*/10")]

    # Estimate runs per hour
    runs_per_hour = 0
    for j in jobs:
        sched = j.get("schedule", "")
        if sched.startswith("*/"):
            try:
                mins = int(sched.split()[0].replace("*/", ""))
                if mins > 0:
                    runs_per_hour += 60 // mins
            except Exception:
                pass
        elif sched.startswith("0 *") or sched == "0 * * * *":
            runs_per_hour += 1
        elif sched.startswith("0 "):
            runs_per_hour += 1

    if len(freq_5m) > 3:
        names = ", ".join(j["name"] for j in freq_5m)
        cmd = "\n".join([f"hermes cron edit {j['job_id']} --schedule '*/15 * * * *'" for j in freq_5m])
        recommendations.append({
            "icon": "⚡", "title": "Batch 5-minute jobs",
            "body": f"{len(freq_5m)} jobs run every 5 min ({names}). Reduce gateway load by moving non-urgent ones to 15 min or batching them into a single script.",
            "cmd": cmd
        })

    if len(llm_jobs) > 2:
        llm_names = ", ".join(j["name"] for j in llm_jobs)
        cmd = f"# Convert data collection jobs to no_agent scripts\n# Keep only planner/reviewer as LLM agents\nhermes cron edit <job_id> --script <script.py> --no-agent"
        recommendations.append({
            "icon": "🤖", "title": "Move LLM agents off hot paths",
            "body": f"{len(llm_jobs)} LLM-driven jobs ({llm_names}). Move deterministic data collection to no_agent scripts and reserve LLM for planning/review.",
            "cmd": cmd
        })

    if error_jobs:
        err_names = ", ".join(j['name'] for j in error_jobs)
        cmd = "\n".join([f"hermes cron run {j['job_id']}  # debug {j['name']}" for j in error_jobs])
        recommendations.append({
            "icon": "🚨", "title": f"Stabilize {len(error_jobs)} failing jobs",
            "body": f"{err_names} are failing. Run them manually to capture error output, then convert to no_agent scripts or reduce frequency.",
            "cmd": cmd
        })

    if runs_per_hour > 100:
        recommendations.append({
            "icon": "📊", "title": "Schedule density is high",
            "body": f"~{runs_per_hour} job runs/hour. For scaling, shard by subsystem and introduce a lightweight scheduler that batches work.",
            "cmd": "# Example: batch quickwin promos into one job\nhermes cron remove promo-qw-github-topics promo-qw-awesome-rust promo-qw-crates-io\nhermes cron create '*/5 * * * *' --name promo-quickwin-batch --script promo_batch_all.py --no-agent"
        })

    return {
        "total": len(jobs),
        "llm": len(llm_jobs),
        "script": len(script_jobs),
        "errors": len(error_jobs),
        "runs_per_hour": runs_per_hour,
        "recommendations": recommendations,
    }


# ═══════════════════════════════════════════════════════════════════════════
# Main generator
# ═══════════════════════════════════════════════════════════════════════════
def generate_html_dashboard(workspace_root: str):
    os.chdir(workspace_root)
    now = datetime.now(timezone.utc)
    timestamp = now.strftime("%Y-%m-%d %H:%M UTC")

    reports = {
        "agent": read_report("docs/AGENT_REPORT.md"),
        "deps": read_report("docs/DEPENDENCY_AUDIT.md"),
        "health": read_report("docs/HEALTH_DASHBOARD.md"),
        "control": read_report("docs/CONTROL_CENTER.md"),
        "plan": read_report("docs/PLAN.md"),
        "review": read_report("docs/REVIEW.md"),
        "dispatch": read_report("docs/DISPATCH_LOG.md"),
        "workers": read_report("docs/WORKER_STATUS.md"),
        "roadmap": read_report("docs/ROADMAP.md"),
        "facts": read_json("docs/FACTS.json"),
        "cron": read_json("docs/CRON_STATUS.json"),
        "backlog": read_report("docs/BACKLOG.md"),
        "promotion": read_report("docs/PROMOTION_LOG.md"),
        "research": read_report("docs/RESEARCH.md"),
        "pulse": read_report("docs/PULSE_LOG.md"),
        "health_loop": read_report("docs/HEALTH_LOOP.md"),
        "errors": read_report("docs/ERRORS.md"),
        "operator_log": read_operator_log("docs/OPERATOR_LOG.md"),
        "healer_dispatch": read_report("docs/HEALER_DISPATCH.md"),
    }

    facts = reports["facts"]
    git = facts.get("git", {})
    branch = git.get("branch", "?")
    commit = git.get("commit_short", "?")
    rust_files = facts.get("codebase", {}).get("rust_files", 0)
    loc = facts.get("codebase", {}).get("lines_of_code", 0)
    clippy = facts.get("codebase", {}).get("clippy_warnings", 0)
    todo_count = len(facts.get("todos", []))
    deps_count = len(facts.get("dependencies", []))
    dead_links = facts.get("dead_links", [])

    cron_jobs = reports["cron"].get("jobs", [])
    cron_analysis = analyze_cron(cron_jobs)
    cron_statuses = {"ok": 0, "error": 0, "running": 0, "pending": 0, "scheduled": 0, "unknown": 0}
    for j in cron_jobs:
        cron_statuses[j.get("status", "unknown")] = cron_statuses.get(j.get("status", "unknown"), 0) + 1

    # Build a job lookup
    job_by_name = {j["name"]: j for j in cron_jobs}

    # Discover worker jobs from cron data
    worker_jobs = [j for j in cron_jobs if j.get("name", "").startswith("ramshield-worker-")]
    worker_ids = {j["name"].replace("ramshield-worker-", ""): j for j in worker_jobs}

    # Job chain steps
    chain_steps = [
        ("facts-collector", "Facts", "ramshield-facts-collector", reports["facts"].get("generated_at", "")),
        ("daily-planner", "Planner", "ramshield-daily-planner", ""),
        ("dispatcher", "Dispatcher", "ramshield-dispatcher", reports["dispatch"][:20] if reports["dispatch"].startswith("20") else ""),
        ("workers", f"Workers ({len(worker_jobs)})", "", ""),
        ("reviewer", "Reviewer", "ramshield-reviewer", ""),
    ]

    def step_state(name: str, cron_name: str, fallback_text: str):
        if cron_name and cron_name in job_by_name:
            j = job_by_name[cron_name]
            return j.get("status", "unknown"), rel_time(j.get("last_run", "")), j.get("last_error", "")
        if name == "workers":
            if not worker_jobs:
                return "info", "not dispatched", ""
            error = any(j.get("status") == "error" for j in worker_jobs)
            pending = any(j.get("status") in ("pending", "scheduled") for j in worker_jobs)
            running = any(j.get("status") == "running" for j in worker_jobs)
            if error:
                return "error", f"{len(worker_jobs)} workers", ""
            if running:
                return "running", f"{len(worker_jobs)} workers", ""
            if pending:
                return "pending", f"{len(worker_jobs)} workers", ""
            return "ok", f"{len(worker_jobs)} workers", ""
        st = module_status(fallback_text)
        return st, "manual", ""

    # Section overview statuses
    overview = [
        ("ops", "Operations", cron_statuses["error"] == 0 and cron_statuses["ok"] > 0),
        ("log", "Log", True),
        ("chain", "Job Chain", cron_statuses.get("error", 0) == 0),
        ("modules", "Modules", True),
        ("health", "Health", module_status(reports["health"]) != "danger"),
        ("growth", "Growth", module_status(reports["promotion"]) != "danger"),
        ("backlog", "Backlog", True),
        ("systems", "Systems", cron_analysis["errors"] == 0),
    ]

    project_name = os.environ.get("DASHBOARD_PROJECT_NAME", "RamShield")

    # Helper to render a module card
    def module_card(title: str, status: str, last: str, log_lines: list, full_report: str, badge_text: str = ""):
        st_class = {"ok": "status-ok", "warn": "status-warn", "danger": "status-danger", "info": "status-info"}.get(status, "status-neutral")
        st_label = status.upper()
        log_html = ""
        if log_lines:
            for l in log_lines:
                # If line starts with ISO date, strip it for cleaner tail
                clean = re.sub(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})?\s*', '', l)
                log_html += f'<span class="log-line">• {html_escape(clean[:180])}</span>'
        else:
            log_html = '<span class="log-line" style="color:var(--muted-2);">No recent activity</span>'
        content_id = f"mod-{title.lower().replace(' ', '-')}"
        return f'''
    <article class="panel">
      <div class="panel-header">
        <h2>{title} <span>{badge_text}</span></h2>
        <span class="badge badge-{status if status in ('ok','warn','danger','info') else 'neutral'}">{st_label}</span>
      </div>
      <div class="panel-body">
        <div class="module-status"><div class="status-dot {st_class}"></div><span style="font-size:.78rem;color:var(--muted);">last activity {last}</span></div>
        <div class="log-tail">{log_html}</div>
        <button class="collapsible" aria-expanded="false" onclick="toggle(this,'{content_id}')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="9 18 15 12 9 6"></polyline></svg>
          Full report
        </button>
        <div id="{content_id}" class="collapsible-content">
          <div class="markdown-body" style="margin-top:12px;">{md_to_html(full_report)}</div>
        </div>
      </div>
    </article>'''

    def html_escape(s: str) -> str:
        return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

    # Cron rows
    cron_rows_html = ""
    for j in cron_jobs:
        st = j.get("status", "unknown")
        bg = {"ok": "badge-ok", "error": "badge-danger", "running": "badge-info", "pending": "badge-warn", "scheduled": "badge-neutral"}.get(st, "badge-neutral")
        title = html_escape(j.get("last_error", "")[:120])
        cron_rows_html += f'''
            <tr title="{title}">
              <td>{j['name']}</td>
              <td><code>{j.get('schedule','')}</code></td>
              <td><span class="badge {bg}">{st}</span></td>
              <td>{rel_time(j.get('last_run',''))}</td>
              <td>{j.get('execution','')}</td>
            </tr>'''

    # Recommendations
    recs_html = ""
    if cron_analysis["recommendations"]:
        for r in cron_analysis["recommendations"]:
            cmd_block = f'<pre class="cmd">{html_escape(r.get("cmd", ""))}</pre>' if r.get("cmd") else ""
            recs_html += f'''
    <div class="rec">
      <div class="rec-icon">{r['icon']}</div>
      <div>
        <div class="rec-title">{r['title']}</div>
        <div class="rec-body">{r['body']}</div>
        {cmd_block}
      </div>
    </div>'''
    else:
        recs_html = '<p style="color:var(--muted);">No critical scaling recommendations. Fleet looks healthy.</p>'

    # Chain HTML
    chain_html = '<div class="chain">'
    for idx, (key, label, cron_name, fallback_ts) in enumerate(chain_steps):
        status, last, err = step_state(key, cron_name, reports.get(key, ""))
        chain_html += f'''
      <div class="chain-step {status}">
        <div class="chain-dot {status}"></div>
        <div>
          <div class="chain-title">{label}</div>
          <div class="chain-meta">{last}{f" — {err[:40]}" if err else ""}</div>
        </div>
      </div>'''
        if idx < len(chain_steps) - 1:
            chain_html += '<div class="chain-arrow">→</div>'
    chain_html += '</div>'

    # Module cards
    # Worker status card: prefer live cron jobs over static report
    if worker_jobs:
        worker_lines = []
        for j in worker_jobs:
            tid = j["name"].replace("ramshield-worker-", "")
            worker_lines.append(f"{tid}: {j.get('status', 'unknown')} (last {rel_time(j.get('last_run',''))})")
        worker_status = "error" if any(j.get("status") == "error" for j in worker_jobs) else ("pending" if any(j.get("status") in ("pending","scheduled") for j in worker_jobs) else "ok")
        worker_last = f"{len(worker_jobs)} workers"
    else:
        worker_lines = parse_log_tail(reports["workers"], 3)
        worker_status = module_status(reports["workers"])
        worker_last = "recent"

    # Healer status
    healer_issues = 0
    healer_pending = 0
    healer_chains = []
    for j in cron_jobs:
        name = j.get("name", "")
        if name.startswith("healer-"):
            healer_chains.append(j)
            if j.get("status") == "error":
                healer_issues += 1
            elif j.get("status") in ("scheduled", "pending", "running"):
                healer_pending += 1
    healer_status = "danger" if healer_issues else ("info" if healer_pending else "ok")
    healer_tail = []
    if healer_chains:
        healer_tail.append(f"{len(healer_chains)} temp jobs active ({healer_issues} error, {healer_pending} pending)")
    healer_tail.extend(parse_log_tail(reports["healer_dispatch"], 3))

    modules_html = ""
    modules_html += module_card("Error Healer", healer_status, "recent", healer_tail, reports["healer_dispatch"], f"{len(healer_chains)} temp jobs")
    modules_html += module_card("Facts Collector", module_status(reports["facts"].get("generated_at", "")), rel_time(reports["facts"].get("generated_at", "")), [f"Rust files: {rust_files}, LOC: {loc:,}"], reports["facts"].get("generated_at", "") and json.dumps(reports["facts"], indent=2) or "_No FACTS.json_", f"{rust_files} files")
    modules_html += module_card("Daily Plan", module_status(reports["plan"]), rel_time(reports["plan"][:20] if reports["plan"].startswith("20") else ""), parse_log_tail(reports["plan"], 3), reports["plan"])
    modules_html += module_card("Worker Status", worker_status, worker_last, worker_lines, reports["workers"], f"{len(worker_jobs)} live")
    modules_html += module_card("Review", module_status(reports["review"]), "recent", parse_log_tail(reports["review"], 3), reports["review"])
    modules_html += module_card("Health Loop", module_status(reports["health_loop"]), "recent", parse_log_tail(reports["health_loop"], 3), reports["health_loop"])
    modules_html += module_card("Promotion", module_status(reports["promotion"]), "recent", parse_log_tail(reports["promotion"], 3), reports["promotion"])
    modules_html += module_card("Research", module_status(reports["research"]), "recent", parse_log_tail(reports["research"], 3), reports["research"])
    modules_html += module_card("Pulse", module_status(reports["pulse"]), "recent", parse_log_tail(reports["pulse"], 3), reports["pulse"])

    # Overview nav
    overview_html = ""
    for sec_id, sec_label, healthy in overview:
        dot = "status-ok" if healthy else "status-danger"
        overview_html += f'<a href="#{sec_id}"><span class="overview-dot {dot}"></span>{sec_label}</a>'

    html = f'''<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{project_name} Operator Console</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Space+Grotesk:wght@400;500;600;700&display=swap" rel="stylesheet">
<style>{CSS}</style>
</head>
<body>
<div class="cmdbar">
  <div class="container cmdbar-inner">
    <div class="cmdbar-title">
      <div class="heartbeat"></div>
      <h1>{project_name} Operator Console</h1>
    </div>
    <div class="cmdbar-meta">
      <span class="badge badge-neutral">{branch}</span>
      <span class="badge badge-neutral">{commit}</span>
      <span class="badge badge-info">refreshed {timestamp}</span>
    </div>
  </div>
</div>

<div class="container">
  <nav class="overview" id="overview">
    {overview_html}
  </nav>

  <!-- METRICS -->
  <div class="metrics">
    <div class="metric"><div class="metric-value" style="color:var(--ok);">{cron_statuses['ok']}</div><div class="metric-label">Cron OK</div><div class="metric-delta">of {cron_analysis['total']} jobs</div></div>
    <div class="metric"><div class="metric-value" style="color:var(--danger);">{cron_statuses['error']}</div><div class="metric-label">Cron Errors</div><div class="metric-delta">need attention</div></div>
    <div class="metric"><div class="metric-value">{rust_files}</div><div class="metric-label">Rust Files</div><div class="metric-delta">{loc:,} LOC</div></div>
    <div class="metric"><div class="metric-value" style="color:var(--warn);">{clippy}</div><div class="metric-label">Clippy</div><div class="metric-delta">warnings</div></div>
    <div class="metric"><div class="metric-value">{todo_count}</div><div class="metric-label">TODOs</div><div class="metric-delta">across codebase</div></div>
    <div class="metric"><div class="metric-value" style="color:var(--danger);">{len(dead_links)}</div><div class="metric-label">Dead Links</div><div class="metric-delta">in docs</div></div>
    <div class="metric"><div class="metric-value">{deps_count}</div><div class="metric-label">Dependencies</div><div class="metric-delta">tracked</div></div>
    <div class="metric"><div class="metric-value">{cron_analysis['runs_per_hour']}</div><div class="metric-label">Runs / Hour</div><div class="metric-delta">fleet load</div></div>
  </div>

  <!-- OPERATOR LOG -->
  <div class="section-label" id="log">Operator Log</div>
  <div class="panel">
    <div class="panel-header">
      <h2>Live Event Stream</h2>
      <span class="badge badge-info">last 30 events</span>
    </div>
    <div class="panel-body">
      <div class="operator-log">{"".join(f'<span class="log-entry"><span class="ts">{html_escape(l[:19])}</span>{html_escape(l[22:])}</span>' for l in reports["operator_log"].splitlines() if l.strip())}</div>
    </div>
  </div>

  <!-- JOB CHAIN -->
  <div class="section-label" id="chain">Autonomous Pipeline</div>
  {chain_html}

  <!-- OPERATIONS -->
  <div class="section-label" id="ops">Operations</div>
  <div class="panel">
    <div class="panel-header">
      <h2>Cron Fleet Status <span>{cron_analysis['total']} jobs • {cron_analysis['script']} script • {cron_analysis['llm']} LLM</span></h2>
      <span class="badge badge-{('ok' if cron_statuses['error']==0 else 'danger')}">{cron_statuses['error']} errors</span>
    </div>
    <div class="panel-body">
      <div style="display:flex;gap:8px;flex-wrap:wrap;margin-bottom:14px;">
        <span class="badge badge-ok">OK {cron_statuses['ok']}</span>
        <span class="badge badge-danger">Errors {cron_statuses['error']}</span>
        <span class="badge badge-info">Running {cron_statuses['running']}</span>
        <span class="badge badge-warn">Pending {cron_statuses['pending']}</span>
        <span class="badge badge-neutral">Scheduled {cron_statuses['scheduled']}</span>
      </div>
      <table>
        <thead><tr><th>Job</th><th>Schedule</th><th>Status</th><th>Last Run</th><th>Execution</th></tr></thead>
        <tbody>{cron_rows_html}</tbody>
      </table>
    </div>
  </div>

  <!-- MODULES -->
  <div class="section-label" id="modules">Module Consoles</div>
  <div class="module-grid">
    {modules_html}
  </div>

  <!-- HEALTH & BACKLOG -->
  <div class="section-label" id="health">Health & Backlog</div>
  <div class="grid grid-2">
    <div class="panel">
      <div class="panel-header"><h2>Project Health</h2><span class="badge badge-{module_status(reports['health'])}">{module_status(reports['health']).upper()}</span></div>
      <div class="panel-body">{md_to_html(reports['health'])}</div>
    </div>
    <div class="panel">
      <div class="panel-header"><h2>Error Ledger</h2><span class="badge badge-{module_status(reports['errors'])}">{module_status(reports['errors']).upper()}</span></div>
      <div class="panel-body">{md_to_html(reports['errors'])}</div>
    </div>
    <div class="panel">
      <div class="panel-header"><h2>Backlog</h2><span class="badge badge-info">live</span></div>
      <div class="panel-body">{md_to_html(reports['backlog'])}</div>
    </div>
    <div class="panel">
      <div class="panel-header"><h2>Control Center</h2><span class="badge badge-info">directive</span></div>
      <div class="panel-body">{md_to_html(reports['control'])}</div>
    </div>
  </div>

  <!-- GROWTH -->
  <div class="section-label" id="growth">Growth & Discovery</div>
  <div class="grid grid-2">
    <div class="panel">
      <div class="panel-header"><h2>Roadmap</h2><span class="badge badge-info">{len(facts.get('roadmap_open_tasks', []))} open</span></div>
      <div class="panel-body">{md_to_html(reports['roadmap'])}</div>
    </div>
    <div class="panel">
      <div class="panel-header"><h2>Dependency Audit</h2><span class="badge badge-info">{deps_count} crates</span></div>
      <div class="panel-body">{md_to_html(reports['deps'])}</div>
    </div>
  </div>

  <!-- SYSTEMS ENGINEERING -->
  <div class="section-label" id="systems">Systems Engineering</div>
  <div class="grid grid-2">
    <div class="panel">
      <div class="panel-header">
        <h2>Cron Scaling & Recommendations</h2>
        <span class="badge badge-purple">analysis</span>
      </div>
      <div class="panel-body">
        {recs_html}
      </div>
    </div>
    <div class="panel">
      <div class="panel-header">
        <h2>Self-Healing Ledger</h2>
        <span class="badge badge-{healer_status}">{healer_status.upper()}</span>
      </div>
      <div class="panel-body">
        <p style="color:var(--muted);">Tree-like repair cycles: root <code>ramshield-error-healer</code> schedules analyze → solve → verify temp jobs per issue.</p>
        {md_to_html(reports['healer_dispatch'])}
      </div>
    </div>
  </div>

  <footer>
    {project_name} Operator Console v1.0 • Generated {timestamp} •
    Docs: <a href="https://hermes-agent.nousresearch.com/docs">hermes-agent.nousresearch.com/docs</a>
  </footer>
</div>

<script>
function toggle(btn, id) {{
  const el = document.getElementById(id);
  const open = el.classList.toggle('open');
  btn.setAttribute('aria-expanded', open);
}}

// Section scrollspy for sticky overview
const sections = ['overview','ops','log','chain','modules','health','growth','backlog','systems'];
const navLinks = document.querySelectorAll('.overview a');
function onScroll() {{
  let current = 'overview';
  for (const id of sections) {{
    const el = document.getElementById(id);
    if (el && el.getBoundingClientRect().top <= 100) current = id;
  }}
  navLinks.forEach(a => {{
    a.style.color = a.getAttribute('href') === '#' + current ? 'var(--accent)' : '';
    a.style.borderColor = a.getAttribute('href') === '#' + current ? 'var(--accent)' : '';
  }});
}}
window.addEventListener('scroll', onScroll, {{passive:true}});
onScroll();
</script>
</body>
</html>'''

    out = Path('docs') / 'AUTOMATION_DASHBOARD.html'
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(html, encoding='utf-8')
    print(f"Generated operator console: {out} ({len(html):,} bytes)")


def main():
    workspace = os.environ.get('GITHUB_WORKSPACE', str(Path(__file__).parent.parent.parent))
    generate_html_dashboard(workspace)


if __name__ == '__main__':
    main()
