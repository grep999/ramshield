# Cron Job Status — 2026-08-29 09:20 UTC

**Live snapshot from `hermes cron list`.** 28 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 16 |
| Error | 1 |
| Running | 2 |
| Pending | 0 |
| Scheduled | 6 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error | failed | 2026-08-29T10:16:23.381073+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | 🏃 running | running | 2026-08-29T11:11:24.017657+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-29T11:00:44.297511+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ✅ ok | completed | 2026-08-29T10:17:39.405682+02:00 |
| ramshield-reviewer | `0 3 * * *` | ✅ ok | completed | 2026-08-29T10:20:46.115472+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-29T11:15:47.535359+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-29T11:15:47.929323+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-08-29T11:16:05.301362+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-29T11:01:23.712677+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-08-29T11:16:05.654315+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-29T11:16:05.952350+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-29T11:16:06.337911+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-29T11:16:06.761071+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-29T11:10:48.263375+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-29T11:10:48.602625+02:00 |
| promo-std-devto | `*/15 * * * *` | ✅ ok | completed | 2026-08-29T11:16:07.176344+02:00 |
| promo-std-hn | `*/15 * * * *` | ✅ ok | completed | 2026-08-29T11:16:07.509951+02:00 |
| promo-deep-blog | `*/30 * * * *` | ✅ ok | completed | 2026-08-29T11:01:26.543751+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ✅ ok | completed | 2026-08-29T11:01:26.970109+02:00 |
| promo-strategic-plan | `0 * * * *` | ✅ ok | completed | 2026-08-29T11:01:27.300730+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-29T11:01:29.048254+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ✅ ok | completed | 2026-08-29T10:24:07.440397+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-29T11:01:29.323455+02:00 |
| scalper-hourly | `0 * * * *` | ✅ ok | completed | 2026-08-29T11:00:44.679609+02:00 |
| scalper-daily-morning | `0 6 * * *` | ✅ ok | completed | 2026-08-29T10:08:42.631118+02:00 |
| ramshield-worker-T2 | `once at 2026-08-29 11:28` | ❓ unknown |  |  |
| ramshield-worker-T3 | `once at 2026-08-29 11:43` | ❓ unknown |  |  |
| ramshield-backup | `0 2 * * *` | ❓ unknown |  |  |

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
    Execution: failed  a6b909604ad54ec6a5df98197842e6d2

  e3652296ba99 [active]
    Name:      ramshield-helper-agent
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:30:00+02:00
    Deliver:   local
    Last run:  2026-08-29T11:11:24.017657+02:00  ok
    Execution: running  77861ba33fc84de3889f7824f3fb0b8f

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:30:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:00:44.297511+02:00  ok
    Execution: completed  d379d0cb84384a408efb73414f96eeae

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-08-30T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T10:17:39.405682+02:00  ok
    Execution: completed  63bcbbb51e9c45e7b4a4ecbd3e348690

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-08-30T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T10:20:46.115472+02:00  ok
    Execution: completed  8a1f3e0ba445418f8576da3ff3c9c4bd

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:25:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:15:47.535359+02:00  ok
    Execution: running  38a1dc8da27c406bab785505c2f8e4b1

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:25:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:15:47.929323+02:00  ok
    Execution: claimed  9dc397e5ef2d4bc0b97c404a566ad15a

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:30:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:16:05.301362+02:00  ok
    Execution: completed  204be7071ab24d388e2f0f2d60289107

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T12:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:01:23.712677+02:00  ok
    Execution: completed  929937be52fe4442957796995941acdc

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:30:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:16:05.654315+02:00  ok
    Execution: completed  dc499ca8743746a1aa6dc214a6d39fb7

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:25:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:16:05.952350+02:00  ok
    Execution: claimed  deb377187e184059af78277d6eb7b181

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:25:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:16:06.337911+02:00  ok
    Execution: claimed  68c1122a7ab74825bad28970fa5a38e0

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:25:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:16:06.761071+02:00  ok
    Execution: claimed  d9a3e4a789ae4687b2e359950216571d

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:30:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:10:48.263375+02:00  ok
    Execution: claimed  02d07d7e694a49b58c7695be055cf021

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:30:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:10:48.602625+02:00  ok
    Execution: claimed  9d81cc2132db436bab32b427da219b38

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:30:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:16:07.176344+02:00  ok
    Execution: completed  b3bcfa97daee422e96c8ff1d26281c86

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:30:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:16:07.509951+02:00  ok
    Execution: completed  4501d23c6add402a9c3d3e642c23a4ca

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:30:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:01:26.543751+02:00  ok
    Execution: completed  b67733bcfcfa491ea193ecc31a8e0410

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:30:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:01:26.970109+02:00  ok
    Execution: completed  e72bad79350649f68b27102dc8e5d82d

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T12:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:01:27.300730+02:00  ok
    Execution: completed  99b1d471991c4bd782fd04981eddaccd

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:30:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:01:29.048254+02:00  ok
    Execution: completed  8471669d4f4a407c96eedc24dabfecd9

  c0d0d4bc8275 [active]
    Name:      ramshield-dispatcher
    Schedule:  30 1 * * *
    Repeat:    ∞
    Next run:  2026-08-30T01:30:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T10:24:07.440397+02:00  ok
    Execution: completed  9ac54019d858492b8a32260955c2f378

  26862e70b8a0 [active]
    Name:      ramshield-error-healer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:30:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T11:01:29.323455+02:00  ok
    Execution: completed  906789b5da544438842bafc360161ca5

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T12:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-29T11:00:44.679609+02:00  ok
    Execution: completed  0927f873b253486c91c6ab1fe039a543

  77b73c6cddb4 [active]
    Name:      scalper-daily-morning
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-30T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-29T10:08:42.631118+02:00  ok
    Execution: completed  e1bc7501503146cb9455d2741d7ccabc

  8ae98c997638 [active]
    Name:      ramshield-worker-T2
    Schedule:  once at 2026-08-29 11:28
    Repeat:    0/1
    Next run:  2026-08-29T11:28:34+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  c1a9e2a55714 [active]
    Name:      ramshield-worker-T3
    Schedule:  once at 2026-08-29 11:43
    Repeat:    0/1
    Next run:  2026-08-29T11:43:46+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  b4a3b9b01db6 [active]
    Name:      ramshield-backup
    Schedule:  0 2 * * *
    Repeat:    ∞
    Next run:  2026-08-30T02:00:00+02:00
    Deliver:   local
    Script:    backup_project.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
```
