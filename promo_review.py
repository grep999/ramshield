#!/usr/bin/env python3
"""promo_review.py - Quality review for staged promotion assets.

Runs every 30 minutes to review staged promotion content.
"""
import json
from datetime import datetime, timezone
from pathlib import Path

CONTENT_DIR = Path.home() / "promotion_content"
CAMPAIGNS_FILE = CONTENT_DIR / "campaigns.json"

def load_campaigns():
    with open(CAMPAIGNS_FILE) as f:
        return json.load(f)

def review_assets():
    """Review all staged assets in promotion_content."""
    campaigns = load_campaigns()["campaigns"]
    report = {
        "timestamp": datetime.now(timezone.utc).isoformat() + "Z",
        "reviewed": [],
        "ready_to_publish": [],
        "needs_revision": [],
        "summary": {}
    }

    for campaign in campaigns:
        assets_dir = CONTENT_DIR / campaign["assets_dir"]
        if not assets_dir.exists():
            continue

        for asset in assets_dir.glob("*.md"):
            try:
                content = asset.read_text()
                report["reviewed"].append({
                    "campaign": campaign["id"],
                    "asset": str(asset),
                    "size": len(content),
                    "status": "STAGED"
                })
                if "PUBLISHED" not in content and "REJECTED" not in content:
                    report["ready_to_publish"].append({
                        "campaign": campaign["id"],
                        "asset": str(asset)
                    })
            except Exception as e:
                report["needs_revision"].append({
                    "campaign": campaign["id"],
                    "asset": str(asset),
                    "error": str(e)
                })

    report["summary"] = {
        "total_reviewed": len(report["reviewed"]),
        "ready_to_publish": len(report["ready_to_publish"]),
        "needs_revision": len(report["needs_revision"])
    }

    report_file = CONTENT_DIR / f"review_{datetime.now(timezone.utc).strftime('%Y%m%d_%H%M%S')}.json"
    report_file.write_text(json.dumps(report, indent=2))
    print(f"Review complete: {report['summary']}")
    print(f"Report saved: {report_file}")

if __name__ == "__main__":
    review_assets()