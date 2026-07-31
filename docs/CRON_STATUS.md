# Cron Job Status — 2026-07-31 21:10 UTC

**Live snapshot from `hermes cron list`.** 28 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 14 |
| Error | 4 |
| Running | 2 |
| Pending | 0 |
| Scheduled | 6 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ✅ ok | completed | 2026-07-31T19:38:20.361016+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error | failed | 2026-07-31T20:00:22.906925+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | 🏃 running | running | 2026-07-31T23:00:53.494921+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-07-31T23:00:26.554686+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ✅ ok | completed | 2026-07-31T20:04:48.123568+02:00 |
| ramshield-reviewer | `0 3 * * *` | ✅ ok | completed | 2026-07-31T20:09:01.957599+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-07-31T23:05:29.007374+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-07-31T23:05:29.186843+02:00 |
| ramshield-research-agent | `0 * * * *` | ❌ error | failed | 2026-07-31T23:01:03.229025+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-07-31T23:01:18.537652+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-07-31T23:01:33.905562+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-07-31T23:01:34.079254+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-07-31T23:05:29.370721+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-07-31T23:05:29.550715+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-07-31T23:05:29.727439+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-07-31T23:01:34.777225+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-07-31T23:01:34.943868+02:00 |
| promo-std-devto | `*/15 * * * *` | ✅ ok | completed | 2026-07-31T23:01:35.113980+02:00 |
| promo-std-hn | `*/15 * * * *` | ✅ ok | completed | 2026-07-31T23:01:35.289823+02:00 |
| promo-deep-blog | `*/30 * * * *` | ✅ ok | completed | 2026-07-31T23:01:35.486178+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ✅ ok | completed | 2026-07-31T23:01:35.660895+02:00 |
| promo-strategic-plan | `0 * * * *` | ✅ ok | completed | 2026-07-31T23:01:35.800809+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-07-31T23:01:36.141301+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error | failed | 2026-07-31T20:20:42.678197+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-07-31T23:01:36.333391+02:00 |
| scalper-hourly | `0 * * * *` | ❌ error | failed | 2026-07-31T23:01:24.755118+02:00 |
| scalper-daily-morning | `0 6 * * *` | ❓ unknown |  |  |
| llm-scalper-hourly | `every 60m` | ❓ unknown |  |  |

## Raw Output

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Scheduled Jobs                                  │
└─────────────────────────────────────────────────────────────────────────┘

  74d60ec059b9 [active]
    Name:      ramshield-backup
    Schedule:  0 2 * * *
    Repeat:    ∞
    Next run:  2026-08-01T02:00:00+02:00
    Deliver:   local
    Script:    backup_project.sh
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-07-31T19:38:20.361016+02:00  ok
    Execution: completed  07bdc89f1cf1467b870a9216db12d71e

  18e3993ed6a0 [active]
    Name:      RamShield Promotion Agent
    Schedule:  0 9 * * *
    Repeat:    ∞
    Next run:  2026-08-01T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion
    Last run:  2026-07-31T20:00:22.906925+02:00  error: TimeoutError: Cron job 'RamShield Promotion Agent' idle for 600s (limit 600s) — last activity: waiting for non-streaming API response
    Execution: failed  16cc488711134a5fb22cbf5fd8ca6ad4

  e3652296ba99 [active]
    Name:      ramshield-helper-agent
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:20:00+02:00
    Deliver:   local
    Last run:  2026-07-31T23:00:53.494921+02:00  error: RuntimeError: HTTP 429: [openrouter/openrouter/free] [429]: {"error":{"message":"Rate limit exceeded: free-models-per-day. Add 10 credits to unlock 1000  (reset after 1m 49s)
    Execution: running  f76f7c59c97d44aeada6c56f9a8014ee

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:30:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:00:26.554686+02:00  ok
    Execution: completed  70caf8ffcda94852b2944332617fe037

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-08-01T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T20:04:48.123568+02:00  ok
    Execution: completed  89d35596847248858d1c7feb4f21a2d9

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-08-01T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T20:09:01.957599+02:00  ok
    Execution: completed  a72cbbbdf7884c289e141a824187b0b7

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:15:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:05:29.007374+02:00  ok
    Execution: running  2fe4da0dd6ac4edeb26c04f82b6ad796

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:15:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:05:29.186843+02:00  ok
    Execution: claimed  88d2b42d6e344e6788c33455476b417c

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T00:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:01:03.229025+02:00  error: RuntimeError: HTTP 429: [openrouter/poolside/laguna-s-2.1:free] [429]: {"error":{"message":"Rate limit exceeded: free-models-per-day. Add 10 credits to unlock 1000  (reset after 1m 39s)
    Execution: failed  11f0d6fd05ee472d882c1e50ff0ed289

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:15:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:01:18.537652+02:00  ok
    Execution: completed  d7f904c80c73461f95af4bbc9a5a3481

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T00:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:01:33.905562+02:00  ok
    Execution: completed  f5ab376b88fe4d74bb771a71c33fca84

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:15:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:01:34.079254+02:00  ok
    Execution: completed  ff3ec79041e349b5a4222f0d6bd81c77

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:15:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:05:29.370721+02:00  ok
    Execution: claimed  7b3a3cfa1c0d4794b4e0b40ab6288319

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:15:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:05:29.550715+02:00  ok
    Execution: claimed  5b0fbb5838e44847ae7648d65ca63086

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:15:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:05:29.727439+02:00  ok
    Execution: claimed  0a8be32c8078469daa60877569216eba

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:20:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:01:34.777225+02:00  ok
    Execution: claimed  5b6c1d515fc54e41a0d4fd82da966294

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:20:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:01:34.943868+02:00  ok
    Execution: claimed  dd11f0fcfa314ed6b3234e018e5ee94a

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:15:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:01:35.113980+02:00  ok
    Execution: completed  299f4977d9a24a1aa6c2f44317fa64da

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:15:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:01:35.289823+02:00  ok
    Execution: completed  2620d6b4ecbe4b329975ed247992521b

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:30:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:01:35.486178+02:00  ok
    Execution: completed  ac1082fee328428ab7a6f7f835270a17

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:30:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:01:35.660895+02:00  ok
    Execution: completed  d305678390de4299b2c12b415362634a

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T00:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:01:35.800809+02:00  ok
    Execution: completed  f9c8dc5681704e36963dba24f555aa2f

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:30:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:01:36.141301+02:00  ok
    Execution: completed  bbb669a94d1d4471ae5ca36702938965

  c0d0d4bc8275 [active]
    Name:      ramshield-dispatcher
    Schedule:  30 1 * * *
    Repeat:    ∞
    Next run:  2026-08-01T01:30:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T20:20:42.678197+02:00  error: RuntimeError: Skipped to prevent unintended spend: global inference config drifted since this job was created (provider 'opencode-go' -> 'custom'; model 'kimi-k2.7-code' -> 'ram'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=c0d0d4bc8275 provider=<provider> model=<model>` (or pin the original values to keep them). See #44585.
    Execution: failed  cd4e0696270e4146b1b4c0fd90862058

  26862e70b8a0 [active]
    Name:      ramshield-error-healer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-31T23:30:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-31T23:01:36.333391+02:00  ok
    Execution: completed  84e6ca6f6a3e4ac5ac57a68906ed7d0c

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T00:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Last run:  2026-07-31T23:01:24.755118+02:00  error: RuntimeError: HTTP 404: [openrouter/poolside/laguna-xs-2.1:free] [429]: {"error":{"message":"Rate limit exceeded: free-models-per-day. Add 10 credits to unlock 1000  (reset after 1m 18s)
    Execution: failed  5bf58294f93446d9903c50baddad7c0e

  77b73c6cddb4 [active]
    Name:      scalper-daily-morning
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-01T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check

  34e128879624 [active]
    Name:      llm-scalper-hourly
    Schedule:  every 60m
    Repeat:    ∞
    Next run:  2026-07-31T23:16:20.058530+02:00
    Deliver:   local
    Script:    llm_scalper.py
    Mode:      no-agent (script stdout delivered directly)
```
