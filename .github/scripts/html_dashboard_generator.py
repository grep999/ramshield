#!/usr/bin/env python3
"""HTML Automation Dashboard Generator — Improved version with Timeline.

Reads Markdown reports and compiles into a single HTML dashboard with:
- Dark theme, neon-cyan accents, grid background (Space Grotesk)
- Main timeline: Roadmap milestones → Current sprint → Today's tasks
- Priority alignment panel: Roadmap priorities vs daily plan
- Dead links report
- All sections that were previously in facts_collector.py now in html_dashboard_generator.py
"""

import os
import re
import json
from datetime import datetime, timezone
from pathlib import Path


# ─── CSS ──────────────────────────────────────────────────────────────────
CSS = """
:root {
  --bg:#0a0a0f; --panel:#11111a; --panel-border:#1f1f2e;
  --text:#e8e8f0; --muted:#7a7a9a; --accent:#00f5d4; --accent-dim:#00c9a8;
  --warn:#ffb800; --danger:#ff3d5a; --ok:#00d47e; --info:#4da6ff;
  --grid:#0d0d14;
}
*, *::before, *::after { box-sizing:border-box; }
html { font-size:16px; }
body {
  margin:0; min-height:100vh;
  font-family:'Space Grotesk',system-ui,sans-serif;
  background:var(--bg); color:var(--text);
  background-image:
    linear-gradient(rgba(0,245,212,.03) 1px, transparent 1px),
    linear-gradient(90deg, rgba(0,245,212,.03) 1px, transparent 1px);
  background-size:40px 40px;
}
a { color:var(--accent); text-decoration:none; }
a:hover { text-shadow:0 0 8px var(--accent); }
.container { max-width:1400px; margin:0 auto; padding:24px; }

/* Header */
header { margin-bottom:32px; }
h1 {
  font-size:clamp(1.6rem,3vw,2.2rem); font-weight:700; letter-spacing:-.02em;
  background:linear-gradient(90deg,var(--accent),var(--info));
  -webkit-background-clip:text; background-clip:text; color:transparent;
  margin:0 0 8px;
}
.subtitle { color:var(--muted); font-size:.95rem; }
.badge-row { display:flex; gap:8px; flex-wrap:wrap; margin-top:12px; }
.badge {
  font-size:.7rem; font-weight:600; text-transform:uppercase; letter-spacing:.05em;
  padding:4px 10px; border-radius:999px; border:1px solid transparent;
}
.badge-ok { background:rgba(0,212,126,.12); border-color:var(--ok); color:var(--ok); }
.badge-warn { background:rgba(255,184,0,.12); border-color:var(--warn); color:var(--warn); }
.badge-danger { background:rgba(255,61,90,.12); border-color:var(--danger); color:var(--danger); }
.badge-info { background:rgba(77,166,255,.12); border-color:var(--info); color:var(--info); }
.badge-neutral { background:rgba(122,122,154,.12); border-color:var(--muted); color:var(--muted); }

/* Layout grid */
.grid { display:grid; gap:20px; }
@media (min-width: 1000px) {
  .grid-2 { grid-template-columns:1fr 1fr; }
  .grid-3 { grid-template-columns:1fr 1fr 1fr; }
  .grid-main { grid-template-columns:2fr 1fr; }
}

/* Cards / Panels */
.panel {
  background:var(--panel); border:1px solid var(--panel-border);
  border-radius:12px; overflow:hidden;
  transition:border-color .2s, box-shadow .2s;
}
.panel:hover { border-color:rgba(0,245,212,.18); box-shadow:0 0 0 1px rgba(0,245,212,.08); }
.panel-header {
  display:flex; align-items:center; justify-content:space-between;
  padding:14px 18px; border-bottom:1px solid var(--panel-border);
  background:rgba(0,0,0,.15);
}
.panel-header h2 { margin:0; font-size:.95rem; font-weight:600; letter-spacing:.01em; }
.panel-body { padding:16px 18px 18px; }

/* Timeline */
.timeline { position:relative; padding-left:24px; }
.timeline::before {
  content:''; position:absolute; left:6px; top:0; bottom:0; width:2px;
  background:linear-gradient(180deg,var(--accent),transparent);
}
.timeline-item {
  position:relative; padding:12px 0 12px 24px; border-radius:8px;
  background:rgba(255,255,255,.02); margin-bottom:10px;
  transition:background .2s;
}
.timeline-item:hover { background:rgba(0,245,212,.04); }
.timeline-item::before {
  content:''; position:absolute; left:-17px; top:18px; width:10px; height:10px;
  border-radius:50%; border:2px solid var(--panel-border); background:var(--panel);
  box-shadow:0 0 0 3px var(--bg); z-index:1;
}
.timeline-item.milestone::before { background:var(--accent); border-color:var(--accent); box-shadow:0 0 0 3px var(--bg), 0 0 12px var(--accent); }
.timeline-item.current::before { background:var(--info); border-color:var(--info); box-shadow:0 0 0 3px var(--bg), 0 0 12px var(--info); }
.timeline-item.completed::before { background:var(--ok); border-color:var(--ok); }
.timeline-item.pending::before { background:transparent; border-color:var(--muted); }
.timeline-meta { display:flex; gap:8px; align-items:center; margin-bottom:6px; font-size:.75rem; }
.timeline-title { font-weight:600; font-size:.9rem; }
.timeline-desc { color:var(--muted); font-size:.8rem; margin-top:2px; }

/* Progress Ring */
.progress-ring { width:48px; height:48px; transform:rotate(-90deg); }
.progress-ring-bg { fill:none; stroke:var(--panel-border); stroke-width:3.5; }
.progress-ring-fg {
  fill:none; stroke:var(--accent); stroke-width:3.5; stroke-linecap:round;
  stroke-dasharray:138; stroke-dashoffset:138; transition:stroke-dashoffset .6s ease;
}
.progress-ring-text { fill:var(--text); font-size:8px; dominant-baseline:middle; text-anchor:middle; }

/* Priority Alignment */
.priority-grid { display:grid; gap:10px; }
@media (min-width:600px) { .priority-grid { grid-template-columns:1fr 1fr; } }
.priority-card { background:rgba(255,255,255,.02); border:1px solid var(--panel-border); border-radius:8px; padding:12px; }
.priority-card.aligned { border-color:rgba(0,212,126,.4); background:rgba(0,212,126,.05); }
.priority-card.mismatch { border-color:rgba(255,184,0,.4); background:rgba(255,184,0,.05); }
.priority-label { font-size:.7rem; font-weight:600; text-transform:uppercase; letter-spacing:.05em; color:var(--muted); margin-bottom:6px; }
.priority-value { font-weight:600; font-size:.85rem; }
.priority-value.ok { color:var(--ok); }
.priority-value.warn { color:var(--warn); }
.priority-value.miss { color:var(--danger); }

/* Status indicators */
.status-dot { width:8px; height:8px; border-radius:50%; display:inline-block; margin-right:6px; }
.status-ok { background:var(--ok); box-shadow:0 0 8px var(--ok); }
.status-warn { background:var(--warn); box-shadow:0 0 8px var(--warn); }
.status-danger { background:var(--danger); box-shadow:0 0 8px var(--danger); }
.status-info { background:var(--info); box-shadow:0 0 8px var(--info); }
.status-pending { background:transparent; border:1px solid var(--muted); }

/* Tables */
table { width:100%; border-collapse:collapse; font-size:.8rem; }
th, td { padding:8px 10px; text-align:left; border-bottom:1px solid var(--panel-border); }
th { color:var(--muted); font-weight:600; font-size:.7rem; text-transform:uppercase; letter-spacing:.05em; }
tr:hover td { background:rgba(0,245,212,.04); transition:background .15s; }
td code { background:rgba(0,245,212,.1); color:var(--accent); padding:2px 6px; border-radius:4px; font-size:.75rem; }

/* Divider between major sections */
.section-divider {
  height:1px; margin:32px 0 20px;
  background:linear-gradient(90deg,transparent,var(--accent-dim),transparent);
  position:relative;
}
.section-divider::after {
  content:''; position:absolute; left:50%; top:50%; transform:translate(-50%,-50%);
  width:8px; height:8px; border-radius:50%; background:var(--accent);
  box-shadow:0 0 12px var(--accent);
}

/* Sticky section labels */
.section-label {
  font-size:.65rem; color:var(--muted); letter-spacing:.18em;
  font-weight:600; text-transform:uppercase;
  margin-bottom:8px; padding-left:6px; border-left:2px solid var(--accent-dim);
}

/* Glow pulse on key panels */
@keyframes pulse-glow {
  0%,100% { box-shadow:0 0 0 0 rgba(0,245,212,.0); }
  50% { box-shadow:0 0 0 4px rgba(0,245,212,.08); }
}
.panel.glow { animation:pulse-glow 3s ease-in-out infinite; }

/* Make table within .panel not double up borders */
.panel-body table { border:1px solid var(--panel-border); border-radius:6px; overflow:hidden; }
.panel-body table th:first-child { border-top-left-radius:6px; }
.panel-body table th:last-child { border-top-right-radius:6px; }

/* Markdown rendering helpers */
.markdown-body { line-height:1.6; }
.markdown-body h3 { font-size:1rem; margin:16px 0 8px; color:var(--accent); }
.markdown-body p { margin:8px 0; }
.markdown-body ul, .markdown-body ol { margin:8px 0 8px 20px; }
.markdown-body li { margin:4px 0; }
.markdown-body code { background:rgba(0,245,212,.1); color:var(--accent); padding:2px 6px; border-radius:4px; font-size:.85em; }
.markdown-body pre { background:#050508; border:1px solid var(--panel-border); border-radius:8px; padding:12px; overflow-x:auto; }
.markdown-body pre code { background:none; color:inherit; padding:0; }
.markdown-body hr { border:none; border-top:1px solid var(--panel-border); margin:16px 0; }
.markdown-body strong { color:var(--accent); }
.markdown-body blockquote { border-left:3px solid var(--accent); padding-left:12px; color:var(--muted); margin:12px 0; font-style:italic; }

/* Collapsible */
.collapsible { border:none; background:none; color:var(--muted); font-size:.75rem; cursor:pointer; padding:4px 0; display:flex; align-items:center; gap:6px; }
.collapsible:hover { color:var(--accent); }
.collapsible svg { transition:transform .2s; width:14px; height:14px; }
.collapsible[aria-expanded="true"] svg { transform:rotate(90deg); }
.collapsible-content { overflow:hidden; max-height:0; transition:max-height .3s ease; }
.collapsible-content.open { max-height:5000px; }

/* Footer */
footer { text-align:center; padding:24px; color:var(--muted); font-size:.75rem; }
"""


# ─── Markdown → HTML ──────────────────────────────────────────────────────
def md_to_html(md: str) -> str:
    """Minimal markdown → HTML for our known report formats."""
    if not md or md.startswith("_"):
        return f'<p class="muted">{md}</p>'
    html = md
    # Code fences first
    html = re.sub(r'```(\w+)?\n(.*?)```', r'<pre><code>\2</code></pre>', html, flags=re.S)
    # Headings
    html = re.sub(r'^###\s+(.+)$', r'<h3>\1</h3>', html, flags=re.M)
    html = re.sub(r'^##\s+(.+)$', r'<h3>\1</h3>', html, flags=re.M)
    # Bold
    html = re.sub(r'\*\*(.+?)\*\*', r'<strong>\1</strong>', html)
    # Inline code
    html = re.sub(r'`([^`]+)`', r'<code>\1</code>', html)
    # Tables
    def table_repl(m):
        rows = [r.strip() for r in m.group(0).strip().split('\n') if r.strip()]
        if len(rows) < 2: return m.group(0)
        header = [c.strip() for c in rows[0].split('|') if c.strip()]
        sep = rows[1]
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
    # Paragraphs
    blocks = html.split('\n\n')
    out = []
    for b in blocks:
        b = b.strip()
        if not b: continue
        if b.startswith('<'): out.append(b)
        else:
            b = b.replace('\n', '<br>')
            out.append(f'<p>{b}</p>')
    return '\n'.join(out)


def read_report(path: str) -> str:
    try:
        return Path(path).read_text(encoding='utf-8')
    except Exception:
        return f"_Report not found or unreadable: {path}_"


# ─── Data Extractors ──────────────────────────────────────────────────────
def extract_roadmap_milestones(roadmap_md: str):
    """Parse ROADMAP.md into structured milestones."""
    milestones = []
    current_quarter = None
    for line in roadmap_md.split('\n'):
        q_match = re.match(r'^###\s+(Q\d+:.*?)(?:\(Weeks?\s*[\d-]+\))?', line)
        if q_match:
            current_quarter = q_match.group(1).strip()
            continue
        m = re.match(r'^-\s*\[([ x])\]\s*(.+)', line)
        if m and current_quarter:
            done = m.group(1) == 'x'
            text = m.group(2).strip()
            # Extract milestone marker
            is_milestone = text.startswith('*Milestone:')
            clean = text.replace('*Milestone:*', '').strip()
            milestones.append({
                'quarter': current_quarter,
                'task': clean,
                'done': done,
                'milestone': is_milestone
            })
    return milestones


def extract_plan_tasks(plan_md: str):
    """Extract T1, T2, T3... tasks from PLAN.md."""
    tasks = []
    for m in re.finditer(r'###\s+(T\d+):\s*(.+?)\n(?:- Target:\s*(.+?)\n)?(?:- Action:\s*(.+?)\n)?(?:- Verify:\s*(.+?))?(?=\n###|\n##|\Z)', plan_md, re.S):
        tasks.append({
            'id': m.group(1),
            'title': m.group(2).strip(),
            'target': m.group(3).strip() if m.group(3) else '',
            'action': m.group(4).strip() if m.group(4) else '',
            'verify': m.group(5).strip() if m.group(5) else ''
        })
    return tasks


def extract_worker_status(worker_md: str):
    """Parse WORKER_STATUS.md table."""
    rows = []
    for m in re.finditer(r'\|\s*(\w+)\s*\|\s*([^|]+)\s*\|\s*(\w+)\s*\|\s*([^|]*)\s*\|', worker_md):
        rows.append({
            'id': m.group(1).strip(),
            'title': m.group(2).strip(),
            'status': m.group(3).strip().lower(),
            'time': m.group(4).strip()
        })
    return rows


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


def progress_ring_svg(percent: int) -> str:
    """Inline SVG progress ring."""
    offset = 138 - (138 * percent / 100)
    return f'''
    <svg class="progress-ring" viewBox="0 0 52 52" aria-label="Progress {percent}%">
      <circle class="progress-ring-bg" cx="26" cy="26" r="22"/>
      <circle class="progress-ring-fg" cx="26" cy="26" r="22"
              style="stroke-dashoffset:{offset:.1f}"/>
      <text class="progress-ring-text" x="26" y="27">{percent}%</text>
    </svg>'''


# ─── Main Generator ───────────────────────────────────────────────────────

def generate_html_dashboard(workspace_root: str):
    os.chdir(workspace_root)
    now = datetime.now(timezone.utc)
    timestamp = now.strftime("%Y-%m-%d %H:%M UTC")
    date_key = now.strftime("%Y-%m-%d")

    # Load reports
    reports = {
        'agent': read_report('docs/AGENT_REPORT.md'),
        'deps': read_report('docs/DEPENDENCY_AUDIT.md'),
        'health': read_report('docs/HEALTH_DASHBOARD.md'),
        'control': read_report('docs/CONTROL_CENTER.md'),
        'plan': read_report('docs/PLAN.md'),
        'review': read_report('docs/REVIEW.md'),
        'dispatch': read_report('docs/DISPATCH_LOG.md'),
        'workers': read_report('docs/WORKER_STATUS.md'),
        'roadmap': read_report('docs/ROADMAP.md'),
        'facts_raw': read_report('docs/FACTS.json'),
        'cron_status': read_report('docs/CRON_STATUS.md'),
        'backlog': read_report('docs/BACKLOG.md'),
        'promotion': read_report('docs/PROMOTION_LOG.md'),
        'research': read_report('docs/RESEARCH.md'),
        'pulse': read_report('docs/PULSE_LOG.md'),
        'health_loop': read_report('docs/HEALTH_LOOP.md'),
        'errors': read_report('docs/ERRORS.md'),
    }

    # Parse structured data
    facts_data = json.loads(reports['facts_raw']) if reports['facts_raw'] and not reports['facts_raw'].startswith("_Report not found") else {}

    milestones = extract_roadmap_milestones(reports['roadmap'])
    plan_tasks = extract_plan_tasks(reports['plan'])
    
    # Extract worker rows from FACTS.json if possible, fallback to WORKER_STATUS.md
    worker_rows = []
    if facts_data and "plan_review_worker_link" in facts_data and "WORKER_STATUS.md" in facts_data["plan_review_worker_link"]:
        worker_rows = extract_worker_status(facts_data["plan_review_worker_link"]["WORKER_STATUS.md"])
    else:
        worker_rows = extract_worker_status(reports['workers'])

    # Use review_statuses from FACTS.json
    review_statuses = facts_data.get('review_statuses', {})
    
    # Dead links from FACTS.json
    dead_links = facts_data.get('dead_links', [])

    # Build timeline items: milestones + today's tasks
    timeline_items = []
    for ms in milestones:
        cls = 'completed' if ms['done'] else 'pending'
        if ms['milestone']: cls = 'milestone'
        timeline_items.append({
            'label': ms['quarter'],
            'title': ms['task'],
            'desc': 'Milestone' if ms['milestone'] else 'Task',
            'class': cls,
            'time': ''
        })

    # Add today's tasks as current sprint
    for t in plan_tasks:
        status = review_statuses.get(t['id'], 'pending') # Get status from REVIEW.md via facts_data
        cls = {'completed':'completed', 'in_progress':'current', 'pending':'current', 'failed':'danger', 'partial':'warn', 'not_started':'pending'}.get(status, 'current')
        timeline_items.append({
            'label': 'Today',
            'title': f"{t['id']}: {t['title']}",
            'desc': t['action'] or t['target'],
            'class': cls,
            'time': '' # Status and time are now derived from review_statuses
        })

    # Priority alignment: compare roadmap Q1 tasks vs today's plan
    q1_tasks = [m['task'] for m in milestones if m['quarter'].startswith('Q1') and not m['milestone']]
    plan_titles = [t['title'] for t in plan_tasks]
    alignment = []
    for qt in q1_tasks[:6]:
        matched = any(qt.lower() in pt.lower() or pt.lower() in qt.lower() for pt in plan_titles)
        alignment.append({'roadmap': qt, 'aligned': matched})
    for pt in plan_titles:
        if not any(pt.lower() in a['roadmap'].lower() or a['roadmap'].lower() in pt.lower() for a in alignment):
            alignment.append({'roadmap': pt, 'aligned': False, 'extra': True})

    # Quick facts from facts_data
    rust_files = facts_data.get('codebase', {}).get('rust_files', 0)
    loc = facts_data.get('codebase', {}).get('lines_of_code', 0)
    clippy = facts_data.get('codebase', {}).get('clippy_warnings', 0)
    todo_count = len(facts_data.get('todos', []))
    roadmap_open = len(facts_data.get('roadmap_open_tasks', []))
    deps = len(facts_data.get('dependencies', []))
    dead_link_count = len(dead_links)

    # Worker progress %
    completed_workers = sum(1 for status in review_statuses.values() if status == 'completed')
    total_workers = len(plan_tasks) or 1 # Base on planned tasks
    worker_pct = int(completed_workers / total_workers * 100)

    # Backlog progress %
    backlog_remaining = facts_data.get('backlog_remaining')
    backlog_total = 50
    if backlog_remaining is None:
        backlog_remaining = 50
    backlog_done = backlog_total - backlog_remaining
    backlog_pct = int(backlog_done / backlog_total * 100)

    # Cron jobs: load structured JSON snapshot from CRON_STATUS.json (live state)
    cron_rows = []
    cron_statuses = {'ok': 0, 'error': 0, 'running': 0, 'pending': 0, 'scheduled': 0, 'unknown': 0}
    try:
        cron_json_path = Path('docs/CRON_STATUS.json')
        if cron_json_path.exists():
            with open(cron_json_path, encoding='utf-8') as f:
                cron_data = json.load(f)
            for job in cron_data.get('jobs', []):
                cron_rows.append({
                    'name': job.get('name', ''),
                    'schedule': job.get('schedule', ''),
                    'status': job.get('status', 'unknown'),
                    'execution': job.get('execution', ''),
                    'last_error': job.get('last_error', ''),
                    'last_run': job.get('last_run', ''),
                })
                cron_statuses[job.get('status', 'unknown')] = cron_statuses.get(job.get('status', 'unknown'), 0) + 1
    except Exception as e:
        cron_rows = []
    cron_total = len(cron_rows)
    cron_ok = cron_statuses['ok']

    # Project name override (production-grade: anything goes)
    project_name = os.environ.get('DASHBOARD_PROJECT_NAME', 'RamShield Automation')

    # ─── HTML Assembly ──────────────────────────────────────────────────
    html = f'''<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{project_name} Dashboard</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Space+Grotesk:wght@400,500,600,700&display=swap" rel="stylesheet">
<style>{CSS}</style>
</head>
<body>
<div class="container">
  <header>
    <h1>{project_name} Control Center</h1>
    <p class="subtitle">Generated {timestamp}  •  Branch: <code>{facts_data.get('git',{}).get('branch','?')}</code>  •  Commit: <code>{facts_data.get('git',{}).get('commit_short','?')}</code></p>
    <div class="badge-row">
      <span class="badge badge-ok">Rust Files: {rust_files}</span>
      <span class="badge badge-info">LOC: {loc:,}</span>
      <span class="badge {'badge-ok' if clippy==0 else 'badge-warn'}">Clippy: {clippy}</span>
      <span class="badge badge-neutral">TODOs: {todo_count}</span>
      <span class="badge badge-info">Roadmap: {roadmap_open}</span>
      <span class="badge badge-neutral">Deps: {deps}</span>
      <span class="badge badge-{'ok' if dead_link_count==0 else 'danger'}">Dead Links: {dead_link_count}</span>
      <span class="badge badge-info">Cron: {cron_ok}/{cron_total}</span>
      <span class="badge badge-neutral">Backlog: {backlog_remaining}/{backlog_total}</span>
      <span class="badge badge-{'ok' if worker_pct==100 else 'info'}">Workers: {worker_pct}% ({completed_workers}/{total_workers})</span>
    </div>
  </header>

  <!-- MAIN TIMELINE -->
  <section class="panel glow" style="margin-bottom:24px;">
    <div class="panel-header">
      <h2>📅 Main Timeline — Roadmap → Sprint → Today</h2>
      <span class="badge badge-info">Auto-synced from ROADMAP.md + PLAN.md</span>
    </div>
    <div class="panel-body">
      <div class="timeline">
'''
    for i, item in enumerate(timeline_items):
        html += f'''
        <div class="timeline-item {item['class']}">
          <div class="timeline-meta">
            <span>{item['label']}</span>
            <span style="color:var(--muted);">{item['time']}</span>
          </div>
          <div class="timeline-title">{item['title']}</div>
          <div class="timeline-desc">{item['desc']}</div>
        </div>'''

    html += f'''
      </div>
    </div>
  </section>

  <!-- GRID: Priority Alignment + Progress + Quick Stats -->
  <div class="grid grid-main">
    <!-- Priority Alignment -->
    <section class="panel">
      <div class="panel-header">
        <h2>🎯 Priority Alignment</h2>
        <span class="badge badge-{'ok' if all(a['aligned'] for a in alignment if not a.get('extra')) else 'warn'}">Roadmap ↔ Daily Plan</span>
      </div>
      <div class="panel-body">
        <div class="priority-grid">
'''

    for a in alignment:
        cls = 'aligned' if a['aligned'] else ('mismatch' if not a.get('extra') else '')
        badge = 'ok' if a['aligned'] else ('warn' if not a.get('extra') else 'info')
        label = 'Roadmap → Today' if not a.get('extra') else 'Extra in Today'
        html += f'''
        <div class="priority-card {cls}">
          <div class="priority-label">{label}</div>
          <div class="priority-value {('ok' if a['aligned'] else 'warn' if not a.get('extra') else 'info')}">{a['roadmap'][:80]}{'…' if len(a['roadmap'])>80 else ''}</div>
          <span class="badge badge-{badge}" style="margin-top:6px;display:inline-block;font-size:.6rem;">{'Aligned' if a['aligned'] else 'Mismatch' if not a.get('extra') else 'Only in Plan'}</span>
        </div>'''

    html += f'''
        </div>
      </div>
    </section>

    <!-- Progress & Quick Stats -->
    <section class="panel">
      <div class="panel-header"><h2>📊 Cycle Progress</h2></div>
      <div class="panel-body" style="display:flex;flex-direction:column;gap:16px;align-items:center;">
        <div style="text-align:center;">
          <div style="font-size:.75rem;color:var(--muted);margin-bottom:8px;">Worker Completion</div>
          {progress_ring_svg(worker_pct)}
        </div>
        <div style="display:grid;grid-template-columns:1fr 1fr;gap:12px;width:100%;max-width:320px;">
          <div class="badge badge-ok" style="justify-content:center;font-size:.8rem;">✅ {completed_workers} Done</div>
          <div class="badge badge-info" style="justify-content:center;font-size:.8rem;">⏳ {total_workers - completed_workers} Pending</div>
          <div class="badge badge-neutral" style="justify-content:center;font-size:.8rem;">📝 {len(plan_tasks)} Planned</div>
          <div class="badge badge-info" style="justify-content:center;font-size:.8rem;">🗺️ {roadmap_open} Roadmap</div>
        </div>
      </div>
    </section>
  </div>

  <!-- 3-COLUMN OPS GRID: Cron | Backlog | Pulse -->
  <div class="section-divider"></div>
  <div class="section-label">Operations</div>
  <div class="grid grid-3" style="margin-top:20px;">
    <!-- Cron Jobs -->
    <section class="panel">
      <div class="panel-header">
        <h2>⏰ Cron Jobs</h2>
        <span class="badge badge-info">{cron_total} active</span>
      </div>
      <div class="panel-body">
        <div style="display:flex;gap:8px;flex-wrap:wrap;margin-bottom:12px;">
          <span class="badge badge-ok">OK: {cron_statuses['ok']}</span>
          <span class="badge badge-danger">Errors: {cron_statuses['error']}</span>
          <span class="badge badge-info">Running: {cron_statuses['running']}</span>
          <span class="badge badge-warn">Pending: {cron_statuses['pending']}</span>
          <span class="badge badge-neutral">Scheduled: {cron_statuses['scheduled']}</span>
        </div>
        <table>
          <thead><tr><th>Job</th><th>Sched</th><th>Status</th><th>Execution</th></tr></thead>
          <tbody>
'''

    for c in cron_rows:
        name = c["name"]
        sched = c["schedule"]
        st = c["status"].lower()
        bg = {"ok": "badge-ok", "error": "badge-danger", "running": "badge-info", "pending": "badge-warn", "scheduled": "badge-neutral"}.get(st, "badge-neutral")
        title_attr = f"title=\"{c['last_error'][:120]}\"" if c.get("last_error") else ""
        html += f'''
            <tr {title_attr}>
              <td>{name}</td>
              <td><code>{sched}</code></td>
              <td><span class="badge {bg}">{c["status"]}</span></td>
              <td><span style="color:var(--muted);font-size:.7rem;">{c["execution"]}</span></td>
            </tr>'''

    if not cron_rows:
        html += '<tr><td colspan="4" style="color:var(--muted);text-align:center;">No cron data yet — first run collector.</td></tr>'

    html += f'''
          </tbody>
        </table>
      </div>
    </section>

    <!-- Backlog -->
    <section class="panel">
      <div class="panel-header">
        <h2>📦 Atomic Backlog</h2>
        <span class="badge badge-neutral">{backlog_remaining}/{backlog_total} left</span>
      </div>
      <div class="panel-body">
        <div style="display:flex;align-items:center;gap:16px;margin-bottom:14px;">
          <div style="position:relative;width:54px;height:54px;">
            {progress_ring_svg(backlog_pct)}
          </div>
          <div>
            <div style="font-weight:600;font-size:1.1rem;">{backlog_done} / {backlog_total}</div>
            <div style="color:var(--muted);font-size:.8rem;">atomic jobs picked up</div>
          </div>
        </div>
        <div style="font-size:.8rem;color:var(--muted);">P0 → P3 priority queue. Pulse agent picks top unfinished item every 5 min.</div>
        <details style="margin-top:8px;">
          <summary style="cursor:pointer;color:var(--accent);font-size:.75rem;">View top 10</summary>
'''
    bg_items = re.findall(r'^\d+\.\s+\[[ x]\]\s+([^.\n]+?)(?:\.|$)', reports['backlog'], re.M)
    for line in bg_items[:10]:
        html += f'<div style="font-size:.75rem;padding:4px 0;color:var(--muted);">• {line}</div>'
    html += '''
        </details>
      </div>
    </section>

    <!-- Pulse -->
    <section class="panel">
      <div class="panel-header">
        <h2>💓 Pulse (5m)</h2>
        <span class="badge badge-info">every 5m</span>
      </div>
      <div class="panel-body">
        <div style="font-size:.8rem;color:var(--muted);margin-bottom:10px;">High-frequency pickup from backlog. Last entry:</div>
'''

    pulse_lines = [l for l in reports['pulse'].split('\n') if l.strip() and not l.startswith('#') and 'No pulse' not in l]
    last_pulse = pulse_lines[-1] if pulse_lines else '_Awaiting first pulse tick._'
    html += f'<div style="background:rgba(0,245,212,.05);border-left:3px solid var(--accent);padding:10px 12px;border-radius:4px;font-family:monospace;font-size:.85rem;">{last_pulse}</div>'

    html += f'''
      </div>
    </section>
  </div>

  <!-- 2-COLUMN OPS GRID: Promotion | Research -->
  <div class="section-divider"></div>
  <div class="section-label">Growth & Discovery</div>
  <div class="grid grid-2" style="margin-top:20px;">
    <!-- Promotion -->
    <section class="panel">
      <div class="panel-header">
        <h2>📣 Promotion</h2>
        <span class="badge badge-info">30m cadence</span>
      </div>
      <div class="panel-body">
        {md_to_html(reports['promotion'])}
      </div>
    </section>

    <!-- Research -->
    <section class="panel">
      <div class="panel-header">
        <h2>🔬 Research</h2>
        <span class="badge badge-info">hourly</span>
      </div>
      <div class="panel-body">
        {md_to_html(reports['research'])}
      </div>
    </section>
  </div>

  <!-- 2-COLUMN OPS GRID: Health Loop | Backlog full -->
  <div class="section-divider"></div>
  <div class="section-label">Health & Backlog Detail</div>
  <div class="grid grid-2" style="margin-top:20px;">
    <section class="panel">
      <div class="panel-header">
        <h2>🏥 Health Loop (15m)</h2>
        <span class="badge badge-info">read-only</span>
      </div>
      <div class="panel-body">
        {md_to_html(reports['health_loop'])}
      </div>
    </section>

    <section class="panel">
      <div class="panel-header">
        <h2>📦 Backlog (full)</h2>
        <span class="badge badge-neutral">{backlog_remaining}/{backlog_total}</span>
      </div>
      <div class="panel-body">
        {md_to_html(reports['backlog'])}
      </div>
    </section>
  </div>

  <!-- ERRORS & ERROR-HANDLING -->
  <section class="panel" style="margin-top:20px;border-left:3px solid var(--danger);">
    <div class="panel-header">
      <h2>🚨 Errors &amp; Error-Handling</h2>
      <span class="badge badge-{'ok' if not reports['errors'].startswith('_') and '0 active' in reports['errors'] else 'danger'}">{('no active errors' if not reports['errors'].startswith('_') and '0 active' in reports['errors'] else 'review')}</span>
    </div>
    <div class="panel-body">
      {md_to_html(reports['errors']) if not reports['errors'].startswith('_') else md_to_html('_No active errors tracked. Health-repair job monitoring all sections._')}
    </div>
  </section>

  <!-- DEAD LINKS REPORT -->
  <section class="panel" style="margin-top:20px;">
    <div class="panel-header">
      <h2>🔗 Dead Links Report</h2>
      <span class="badge badge-{'ok' if dead_link_count == 0 else 'danger'}">{dead_link_count} Dead Links</span>
    </div>
    <div class="panel-body">
      {md_to_html(chr(10).join(dead_links)) if dead_links else md_to_html('_No dead links found._')}
    </div>
  </section>

  <!-- DETAIL PANELS -->
  <div class="grid grid-2" style="margin-top:20px;">
    <section class="panel">
      <div class="panel-header"><h2>📋 Daily Work Plan</h2></div>
      <div class="panel-body">{md_to_html(reports['plan'])}</div>
    </section>
    <section class="panel">
      <div class="panel-header"><h2>👷 Worker Status</h2></div>
      <div class="panel-body">{md_to_html(reports['workers'])}</div>
    </section>
    <section class="panel">
      <div class="panel-header"><h2>🔍 Review & Assessment</h2></div>
      <div class="panel-body">{md_to_html(reports['review'])}</div>
    </section>
    <section class="panel">
      <div class="panel-header"><h2>📤 Task Dispatch Log</h2></div>
      <div class="panel-body">{md_to_html(reports['dispatch'])}</div>
    </section>
    <section class="panel">
      <div class="panel-header"><h2>🏥 Project Health Overview</h2></div>
      <div class="panel-body">{md_to_html(reports['health'])}</div>
    </section>
    <section class="panel">
      <div class="panel-header"><h2>🤖 Agent Control Center</h2></div>
      <div class="panel-body">{md_to_html(reports['control'])}</div>
    </section>
    <section class="panel">
      <div class="panel-header"><h2>📦 Dependency Audit</h2></div>
      <div class="panel-body">{md_to_html(reports['deps'])}</div>
    </section>
    <section class="panel">
      <div class="panel-header"><h2>🤖 Helper Agent Latest Report</h2></div>
      <div class="panel-body">{md_to_html(reports['agent'])}</div>
    </section>
  </div>

  <footer>
    Generated by {project_name} Dashboard Generator v0.6.0  •  Slick & Edgy Theme
    •  Docs: <a href="https://hermes-agent.nousresearch.com/docs">https://hermes-agent.nousresearch.com/docs</a>
  </footer>
</div>
</body>
</html>'''

    out = Path('docs') / 'AUTOMATION_DASHBOARD.html'
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(html, encoding='utf-8')
    print(f"Generated HTML dashboard: {out} ({len(html):,} bytes)")


def main():
    workspace = os.environ.get(
        'GITHUB_WORKSPACE',
        str(Path(__file__).parent.parent.parent)
    )
    generate_html_dashboard(workspace)


if __name__ == '__main__':
    main()