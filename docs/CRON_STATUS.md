# Cron Job Status — 2026-08-01 06:21 UTC

**Live snapshot from `hermes cron list`.** 28 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 16 |
| Error | 4 |
| Running | 2 |
| Pending | 0 |
| Scheduled | 6 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ✅ ok | completed | 2026-08-01T06:25:49.382082+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error | failed | 2026-07-31T20:00:22.906925+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | 🏃 running | running | 2026-08-01T08:11:17.631118+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T08:00:56.969505+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ✅ ok | completed | 2026-08-01T06:26:44.855524+02:00 |
| ramshield-reviewer | `0 3 * * *` | ✅ ok | completed | 2026-08-01T06:32:38.450509+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-01T08:16:00.670818+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:16:00.830001+02:00 |
| ramshield-research-agent | `0 * * * *` | ❌ error | failed | 2026-08-01T08:01:29.149456+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-08-01T08:16:15.810565+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-01T08:02:02.146889+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-08-01T08:16:15.982906+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:16:16.139351+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:16:16.307581+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:16:16.483047+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:11:01.017638+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:11:01.163917+02:00 |
| promo-std-devto | `*/15 * * * *` | ✅ ok | completed | 2026-08-01T08:16:16.649915+02:00 |
| promo-std-hn | `*/15 * * * *` | ✅ ok | completed | 2026-08-01T08:16:16.826053+02:00 |
| promo-deep-blog | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T08:02:03.761936+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T08:02:03.917771+02:00 |
| promo-strategic-plan | `0 * * * *` | ✅ ok | completed | 2026-08-01T08:02:04.102638+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T08:02:04.983228+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error | failed | 2026-08-01T06:37:29.253355+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T08:02:05.142518+02:00 |
| scalper-hourly | `0 * * * *` | ❌ error | failed | 2026-08-01T08:01:47.080819+02:00 |
| scalper-daily-morning | `0 6 * * *` | ✅ ok | completed | 2026-08-01T06:27:10.108879+02:00 |
| llm-scalper-hourly | `every 60m` | ✅ ok | completed | 2026-08-01T07:25:52.794026+02:00 |

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
    Next run:  2026-08-01T08:30:00+02:00
    Deliver:   local
    Last run:  2026-08-01T08:11:17.631118+02:00  error: RuntimeError: HTTP 404: No active credentials for provider: cloudflare
    Execution: running  bba3c8bac0fd441bb91b5c48ac85f480

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:30:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:00:56.969505+02:00  ok
    Execution: completed  b797a690856246a389a8e6dceb787a39

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
    Next run:  2026-08-01T08:25:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:16:00.670818+02:00  ok
    Execution: running  fa4ca406044b4688a7b2cc9a77fc4a75

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:25:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:16:00.830001+02:00  ok
    Execution: claimed  614d6ebd4e5543b4a916a229e23e1e94

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T09:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:01:29.149456+02:00  error: RuntimeError: HTTP 404: No active credentials for provider: cloudflare
    Execution: failed  ea8927fbc3404446a506cd3d168cac28

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:30:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:16:15.810565+02:00  ok
    Execution: completed  f7b7f6045605426d974d06ec84d01677

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T09:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:02:02.146889+02:00  ok
    Execution: completed  334f1abd44e045d4bbb2d5bc3a488c60

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:30:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:16:15.982906+02:00  ok
    Execution: completed  ccb1b3a74290457391803d82b8660ec8

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:25:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:16:16.139351+02:00  ok
    Execution: claimed  afc2842e9811463d85ce0acd9cccee63

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:25:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:16:16.307581+02:00  ok
    Execution: claimed  0de59f7bbbbf433a98fefba5df95556b

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:25:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:16:16.483047+02:00  ok
    Execution: claimed  a58581f83ba2457aacb950a1e68d7e29

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:30:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:11:01.017638+02:00  ok
    Execution: claimed  4472723735564f129a78fa8ad77dc9a9

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:30:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:11:01.163917+02:00  ok
    Execution: claimed  437977b981c743d9bd63766556b65ef0

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:30:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:16:16.649915+02:00  ok
    Execution: completed  e5695c1d2b28437cb6929e2a4faf5358

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:30:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:16:16.826053+02:00  ok
    Execution: completed  b30a1f3cc61b4847bec5b8947d7826a7

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:30:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:02:03.761936+02:00  ok
    Execution: completed  004214e8f2af4982aac10b6de67d3877

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:30:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:02:03.917771+02:00  ok
    Execution: completed  78811f4989b54e8595277b24d8d06409

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T09:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:02:04.102638+02:00  ok
    Execution: completed  325ab5bb261d4e2ca03bd8768a4dc6d4

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T08:30:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:02:04.983228+02:00  ok
    Execution: completed  c1a0ddbe80994197ae00811436c06fb3

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
    Next run:  2026-08-01T08:30:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:02:05.142518+02:00  ok
    Execution: completed  2421db5480694f0d93f29e8f31e9be13

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T09:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Last run:  2026-08-01T08:01:47.080819+02:00  error: RuntimeError: HTTP 404: No active credentials for provider: cloudflare
    Execution: failed  fe922e0abed04977a74daa63f0cd2bff

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
    Next run:  2026-08-01T08:25:52.794026+02:00
    Deliver:   local
    Script:    llm_scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-01T07:25:52.794026+02:00  ok
    Execution: completed  470dbef2474f4808ad857ab5f6e2ac3e
```
