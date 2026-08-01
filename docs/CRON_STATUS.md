# Cron Job Status — 2026-08-01 04:55 UTC

**Live snapshot from `hermes cron list`.** 28 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 19 |
| Error | 3 |
| Running | 2 |
| Pending | 0 |
| Scheduled | 4 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ✅ ok | completed | 2026-08-01T06:25:49.382082+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error | failed | 2026-07-31T20:00:22.906925+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | 🏃 running | running | 2026-08-01T06:40:50.480597+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T06:37:30.331349+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ✅ ok | completed | 2026-08-01T06:26:44.855524+02:00 |
| ramshield-reviewer | `0 3 * * *` | ✅ ok | completed | 2026-08-01T06:32:38.450509+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-01T06:50:48.556473+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T06:50:48.741915+02:00 |
| ramshield-research-agent | `0 * * * *` | ❌ error | failed | 2026-08-01T06:36:35.204662+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-08-01T06:46:04.063005+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-01T06:37:26.096387+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-08-01T06:46:04.232896+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T06:50:48.898970+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T06:50:49.075256+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T06:50:49.268634+02:00 |
| promo-fast-reddit | `*/10 * * * *` | ✅ ok | completed | 2026-08-01T06:50:49.472490+02:00 |
| promo-fast-x | `*/10 * * * *` | ✅ ok | completed | 2026-08-01T06:50:49.648081+02:00 |
| promo-std-devto | `*/15 * * * *` | ✅ ok | completed | 2026-08-01T06:46:04.914890+02:00 |
| promo-std-hn | `*/15 * * * *` | ✅ ok | completed | 2026-08-01T06:46:05.086182+02:00 |
| promo-deep-blog | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T06:37:27.607727+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T06:37:27.777464+02:00 |
| promo-strategic-plan | `0 * * * *` | ✅ ok | completed | 2026-08-01T06:37:27.932409+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T06:37:29.076872+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error | failed | 2026-08-01T06:37:29.253355+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T06:37:29.587850+02:00 |
| scalper-hourly | `0 * * * *` | ✅ ok | completed | 2026-08-01T06:30:40.063858+02:00 |
| scalper-daily-morning | `0 6 * * *` | ✅ ok | completed | 2026-08-01T06:27:10.108879+02:00 |
| llm-scalper-hourly | `every 60m` | ✅ ok | completed | 2026-08-01T06:25:44.838342+02:00 |

## Raw Output

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Scheduled Jobs                                  │
└─────────────────────────────────────────────────────────────────────────┘

  74d60ec059b9 [active]
    Name:      ramshield-backup
    Schedule:  0 2 * * *
    Repeat:    ∞
    Next run:  2026-08-02T02:00:00+02:00
    Deliver:   local
    Script:    backup_project.sh
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-01T06:25:49.382082+02:00  ok
    Execution: completed  4a30e4b1759b4735b242a67bb009a6d1

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
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Last run:  2026-08-01T06:40:50.480597+02:00  error: RuntimeError: Context length exceeded (212 tokens). Cannot compress further.
    Execution: running  566491ac04bb4b4d958c290f53261c27

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:37:30.331349+02:00  ok
    Execution: completed  174bd288c1184007bb974441de6fb935

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-08-02T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:26:44.855524+02:00  ok
    Execution: completed  97772217ff1a4591b362756f2837bd11

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-08-02T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:32:38.450509+02:00  ok
    Execution: completed  325f9cfa6d8e47dba42f2b1fb60b3541

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:50:48.556473+02:00  ok
    Execution: running  5b9c7669942f465786c67a1391cfd3de

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:50:48.741915+02:00  ok
    Execution: claimed  1f372fd14bdd4b63ba1574ad876585bd

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:36:35.204662+02:00  error: RuntimeError: HTTP 429: [openrouter/cohere/north-mini-code:free] [429]: {"error":{"message":"Rate limit exceeded: free-models-per-day. Add 10 credits to unlock 1000  (reset after 8s)
    Execution: failed  1112409bc3ff48b3974e01668d8ac0f9

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:46:04.063005+02:00  ok
    Execution: completed  388312f544a94794b9b6f9f93dcb946d

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:37:26.096387+02:00  ok
    Execution: completed  69b21312b01a45079086a2eda2f7d6bd

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:46:04.232896+02:00  ok
    Execution: completed  73ce693e7a0d4ddc9df75b32bc1649aa

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:50:48.898970+02:00  ok
    Execution: claimed  fd775177721243f8a4bd2818fa3ad750

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:50:49.075256+02:00  ok
    Execution: claimed  706d8313229f415ba940b9291eb4cbef

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:50:49.268634+02:00  ok
    Execution: claimed  4a7627d6067844df92aae4ce3fbd0cf4

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:50:49.472490+02:00  ok
    Execution: completed  d76b8b1c2b884055aae945f6b53d983f

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:50:49.648081+02:00  ok
    Execution: completed  5498ff548f5740968dde0bd1e6b998a2

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:46:04.914890+02:00  ok
    Execution: completed  ee608a32bbc64f7bbea652e238ec7522

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:46:05.086182+02:00  ok
    Execution: completed  8c9a3a5caebc4921936b2b411dbb1cf6

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:37:27.607727+02:00  ok
    Execution: completed  550de8d17be54879a52ba0e1db4eb029

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:37:27.777464+02:00  ok
    Execution: completed  a02b5275ff864d5a8079b8beca992702

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:37:27.932409+02:00  ok
    Execution: completed  9a30caac5425447b907719578a0cea1d

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:37:29.076872+02:00  ok
    Execution: completed  7dc7e9bcfa26459fb69210d1bd73caa1

  c0d0d4bc8275 [active]
    Name:      ramshield-dispatcher
    Schedule:  30 1 * * *
    Repeat:    ∞
    Next run:  2026-08-02T01:30:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:37:29.253355+02:00  error: RuntimeError: Skipped to prevent unintended spend: global inference config drifted since this job was created (provider 'opencode-go' -> 'custom'; model 'kimi-k2.7-code' -> 'ram'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=c0d0d4bc8275 provider=<provider> model=<model>` (or pin the original values to keep them). See #44585.
    Execution: failed  6073b3e744b64c808b56622adaec61f6

  26862e70b8a0 [active]
    Name:      ramshield-error-healer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T06:37:29.587850+02:00  ok
    Execution: completed  88e1dc7f32904b1781b35d1b752b1ffb

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T07:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Last run:  2026-08-01T06:30:40.063858+02:00  ok
    Execution: completed  c1225fc45d7b4f0884118aa5067101e7

  77b73c6cddb4 [active]
    Name:      scalper-daily-morning
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-02T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
    Last run:  2026-08-01T06:27:10.108879+02:00  ok
    Execution: completed  2bd9bc3f3a264c35abaaf6825b6f92a8

  34e128879624 [active]
    Name:      llm-scalper-hourly
    Schedule:  every 60m
    Repeat:    ∞
    Next run:  2026-08-01T07:25:44.838342+02:00
    Deliver:   local
    Script:    llm_scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-01T06:25:44.838342+02:00  ok
    Execution: completed  3fb1781dd76241fd9acfcce07cdc235f
```
