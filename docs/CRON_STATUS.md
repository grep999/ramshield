# Cron Job Status — 2026-07-20 00:23 UTC

**14 active jobs** across the automation pipeline.

| Job | Schedule | Last Status | Next Run | State |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | never | 74d60ec059b9 | active |
| RamShield Promotion Agent | `0 9 * * *` | never | 18e3993ed6a0 | active |
| ramshield-helper-agent | `*/10 * * * *` | never | e3652296ba99 | active |
| ramshield-facts-collector | `*/30 * * * *` | never | 1cb5e490c826 | active |
| ramshield-daily-planner | `0 1 * * *` | never | cd22edb2d5f2 | active |
| ramshield-reviewer | `0 3 * * *` | never | d72f32a35099 | active |
| ramshield-worker-T1 | `once in 30m` | never | d59bf66d1d5c | active |
| ramshield-worker-T2 | `once in 45m` | never | 81dc975889ad | active |
| ramshield-worker-T3 | `once in 60m` | never | 57b89ea7a125 | active |
| ramshield-cron-status | `*/5 * * * *` | never | 53feb7ef060c | active |
| ramshield-pulse | `*/5 * * * *` | never | 076a9de35470 | active |
| ramshield-promotion-agent | `*/30 * * * *` | never | 46102194fe6c | active |
| ramshield-research-agent | `0 * * * *` | never | f270eaf2c891 | active |
| ramshield-health-loop | `*/15 * * * *` | never | 3bc0c27129c2 | active |

---

Raw `hermes cron list` output:

```

┌─────────────────────────────────────────────────────────────────────────┐
│                         Scheduled Jobs                                  │
└─────────────────────────────────────────────────────────────────────────┘

  74d60ec059b9 [active]
    Name:      ramshield-backup
    Schedule:  0 2 * * *
    Repeat:    ∞
    Next run:  2026-07-09T02:00:00+02:00
    Deliver:   local
    Script:    backup_project.sh
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-07-08T02:00:34.069680+02:00  ok

  18e3993ed6a0 [active]
    Name:      RamShield Promotion Agent
    Schedule:  0 9 * * *
    Repeat:    ∞
    Next run:  2026-07-15T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion

  e3652296ba99 [active]
    Name:      ramshield-helper-agent
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-07-19T22:40:00+02:00
    Deliver:   local

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T00:00:00+02:00
    Deliver:   local
    Script:    vehicle_of_rationalism/ramshield/beta/rs/.github/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-07-20T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Execution: running  e4466e93280240999ccd134768032029

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-07-20T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  d59bf66d1d5c [active]
    Name:      ramshield-worker-T1
    Schedule:  once in 30m
    Repeat:    0/1
    Next run:  2026-07-20T00:23:19.810479+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  81dc975889ad [active]
    Name:      ramshield-worker-T2
    Schedule:  once in 45m
    Repeat:    0/1
    Next run:  2026-07-20T00:38:20.950999+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  57b89ea7a125 [active]
    Name:      ramshield-worker-T3
    Schedule:  once in 60m
    Repeat:    0/1
    Next run:  2026-07-20T00:53:22.039020+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T00:40:00+02:00
    Deliver:   local
    Script:    vehicle_of_rationalism/ramshield/beta/rs/.github/scripts/cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T00:40:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  46102194fe6c [active]
    Name:      ramshield-promotion-agent
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T01:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T01:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T00:45:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  ⚠  Gateway is not running — jobs won't fire automatically.
     Start it with: hermes gateway install
                    sudo hermes gateway install --system  # Linux servers
     Check status:  hermes cron status

```