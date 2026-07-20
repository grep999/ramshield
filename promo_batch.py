#!/usr/bin/env python3
"""promo_batch.py - Atomized promotion campaign jobs for RamShield.

10 independent jobs running at different frequencies:
- Quick Wins (5m): GitHub topics, Awesome Rust PR, crates.io metadata
- Fast Reach (10m): Reddit WDYW, X/Twitter thread
- Standard Dev (15m): dev.to article, Show HN copy
- Deep Dive (30m): Technical blog, Rust Weekly pitch
- Strategic (60m): Plan next campaign cycle
"""
import json
import os
import sys
import subprocess
import time
from datetime import datetime
from pathlib import Path

CONTENT_DIR = Path.home() / "promotion_content"
CAMPAIGNS_FILE = CONTENT_DIR / "campaigns.json"

def load_campaigns():
    with open(CAMPAIGNS_FILE) as f:
        return json.load(f)

def get_campaigns_by_type(campaign_type):
    campaigns = load_campaigns()["campaigns"]
    return [c for c in campaigns if c["type"] == campaign_type]

def run_campaign(campaign_id):
    """Execute a single campaign job."""
    campaigns = load_campaigns()["campaigns"]
    campaign = next((c for c in campaigns if c["id"] == campaign_id), None)
    if not campaign:
        print(f"Campaign {campaign_id} not found")
        return 1

    assets_dir = CONTENT_DIR / campaign["assets_dir"]
    assets_dir.mkdir(parents=True, exist_ok=True)

    timestamp = datetime.utcnow().strftime("%Y%m%d_%H%M%S")
    output_file = assets_dir / f"{campaign_id}_{timestamp}.md"

    content = f"""# {campaign['name']}

**Channel:** {campaign['channel']}
**Type:** {campaign['type']}
**Priority:** {campaign['priority']}
**Frequency:** {campaign['frequency']}
**Generated:** {datetime.utcnow().isoformat()}Z

## Description
{campaign['description']}

## Status
- Status: QUEUED
- Output: {output_file}

## Next Steps
- [ ] Generate content
- [ ] Review quality
- [ ] Publish to {campaign['channel']}
- [ ] Track metrics
"""

    output_file.write_text(content)
    print(f"Created: {output_file}")
    return 0

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: promo_batch.py <campaign_id>")
        sys.exit(1)
    sys.exit(run_campaign(sys.argv[1]))