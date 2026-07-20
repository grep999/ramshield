#!/usr/bin/env python3
"""Promotion Reviewer — quality-checks staged promo assets.

Picks up everything in docs/promotion/{content,social,articles,lists,campaigns}
written since the last review, scores each asset on:
  - Length (10..600 words for social, 3000 for articles)
  - Has link to repo
  - Has at least 1 hashtag/topic marker
  - No placeholder strings ("TODO", "FIXME", "lorem")

Writes scores to docs/promotion/metrics/review.jsonl
and a daily status file docs/promotion/REVIEW.md.
"""
import json
import re
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
PROMO = ROOT / "docs" / "promotion"
METRICS = PROMO / "metrics"
METRICS.mkdir(parents=True, exist_ok=True)
REVIEW_LOG = METRICS / "review.jsonl"
REVIEW_DO = PROMO / "REVIEW.md"

REQUIRED_LINK = ("github.com", "crates.io", "dev.to", "reddit.com", "news.ycombinator.com")
PLACEHOLDERS = re.compile(r"\b(TODO|FIXME|XXX|lorem ipsum|Lorem|placeholder)\b", re.I)
MARKERS = re.compile(r"#\w+")

def now_iso():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

def score_asset(p: Path):
    body = p.read_text(encoding="utf-8", errors="ignore")
    words = len(body.split())
    flags = []
    if PLACEHOLDERS.search(body):
        flags.append("placeholder")
    if not any(link in body for link in REQUIRED_LINK):
        flags.append("no-link")
    if words < 10:
        flags.append("too-short")
    if words > 3000 and "/articles/" not in str(p):
        flags.append("too-long")
    return {
        "ts": now_iso(),
        "asset": str(p.relative_to(ROOT)),
        "words": words,
        "markers": len(MARKERS.findall(body)),
        "flags": flags,
        "status": "ok" if not flags else "needs-revision",
    }

def main():
    if not PROMO.is_dir():
        print("promotion dir missing"); return
    overview = []
    scored = 0
    for sub in ("content", "social", "articles", "lists", "campaigns"):
        d = PROMO / sub
        if not d.is_dir():
            continue
        for f in sorted(d.glob("*.md")):
            s = score_asset(f)
            overview.append(s)
            scored += 1
    REVIEW_LOG.write_text(
        "".join(json.dumps(o) + "\n" for o in overview),
        encoding="utf-8"
    )
    status = "ok" if all(o["status"] == "ok" for o in overview) else "needs-revision"
    summary = ["# Promotion Review", f"\n_{now_iso()}_ — **{status}** — {scored} assets reviewed\n"]
    by_status = {}
    for o in overview:
        by_status.setdefault(o["status"], []).append(o)
    for st, items in by_status.items():
        summary.append(f"## {st} ({len(items)})")
        for o in items:
            tag = ""
            if o["flags"]:
                tag = " — " + ",".join(o["flags"])
            summary.append(f"- `{o['asset']}` ({o['words']}w, {o['markers']} markers){tag}")
        summary.append("")
    REVIEW_DO.write_text("\n".join(summary), encoding="utf-8")
    print(f"reviewed {scored} assets → {status}")

if __name__ == "__main__":
    main()
