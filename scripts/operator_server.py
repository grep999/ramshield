#!/usr/bin/env python3
"""RamShield operator dashboard — read-only observer + thin action layer.

Stdlib only. Binds 127.0.0.1:9777.
Endpoints:
  GET  /                 one-page UI (polls every 5s)
  GET  /api/fleet        CRON_STATUS.json passthrough
  GET  /api/log          OPERATOR_LOG.md tail (?n=50)
  GET  /api/engine       proxy :9999/api/snapshot
  GET  /api/git          branch, HEAD, dirty count
  GET  /api/promo        promo output counts per campaign
  GET  /api/bench        last bench log tail
  POST /api/run/<job_id> trigger hermes cron run <id>
  POST /api/regen        facts + cron-status + html dashboard regen
"""
import json
import os
import re
import subprocess
import sys
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CRON_JSON = REPO / "docs" / "CRON_STATUS.json"
OPLOG = REPO / "docs" / "OPERATOR_LOG.md"
PROMO_DIR = Path.home() / "promotion_content"
BENCH_LOG = Path("/tmp/subnet_bench.log")
ENGINE_URL = "http://127.0.0.1:9999/api/snapshot"
HOST = os.environ.get("OP_BIND", "127.0.0.1")
PORT = int(os.environ.get("OP_PORT", "9777"))
HERMES = os.environ.get("HERMES_BIN", "hermes")

UI = r"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>RamShield Operator</title>
<style>
:root{--bg:#07070b;--sf:#0f0f16;--sf2:#15151f;--bd:#252536;--tx:#f0f0f7;--mu:#8b8ba7;
--ac:#00f5d4;--ok:#00d47e;--wr:#ffb800;--er:#ff3d5a;--in:#4da6ff}
*{box-sizing:border-box;margin:0}
body{background:var(--bg);color:var(--tx);font:14px/1.5 ui-monospace,'JetBrains Mono',monospace;padding:16px}
h1{font-size:15px;letter-spacing:.08em;color:var(--ac);margin-bottom:12px;text-transform:uppercase}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(320px,1fr));gap:12px;margin-bottom:14px}
.card{background:var(--sf);border:1px solid var(--bd);border-radius:8px;padding:12px}
.card h2{font-size:11px;color:var(--mu);text-transform:uppercase;letter-spacing:.1em;margin-bottom:8px}
.big{font-size:22px;font-weight:700}
.ok{color:var(--ok)}.err{color:var(--er)}.run{color:var(--in)}.sch{color:var(--wr)}
table{width:100%;border-collapse:collapse;font-size:12px}
th{color:var(--mu);text-align:left;font-weight:500;border-bottom:1px solid var(--bd);padding:4px 6px}
td{padding:3px 6px;border-bottom:1px solid #1b1b27;white-space:nowrap}
tr:hover td{background:var(--sf2)}
button{background:transparent;border:1px solid var(--bd);color:var(--ac);border-radius:4px;
padding:1px 8px;cursor:pointer;font:inherit;font-size:11px}
button:hover{border-color:var(--ac);box-shadow:0 0 8px rgba(0,245,212,.25)}
.logbox{max-height:260px;overflow-y:auto;font-size:12px;line-height:1.6}
.logbox div{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.mut{color:var(--mu)} .dim{color:#5c5c75}
#toast{position:fixed;bottom:16px;right:16px;background:var(--sf2);border:1px solid var(--ac);
padding:8px 14px;border-radius:6px;display:none;z-index:9}
.bar{display:flex;height:10px;border-radius:5px;overflow:hidden;background:var(--sf2);margin:6px 0}
.bar span{display:block;height:100%}
</style>
</head>
<body>
<h1>⬡ ramshield operator</h1>
<div class="grid">
  <div class="card"><h2>Fleet</h2><div id="fleet">…</div></div>
  <div class="card"><h2>Engine (:9999)</h2><div id="engine">…</div></div>
  <div class="card"><h2>Git</h2><div id="git">…</div></div>
  <div class="card"><h2>Bench (last)</h2><div id="bench">…</div></div>
</div>
<div class="grid" style="grid-template-columns:2fr 1fr">
  <div class="card"><h2>Jobs <span class="mut" id="jcount"></span> &nbsp;<button onclick="regen()">↻ regen docs</button></h2>
    <div style="max-height:420px;overflow-y:auto"><table id="jobs"></table></div></div>
  <div class="card"><h2>Promo outputs</h2><div id="promo">…</div></div>
</div>
<div class="card"><h2>Operator log (newest top) <button onclick="loadAll()">↻</button></h2>
  <div class="logbox" id="log"></div></div>
<div id="toast"></div>
<script>
const $=id=>document.getElementById(id);
function toast(m){const t=$('toast');t.textContent=m;t.style.display='block';setTimeout(()=>t.style.display='none',2500)}
async function jget(u){const r=await fetch(u);if(!r.ok)throw 0;return r.json()}
function esc(s){return String(s).replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]))}

async function loadFleet(){
  try{
    const d=await jget('/api/fleet');
    const jobs=d.jobs||[];
    const n=s=>jobs.filter(j=>j.status===s).length;
    const ok=n('ok')+n('running'), sch=n('scheduled'), err=n('error');
    $('fleet').innerHTML=`<div class="big"><span class="ok">${ok}</span>
      <span class="mut">/ ${jobs.length}</span> healthy
      ${err?`<span class="err">${err} err</span>`:''} ${sch?`<span class="sch">${sch} sched</span>`:''}</div>
      <div class="bar"><span class="ok" style="width:${ok/Math.max(jobs.length,1)*100}%"></span>
      <span class="sch" style="width:${sch/Math.max(jobs.length,1)*100}%"></span>
      <span class="err" style="width:${err/Math.max(jobs.length,1)*100}%"></span></div>
      <div class="dim">snapshot ${esc((d.generated_at||'').slice(0,19))}</div>`;
    $('jcount').textContent=`(${jobs.length})`;
    $('jobs').innerHTML='<tr><th>job</th><th>schedule</th><th>status</th><th>last error</th><th></th></tr>'+
      jobs.slice().sort((a,b)=>(a.status==='error'?-1:0)+(b.status==='error'?1:0)).map(j=>{
        const cls=j.status==='error'?'err':(j.status==='running'?'run':(j.status==='scheduled'?'sch':'ok'));
        const e=(j.last_error||'').splitlines?j.last_error:(j.last_error||'');
        const first=e?String(e).split('\n')[0].slice(0,60):'';
        return `<tr title="${esc(String(j.last_error||''))}">
          <td>${esc(j.name)}</td><td class="mut">${esc(j.schedule)}</td>
          <td class="${cls}">${j.status}</td><td class="dim">${esc(first)}</td>
          <td><button onclick="runJob('${j.job_id}','${esc(j.name)}')">run</button></td></tr>`}).join('');
  }catch(e){$('fleet').innerHTML='<span class="err">fleet snapshot unavailable</span>'}
}
async function loadEngine(){
  try{
    const d=await jget('/api/engine');
    $('engine').innerHTML=`<div class="big ${d.is_healthy?'ok':'err'}">${d.is_healthy?'HEALTHY':'DOWN'}</div>
      <div>blocked <b>${d.pipeline?.blocked??'—'}</b></div>
      <div>ingested <b>${(d.events_ingested??0).toLocaleString()}</b></div>
      <div>cpu <b>${d.cpu_usage?.toFixed(0)??'—'}%</b> mem <b>${d.memory_usage_mb??'—'}MB</b></div>
      <div class="dim">uptime ${Math.floor((d.uptime_secs??0)/60)}m</div>`;
  }catch(e){$('engine').innerHTML='<span class="dim">engine down / not running</span>'}
}
async function loadGit(){
  try{const d=await jget('/api/git');
    $('git').innerHTML=`<div class="big">${esc(d.branch)||'?'}</div>
      <div>${esc(d.head)}</div>
      <div class="${d.dirty?'sch':'ok'}">${d.dirty} dirty files</div>`;}
  catch(e){$('git').innerHTML='—'}
}
async function loadBench(){
  try{const d=await jget('/api/bench');
    $('bench').innerHTML=d.line?`<div>${esc(d.line)}</div>`:'<span class="dim">no runs yet</span>';}
  catch(e){$('bench').innerHTML='<span class="dim">no runs yet</span>'}
}
async function loadPromo(){
  try{const d=await jget('/api/promo');
    const rows=Object.entries(d).sort((a,b)=>b[1]-a[1]).slice(0,10)
      .map(([k,v])=>`<div>${esc(k)} <span class="mut" style="float:right">${v}</span></div>`).join('');
    $('promo').innerHTML=rows||'<span class="dim">no outputs</span>';}
  catch(e){$('promo').innerHTML='—'}
}
async function loadLog(){
  try{const d=await jget('/api/log?n=40');
    $('log').innerHTML=(d.lines||[]).reverse().map(l=>{
      const c=l.includes('error')||l.includes('fail')?'err':'';
      return `<div class="${c}">${esc(l)}</div>`}).join('');}
  catch(e){$('log').innerHTML='<div class="dim">no log</div>'}
}
async function runJob(id,name){
  if(!confirm(`Trigger "${name}" now?`))return;
  const r=await fetch('/api/run/'+id,{method:'POST'});
  toast(r.ok?`triggered ${name}`:`failed (${r.status})`);
  setTimeout(loadFleet,3000);
}
async function regen(){
  toast('regen started…');
  await fetch('/api/regen',{method:'POST'});
  toast('docs regenerated');setTimeout(loadAll,2000);
}
function loadAll(){loadFleet();loadEngine();loadGit();loadBench();loadPromo();loadLog()}
loadAll();setInterval(loadAll,5000);
</script>
</body>
</html>"""


def _sh(*args, cwd=None):
    return subprocess.run(args, capture_output=True, text=True, timeout=30, cwd=cwd)


class Handler(BaseHTTPRequestHandler):
    def _send(self, code, body, ctype="application/json"):
        payload = body.encode() if isinstance(body, str) else body
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def _json(self, obj, code=200):
        self._send(code, json.dumps(obj))

    def do_GET(self):
        path, _, qs = self.path.partition("?")
        params = dict(p.split("=", 1) for p in qs.split("&") if "=" in p) if qs else {}
        if path == "/":
            return self._send(200, UI, "text/html; charset=utf-8")
        if path == "/api/fleet":
            try:
                return self._json(json.load(open(CRON_JSON)))
            except Exception as e:
                return self._json({"error": str(e)}, 503)
        if path == "/api/log":
            n = min(int(params.get("n", "50")), 500)
            try:
                lines = open(OPLOG, errors="replace").read().splitlines()[-n:]
                return self._json({"lines": lines})
            except FileNotFoundError:
                return self._json({"lines": []})
        if path == "/api/engine":
            try:
                with urllib.request.urlopen(ENGINE_URL, timeout=3) as r:
                    return self._json(json.load(r))
            except Exception:
                return self._json({"is_healthy": False}, 200)
        if path == "/api/git":
            branch = _sh("git", "-C", str(REPO), "branch", "--show-current").stdout.strip()
            head = _sh("git", "-C", str(REPO), "log", "-1", "--format=%h %s").stdout.strip()
            dirty = _sh("git", "-C", str(REPO), "status", "--short").stdout.count("\n")
            return self._json({"branch": branch, "head": head, "dirty": dirty})
        if path == "/api/promo":
            out = {}
            if PROMO_DIR.exists():
                for d in PROMO_DIR.iterdir():
                    if d.is_dir():
                        out[d.name] = sum(1 for f in d.glob("*.md"))
            return self._json(out)
        if path == "/api/bench":
            line = ""
            if BENCH_LOG.exists():
                m = [l for l in open(BENCH_LOG, errors="replace").read().splitlines()
                     if l.startswith("done:")]
                line = m[-1] if m else ""
            return self._json({"line": line})
        return self._json({"error": "not found"}, 404)

    def do_POST(self):
        # loopback-only guard at request layer too
        if self.client_address[0] not in ("127.0.0.1", "::1"):
            return self._json({"error": "forbidden"}, 403)
        if self.path.startswith("/api/run/"):
            job_id = self.path.rsplit("/", 1)[-1]
            if not re.fullmatch(r"[0-9a-f]{12}", job_id):
                return self._json({"error": "bad job_id"}, 400)
            r = _sh(HERMES, "cron", "run", job_id)
            ok = r.returncode == 0 and "not found" not in (r.stdout + r.stderr).lower()
            return self._json({"ok": ok, "out": (r.stdout or r.stderr)[:200]}, 200 if ok else 500)
        if self.path == "/api/regen":
            scripts = [
                ".github/scripts/facts_collector.py",
                ".github/scripts/cron_status_collector.py",
                ".github/scripts/html_dashboard_generator.py",
            ]
            results = {}
            for s in scripts:
                p = REPO / s
                if p.exists():
                    r = _sh(sys.executable, str(p), cwd=str(REPO))
                    results[s.rsplit('/', 1)[-1]] = r.returncode
            ok = all(v == 0 for v in results.values())
            return self._json({"ok": ok, "results": results}, 200 if ok else 500)
        return self._json({"error": "not found"}, 404)

    def log_message(self, *args, **kwargs):  # silence per-request stderr spam
        pass


def main():
    srv = ThreadingHTTPServer((HOST, PORT), Handler)
    print(f"operator dashboard → http://{HOST}:{PORT}")
    srv.serve_forever()


if __name__ == "__main__":
    main()
