# RamShield Operator — Documentation

Complete reference for the autonomous operator setup: cron fleet, dashboards,
bench harness, maintenance procedures. Live state lives in `docs/CRON_STATUS.md`
(regenerated every 5 min); this doc explains what exists and how to run it.

---

## 1. What the operator is

A set of Hermes cronjobs + scripts that run RamShield's project work autonomously:
planning → dispatching → worker agents → review → healing → git automation → promotion.
Humans supervise via two dashboards; the agent loop handles the rest.

Key files:

| File | Role |
|---|---|
| `docs/OPERATOR_LOG.md` | Append-only event stream (cron-status, healer, helper). Newest at bottom. |
| `docs/CRON_STATUS.md` / `.json` | Live cron snapshot, regenerated every 5 min by `ramshield-cron-status`. JSON is gitignored. |
| `docs/AUTOMATION_DASHBOARD.html` | Operator console UI. Regenerate: `python3 .github/scripts/html_dashboard_generator.py`. |
| `docs/FACTS.json` | Codebase facts snapshot (git state, metrics, roadmap tasks). Consumed by planner/healer. |
| `~/.hermes/cron/jobs.json` | Source of truth for job definitions (model pin, schedule, prompt). |
| `~/.hermes/scripts/` | Runtime copies of all operator scripts. |

---

## 2. Cron fleet (27 active jobs)

### 2.1 Agent pipeline (LLM-driven)

| Job | Schedule | Purpose |
|---|---|---|
| `ramshield-daily-planner` | 01:00 | Reads FACTS.json + backlog, writes daily PLAN.md with prioritized tasks. |
| `ramshield-dispatcher` | 01:30 | Creates one temp cron worker per plan task (analyze→solve→verify pattern). |
| `ramshield-pulse` | */5 min | Picks highest-priority P0 backlog item, executes small fixes. |
| `ramshield-helper-agent` | */10 min | Metrics scan, TODO/FIXME sweep, AGENT_REPORT.md refresh. |
| `ramshield-research-agent` | hourly | Feeds RESEARCH.md: new libs, papers, competitor moves. |
| `ramshield-reviewer` | 03:00 | Reviews worker output from today's plan; flags regressions. |
| `RamShield Promotion Agent` | 09:00 | Long-horizon promo work in `/home/m/out/ramshield_promotion`. |

All pinned to `provider=custom, model=zombobobo` (2026-08-22) after a global
config drift left them skip-locked. To re-pin after future drifts:

```bash
hermes cron edit <job_id> --provider custom --model zombobobo
```

### 2.2 Maintenance loop (no-agent scripts)

| Job | Schedule | Script | Purpose |
|---|---|---|---|
| `ramshield-cron-status` | */5 min | `cron_status_collector.py` | Snapshot cron state into docs. |
| `ramshield-health-loop` | */15 min | `health_check_repair.py` | Read-only sanity: facts age, frozen docs, missing files. |
| `ramshield-health-repair` | hourly | same script, repair mode | Auto-fixes: regen dashboard, symlink cleanup, FACTS regen. |
| `ramshield-git-automation` | */15 min | `git_automation.py` | Auto-commit + push dirty docs/state. |
| `ramshield-error-healer` | */30 min | `ramshield_error_healer.sh` | Detects recurring errors, spawns analyze/solve/verify heal jobs. |
| `ramshield-facts-collector` | */30 min | `facts_collector.py` | Refreshes FACTS.json. |
| `ramshield-backup` | 02:00 | `backup_project.sh` | Timestamped project backup, keeps last 2. |
| scalper jobs (3) | hourly/06:00/daily | `scalper.py` | LLM model discovery + combo updates in 9router (unrelated to RamShield code; shares the scheduler). |

### 2.3 Promotion batch (10 jobs, no-agent)

All wrappers exec `~/.hermes/scripts/promo_batch.py` with `PROMO_CAMPAIGN_ID`.
Output lands in `~/promotion_content/<campaign>/`.

| Frequency | Jobs |
|---|---|
| */5 min | qw-github-topics, qw-awesome-rust, qw-crates-io |
| */10 min | fast-reddit, fast-x |
| */15 min | std-devto, std-hn |
| */30 min | deep-blog, deep-rust-weekly, + promo-reviewer |
| hourly | strategic-plan |

Campaign definitions: `~/promotion_content/campaigns.json`.
**Gotcha (fixed 2026-08-22):** wrappers once pointed at `<repo>/promo_batch.py`
which doesn't exist → exit 2 every tick. Real location is
`~/.hermes/scripts/promo_batch.py`. If wrappers break again, check path first.

---

## 3. Dashboards

### 3.1 AUTOMATION_DASHBOARD.html (operator console)

Static HTML, dark theme, sections: Overview / Operations / Log / Job Chain /
Modules / Health / Growth / Backlog / Systems.

Regenerate after meaningful state changes:

```bash
python3 .github/scripts/html_dashboard_generator.py   # from repo root
```

Also auto-regenerated hourly by `health-repair` when stale (>30 min).

### 3.2 Live dashboard (`scripts/operator_server.py`)

Web console at **http://127.0.0.1:9777** — loopback only, no auth needed locally.
Run as a user service (survives reboot):

```bash
systemctl --user status ramshield-operator    # should be active
journalctl --user -u ramshield-operator -f    # logs
```

Panels: fleet bar, job table with per-job [run] buttons + regen button,
engine health (:9999 proxy), git state, promo output counts, last bench
result, live OPERATOR_LOG stream (5s poll).

API: `GET /api/{fleet,log,engine,git,promo,bench}`,
`POST /api/run/<job_id>`, `POST /api/regen`.
Stdlib-only server; POSTs are rejected from non-loopback addresses.

### 3.3 Tiny Console (`scripts/operator_console.py`)

Lightweight terminal window to talk to the agent and inspect operator state.
No dependencies beyond Python stdlib.

```bash
./scripts/operator_console.py            # or: python3 scripts/operator_console.py
```

Commands:

| Command | Action |
|---|---|
| `status` | Cron fleet counts + server health + branch/commit |
| `errors [n]` | Last n failing jobs w/ error text |
| `log [n]` | Tail OPERATOR_LOG.md |
| `jobs` | Full job table (name, schedule, status) |
| `run <job_id>` | Trigger a cronjob immediately |
| `list` | List cronjob ids+names (for `run`) |
| `ask <text>` | Send anything else to the hermes agent as a prompt |
| `help`, `quit` | — |

The console shells out to `hermes -z <prompt>` for `ask` — first-class agent
access without leaving the window. `run` uses `hermes cron run <id>`.

### 3.3 Live engine dashboards (separate concern)

The Rust binary serves its own dashboards when running
(`./target/release/ramshield config.stress.toml`):
- IPC listener on `127.0.0.1:7890`
- HTTP snapshot API on `127.0.0.1:9999/api/snapshot`
- Prometheus `/metrics` (auth-gated dashboard since commit 4a27b05/336a145)

---

## 4. Bench / attack harness

`scripts/attack_nexus.py` — multi-vector attack simulator against local IPC.
Authorized localhost testing only.

Profiles (`scripts/profiles.json`): l7_http_flood, volumetric_syn, slowloris,
dns_amplification, botnet_entropy, api_abuse, red_team_full (chain),
**subnet_ddos_5min** (added 2026-08-22).

Subnet DDoS benchmark (30 unique /24s per 15 s rotation):

```bash
./target/release/ramshield config.stress.toml &   # if not already up
curl http://127.0.0.1:9999/api/snapshot           # verify health
./scripts/subnet_ddos_bench.sh                    # 5 min run
```

Wrapper is loopback-only (refuses non-127.x targets) and pre-checks IPC liveness.
Verify results afterwards:

```bash
curl -s http://127.0.0.1:9999/api/snapshot | grep -o '"blocks_applied":[0-9]*'
tail -1 /tmp/subnet_bench.log    # "done: N events, M eps, K errors"
```

Last verified run: 9,623,974 events @ ~31.9k eps, 764 blocks applied, healthy.

---

## 5. Daily rhythm (what runs when)

```
01:00  daily-planner      writes PLAN.md
01:30  dispatcher         spawns per-task workers
02:00  backup             project snapshot
03:00  reviewer           audits worker output
06:00  scalper-morning    model refresh
09:00  promotion agent    long-horizon growth work
every 5m   pulse + cron-status + promo quickwins
every 10m  helper-agent
every 15m  health-loop + git-automation
every 30m  facts-collector + error-healer + promo deep dives
hourly     research-agent + health-repair + strategic-plan
```

---

## 6. Operating procedures

### Fleet health check

```bash
python3 scripts/operator_console.py   # then: status, errors
# or headless:
python3 -c "import json;d=json.load(open('docs/CRON_STATUS.json'));print(sum(j['status']=='error' for j in d['jobs']),'errors')"
```

### Fix a failing job

1. `hermes cron list` → find job_id + last_error
2. Common causes:
   - `drift_skip:silent` → re-pin model (§2.1)
   - `Script exited with code 2` → wrapper path broken (§2.3), or script syntax — `bash -n <script>`
3. After fix: `hermes cron run <job_id>` to verify immediately.

### Regenerate everything after big changes

```bash
python3 .github/scripts/facts_collector.py
python3 .github/scripts/cron_status_collector.py
python3 .github/scripts/html_dashboard_generator.py
git add docs/ && git commit -m "docs(ops): refresh" && git push
```

### Known non-errors

- 5 daily jobs show stale drift errors until their next scheduled fire (pins
  applied 2026-08-22 evening; fires are 01:00–09:00 next morning).
- `CRON_STATUS.json` is gitignored by design; only `.md` is tracked.
- Promo batch outputs are QUEUED stubs until a human/agent publishes them;
  exit 0 = queued successfully, not published.
