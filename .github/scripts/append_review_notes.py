#!/usr/bin/env python3
"""Append sitelink to FACTS.json once per cycle."""

import json
from pathlib import Path

facts_path = Path('docs/FACTS.json')
facts = json.loads(facts_path.read_text())

facts['review_notes'] = '2026-07-31 REVIEW: Dispatcher did not dispatch for 2026-07-31 plan (scheduled 01:30 UTC). All tasks NOT_STARTED. Re-add T1-T5, investigate dispatcher cron, create WORKER_STATUS.md placeholder if missing.'

facts_path.write_text(json.dumps(facts, indent=2) + '\n')
print(f"Updated {facts_path} review_notes")