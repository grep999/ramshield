#!/usr/bin/env python3
"""cron_status_collector.py - Snapshot active cron jobs into CRON_STATUS.md.
Runs every 5 minutes via hermes cron. Uses `hermes cron list` to get live data.
"""
import subprocess, os, sys
from datetime import datetime, timezone

OUTPUT = "docs/CRON_STATUS.md"

def main():
    result = subprocess.run(
        ["hermes", "cron", "list"],
        capture_output=True, text=True,
        env={**os.environ, "HOME": "/home/m"},
        timeout=30
    )
    raw = result.stdout
    if not raw.strip():
        raw = "(hermes cron list returned empty output)"

    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

    content = f"""# Cron Job Status — {now}

**Live snapshot from `hermes cron list`.** Updated every 5 minutes.

```
{raw.strip()}
```
"""
    with open(OUTPUT, "w") as f:
        f.write(content)
    print(f"CRON_STATUS.md written ({len(content)} bytes) at {now}")

if __name__ == "__main__":
    main()