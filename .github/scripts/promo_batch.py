#!/usr/bin/env python3
"""Promotion Agency batch runner.

Each promotion job is a single atomic action (one channel, one asset).
Run via cron (no_agent=true). Writes outcomes to:
  docs/promotion/metrics/<job>.jsonl     (machine-readable)
  docs/PROMOTION_LOG.md                  (human-readable, dashboard source)
"""
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
PROMO = ROOT / "docs" / "promotion"
METRICS = PROMO / "metrics"
METRICS.mkdir(parents=True, exist_ok=True)
PROMO_LOG = ROOT / "docs" / "PROMOTION_LOG.md"

# Job registry: (id, tier, channel, action, asset_hint)
JOBS = [
    ("promo-qw-topics",  "quick",  "github",     "update topics/description", "README badges + keywords"),
    ("promo-qw-awesome", "quick",  "awesome",    "submit PR to awesome list",  "awesome-rust/security"),
    ("promo-qw-crates",  "quick",  "crates.io",  "set keywords/categories",    "ddos, ratelimit, tokio"),
    ("promo-fast-reddit","fast",   "reddit",     "post to r/rust WDYW",        "short intro post"),
    ("promo-fast-x",     "fast",   "x",          "post launch tweet",          "1-liner + link"),
    ("promo-std-devto",  "std",    "dev.to",     "publish intro article",      "blog post draft"),
    ("promo-std-hn",     "std",    "hackernews", "Show HN submission",          "showhn copy"),
    ("promo-deep-blog",  "deep",   "blog",       "write article section",      "technical deep-dive"),
    ("promo-deep-news",  "deep",   "newsletters","pitch to Rust Weekly",        "newsletter blurb"),
    ("promo-strat-plan", "strat",  "campaign",   "plan next campaign",          "brief in campaigns/"),
]

def now_iso():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

def log_outcome(job_id, channel, action, status, detail=""):
    """Append JSONL outcome + update PROMOTION_LOG.md tail."""
    rec = {
        "ts": now_iso(), "job": job_id, "channel": channel,
        "action": action, "status": status, "detail": detail,
    }
    (METRICS / f"{job_id}.jsonl").open("a").write(json.dumps(rec) + "\n")
    # Append human-readable line (keep last 200 lines)
    line = f"- `{now_iso()}` **{job_id}** [{channel}] {action} → {status}"
    if detail:
        line += f" — {detail}"
    existing = PROMO_LOG.read_text() if PROMO_LOG.exists() else "# Promotion Log\n\n"
    lines = existing.splitlines()
    lines.append(line)
    PROMO_LOG.write_text("\n".join(lines[:200]) + "\n")

def run_job(job_id, tier, channel, action, hint):
    """Dispatch a single atomic promotion action.

    ponytail: real posting needs API tokens (X/LinkedIn/Reddit). Without
    credentials we draft + stage assets locally and log the intent. Wire
    token-based posting when creds available.
    """
    # Build/stage asset in the right subdir (no external network call).
    target_dir = {
        "github": PROMO / "content", "awesome": PROMO / "lists",
        "crates.io": PROMO / "content", "reddit": PROMO / "social",
        "x": PROMO / "social", "dev.to": PROMO / "articles",
        "hackernews": PROMO / "social", "blog": PROMO / "articles",
        "newsletters": PROMO / "lists", "campaign": PROMO / "campaigns",
    }.get(channel, PROMO / "content")
    target_dir.mkdir(parents=True, exist_ok=True)
    asset = target_dir / f"{job_id}.md"
    asset.write_text(
        f"# {job_id}\n\nChannel: {channel}\nAction: {action}\nHint: {hint}\n"
        f"Generated: {now_iso()}\nStatus: staged (awaiting publish)\n"
    )
    log_outcome(job_id, channel, action, "staged", f"asset: {asset.name}")
    return True

def main():
    # Which job to run is passed as argv[1]; if none, run all 10 in batch.
    if len(sys.argv) > 1:
        wanted = sys.argv[1:]
    else:
        wanted = [j[0] for j in JOBS]
    registry = {j[0]: j for j in JOBS}
    ran = 0
    for jid in wanted:
        if jid not in registry:
            print(f"unknown job: {jid}", file=sys.stderr)
            continue
        _, tier, channel, action, hint = registry[jid]
        ok = run_job(jid, tier, channel, action, hint)
        ran += ok
    print(f"promotion batch: {ran}/{len(wanted)} jobs staged")
    sys.exit(0)

if __name__ == "__main__":
    main()
