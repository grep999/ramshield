# Git Automation & Error-Handling Manual

## Patterns Discovered in Production

### 1. **Gateway Must Be Running**
- **Symptom**: All cron jobs show `never` status, `next_run` in past, no executions
- **Fix**: `hermes gateway install` + `hermes gateway start`
- **Verification**: `hermes cron status` shows "✓ User gateway service is running"

### 2. **LLM Jobs Timeout on Slow Provider**
- **Symptom**: `TimeoutError: idle for 600s (limit 600s) — waiting for non-streaming API response`
- **Root cause**: `ram` model/provider slow; 600s hard limit
- **Fix**: Convert to `no_agent=true` + deterministic Python script
  - `ramshield-health-loop` → `health_check_repair.py`
  - `ramshield-health-repair` → `health_check_repair.py`
  - `ramshield-helper-agent` → next candidate
- **Result**: Instant execution, no LLM latency

### 3. **Script Path Must Be in `~/.hermes/scripts/`**
- **Symptom**: `Script not found: /home/m/.hermes/scripts/vehicle_of_rationalism/...`
- **Fix**: `cp script.py ~/.hermes/scripts/` then update cron with bare filename
- **Affected**: `facts_collector.py`, `cron_status_collector.py`, `health_check_repair.py`, `git_automation.py`

### 4. **Dashboard Stale Until Collectors Run**
- **Symptom**: Dashboard shows old "never" statuses
- **Fix**: `cron_status_collector.py` runs every 5min via cron → writes `CRON_STATUS.md` → dashboard reads it
- **Verify**: `grep "ramshield-pulse" docs/AUTOMATION_DASHBOARD.html`

### 5. **Diverged Branches (main vs master)**
- **Symptom**: Push rejected, history divergence
- **Fix**: `git fetch origin && git push origin temp_sync:main && git push origin temp_sync:master`
- **Automated**: `git_automation.py` does fetch + rebase + push

### 6. **Once-Jobs Stuck Forever**
- **Symptom**: `ramshield-worker-T1` schedule "once in 30m", past time, never runs
- **Fix**: Delete stale once-jobs; use recurring cron with `no_agent` scripts instead

### 7. **Missing Files Break Pipeline**
- `FACTS.json` missing → collectors fail
- `PULSE_LOG.md` missing → dashboard shows "EMPTY"
- **Fix**: `git_automation.py` auto-commits generated files; collectors regenerate on schedule

### 8. **Dead Link False Positives**
- **Symptom**: 3,849 "dead links" (same 3 links repeated in `HEALTH_DASHBOARD.md`)
- **Fix**: Deduplicate in `check_dead_links()` using `seen` set; skip anchor-only links (`#section`)

---

## Standard Recovery Procedure

```bash
# 1. Verify gateway
hermes gateway status
# If not running:
hermes gateway install && hermes gateway start

# 2. Check cron health
hermes cron list
# Look for: timeout errors, never-run jobs, script-not-found

# 3. Fix timeout jobs
# Edit cron → no_agent=true, script=<name>.py
# Ensure script exists in ~/.hermes/scripts/

# 4. Regenerate dashboard
cd /home/m/vehicle_of_rationalism/ramshield/beta/rs
python3 .github/scripts/facts_collector.py
python3 .github/scripts/cron_status_collector.py
python3 .github/scripts/html_dashboard_generator.py

# 5. Verify dashboard live
curl -s http://127.0.0.1:8080/AUTOMATION_DASHBOARD.html | grep "ramshield-pulse"

# 6. Force git sync if needed
python3 .github/scripts/git_automation.py
```

---

## Cron Job Patterns That Work

| Job Type | Mode | Script Location | Timeout |
|----------|------|-----------------|---------|
| Data collection | `no_agent=true` | `~/.hermes/scripts/*.py` | < 5s |
| Health check | `no_agent=true` | `~/.hermes/scripts/*.py` | < 10s |
| Git automation | `no_agent=true` | `~/.hermes/scripts/*.py` | < 15s |
| LLM reasoning | Avoid in cron | Use explicit agent runs | 600s limit |

---

## Health Check Repair Job — Updated Instructions

The `ramshield-health-repair` cronjob (hourly) now runs `health_check_repair.py` which:
1. Checks all 7 dashboard sections independently
2. Applies safe auto-fixes (symlinks, missing files, FACTS.json refresh)
3. Logs to `docs/HEALTH_CHECK.md` with per-section status
4. Runs `git_automation.py` to commit any changes

If it reports `CRITICAL`:
- Check `hermes cron list` for timeout errors
- Run the Standard Recovery Procedure above
- Dashboard will self-heal once gateway + no_agent jobs are healthy