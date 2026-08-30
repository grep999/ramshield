# Cron Job Status — 2026-08-30 06:40 UTC

**Live snapshot from `hermes cron list`.** 30 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 17 |
| Error | 1 |
| Running | 2 |
| Pending | 0 |
| Scheduled | 6 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error |  | 2026-08-29T10:16:23.381073+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | 🏃 running | running | 2026-08-30T08:34:15.188422+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-30T08:34:00.980182+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ✅ ok | completed | 2026-08-30T08:24:21.712795+02:00 |
| ramshield-reviewer | `0 3 * * *` | ✅ ok | completed | 2026-08-30T08:27:45.793410+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-30T08:35:36.976727+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-30T08:35:37.263692+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-08-30T08:34:56.174445+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-30T08:29:31.607240+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-08-30T08:34:56.493252+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-30T08:35:37.622274+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-30T08:35:37.985424+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-30T08:35:38.582074+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-30T08:34:57.507764+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-30T08:34:57.762887+02:00 |
| promo-std-devto | `*/15 * * * *` | ✅ ok | completed | 2026-08-30T08:34:58.047639+02:00 |
| promo-std-hn | `*/15 * * * *` | ✅ ok | completed | 2026-08-30T08:34:58.334219+02:00 |
| promo-deep-blog | `*/30 * * * *` | ✅ ok | completed | 2026-08-30T08:34:58.617892+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ✅ ok | completed | 2026-08-30T08:34:58.985169+02:00 |
| promo-strategic-plan | `0 * * * *` | ✅ ok | completed | 2026-08-30T08:29:34.734618+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-30T08:35:09.588532+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ✅ ok | completed | 2026-08-30T08:33:50.908498+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-30T08:33:51.267581+02:00 |
| scalper-hourly | `0 * * * *` | ✅ ok | completed | 2026-08-30T08:19:34.784235+02:00 |
| scalper-daily-morning | `0 6 * * *` | ✅ ok | completed | 2026-08-30T08:19:34.673407+02:00 |
| ramshield-backup | `0 2 * * *` | ✅ ok | completed | 2026-08-30T08:33:51.545611+02:00 |
| ramshield-worker-T1 | `once in 45m` | ❓ unknown |  |  |
| ramshield-worker-T2 | `once in 60m` | ❓ unknown |  |  |
| ramshield-worker-T3 | `once in 75m` | ❓ unknown |  |  |
| ramshield-worker-T4 | `once in 90m` | ❓ unknown |  |  |

## Raw Output

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Scheduled Jobs                                  │
└─────────────────────────────────────────────────────────────────────────┘

  18e3993ed6a0 [active]
    Name:      RamShield Promotion Agent
    Schedule:  0 9 * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion
    Last run:  2026-08-29T10:16:23.381073+02:00  error: RuntimeError: HTTP 401: [openrouter/nvidia/nemotron-3-ultra-550b-a55b:free] [404]: {"error":{"message":"Provider returned error","code":404,"metadata":{"raw":"","provider_name":"Nvidia","is_byok":false}},"user_id":"user_3IGHD8oZegZCxCNYK4ghOoIvpqu"} (reset after 2m)

  e3652296ba99 [active]
    Name:      ramshield-helper-agent
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T08:50:00+02:00
    Deliver:   local
    Last run:  2026-08-30T08:34:15.188422+02:00  ok
    Execution: running  e589537a03174ef38300abfa7aee9f45

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:00:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:34:00.980182+02:00  ok
    Execution: completed  650b1671dc0044b092e4a607eb338df6

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-08-31T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:24:21.712795+02:00  ok
    Execution: completed  a70c76a55fab4536ac2c2cd299f38f39

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-08-31T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:27:45.793410+02:00  ok
    Execution: completed  0f221f99b35c4d3da89771542dde5b0c

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T08:45:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:35:36.976727+02:00  ok
    Execution: running  dc36a8f1ba114429bbec641ef3b75bc3

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T08:45:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:35:37.263692+02:00  ok
    Execution: claimed  56087b9b09be477cb90c73d33f7f78da

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T08:45:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:34:56.174445+02:00  ok
    Execution: completed  3a67dc15fbe54d8ba1c0004b89c87fac

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:29:31.607240+02:00  ok
    Execution: completed  a712eb0d25844a8bad8c93b76928c1ac

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T08:45:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:34:56.493252+02:00  ok
    Execution: completed  3957914bfc104c30ab8c21ff2def1919

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T08:45:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:35:37.622274+02:00  ok
    Execution: claimed  caca81f33edd43a9ba2caa930b536243

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T08:45:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:35:37.985424+02:00  ok
    Execution: claimed  7b1960ff81ab484f9f573838176fd20b

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T08:45:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:35:38.582074+02:00  ok
    Execution: claimed  2aee92a8df9a4b0ab4cb3dfe6b6ee627

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T08:50:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:34:57.507764+02:00  ok
    Execution: claimed  fd4804aa197244ccbb24daf3d1cdf7c0

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T08:50:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:34:57.762887+02:00  ok
    Execution: claimed  830030c87f3741e79a2f489e049d568e

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T08:45:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:34:58.047639+02:00  ok
    Execution: completed  25627fc5ea23445b98f6072e4d813b1e

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T08:45:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:34:58.334219+02:00  ok
    Execution: completed  a45a521e42b44a508f8f92f7d3422599

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:00:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:34:58.617892+02:00  ok
    Execution: completed  a4fe501342824775bd263c5cc776fa3f

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:00:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:34:58.985169+02:00  ok
    Execution: completed  20f868e1ca42429faa1a406b1b65b02c

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:29:34.734618+02:00  ok
    Execution: completed  3d93de41ae974549879b6a5407131490

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:00:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:35:09.588532+02:00  ok
    Execution: completed  fa262c87a2c84c7c8a99b8873cf725f5

  c0d0d4bc8275 [active]
    Name:      ramshield-dispatcher
    Schedule:  30 1 * * *
    Repeat:    ∞
    Next run:  2026-08-31T01:30:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:33:50.908498+02:00  ok
    Execution: completed  b251d5ce5a4f482b9cfeacec6b390103

  26862e70b8a0 [active]
    Name:      ramshield-error-healer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:00:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:33:51.267581+02:00  ok
    Execution: completed  ae89912b681241d3bb392359b64dc093

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-30T08:19:34.784235+02:00  ok
    Execution: completed  b2f1739e62f7447fbd4184ea556fcce1

  77b73c6cddb4 [active]
    Name:      scalper-daily-morning
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-31T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-30T08:19:34.673407+02:00  ok
    Execution: completed  e4b1d044d51f459c8af047fe605c2fd8

  b4a3b9b01db6 [active]
    Name:      ramshield-backup
    Schedule:  0 2 * * *
    Repeat:    ∞
    Next run:  2026-08-31T02:00:00+02:00
    Deliver:   local
    Script:    backup_project.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T08:33:51.545611+02:00  ok
    Execution: completed  1192b7c1dcc64641841b2096267c29c3

  8ada1a42de92 [active]
    Name:      ramshield-worker-T1
    Schedule:  once in 45m
    Repeat:    0/1
    Next run:  2026-08-30T09:16:51.812277+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  58afbac53551 [active]
    Name:      ramshield-worker-T2
    Schedule:  once in 60m
    Repeat:    0/1
    Next run:  2026-08-30T09:32:59.578193+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  a1de92dab876 [active]
    Name:      ramshield-worker-T3
    Schedule:  once in 75m
    Repeat:    0/1
    Next run:  2026-08-30T09:48:06.748069+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  fdfedf7553bc [active]
    Name:      ramshield-worker-T4
    Schedule:  once in 90m
    Repeat:    0/1
    Next run:  2026-08-30T10:03:11.787356+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
```
