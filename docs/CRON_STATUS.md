# Cron Job Status — 2026-08-30 07:25 UTC

**Live snapshot from `hermes cron list`.** 27 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 19 |
| Error | 0 |
| Running | 1 |
| Pending | 0 |
| Scheduled | 4 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| RamShield Promotion Agent | `0 9 * * *` | ✅ ok | completed | 2026-08-30T09:06:20.857722+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-30T09:06:32.256624+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ✅ ok | completed | 2026-08-30T08:24:21.712795+02:00 |
| ramshield-reviewer | `0 3 * * *` | ✅ ok | completed | 2026-08-30T08:27:45.793410+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-30T09:20:52.339640+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-30T09:20:52.755430+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-08-30T09:16:26.782765+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-30T09:07:47.916574+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-30T09:20:53.129400+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-30T09:20:53.449589+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-30T09:20:53.751373+02:00 |
| promo-fast-reddit | `*/10 * * * *` | ✅ ok | completed | 2026-08-30T09:20:54.084172+02:00 |
| promo-fast-x | `*/10 * * * *` | ✅ ok | completed | 2026-08-30T09:20:54.327734+02:00 |
| promo-std-devto | `*/15 * * * *` | ✅ ok | completed | 2026-08-30T09:16:27.851174+02:00 |
| promo-std-hn | `*/15 * * * *` | ✅ ok | completed | 2026-08-30T09:16:28.115725+02:00 |
| promo-deep-blog | `*/30 * * * *` | ✅ ok | completed | 2026-08-30T09:07:51.094390+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ✅ ok | completed | 2026-08-30T09:07:51.387972+02:00 |
| promo-strategic-plan | `0 * * * *` | ✅ ok | completed | 2026-08-30T09:07:51.726413+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-30T09:07:58.358332+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ✅ ok | completed | 2026-08-30T08:33:50.908498+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-30T09:07:58.637535+02:00 |
| scalper-hourly | `0 * * * *` | ✅ ok | completed | 2026-08-30T09:00:38.834696+02:00 |
| scalper-daily-morning | `0 6 * * *` | ✅ ok | completed | 2026-08-30T08:19:34.673407+02:00 |
| ramshield-backup | `0 2 * * *` | ✅ ok | completed | 2026-08-30T08:33:51.545611+02:00 |
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
    Next run:  2026-08-31T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion
    Last run:  2026-08-30T09:06:20.857722+02:00  ok
    Execution: completed  a72e2ffb5ffc42a99a7f564d8902c574

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:06:32.256624+02:00  ok
    Execution: completed  e4e84a70feac4fc38fdd5ad44e877042

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
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:20:52.339640+02:00  ok
    Execution: running  27bd17cd2ee74ad68f753e0f03d28f37

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:20:52.755430+02:00  ok
    Execution: claimed  612ed2f34295447d8ac7624e5448a0c8

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:16:26.782765+02:00  ok
    Execution: completed  c30da927386a4bd883ab81449ad62bb2

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T10:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:07:47.916574+02:00  ok
    Execution: completed  9637d73f4e424cbebb14ec9565a58500

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:20:53.129400+02:00  ok
    Execution: claimed  690a4740ea6a4e6a833827937d9c28cb

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:20:53.449589+02:00  ok
    Execution: claimed  31ac7b62bb1c42cab74fc5e7133294bf

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:20:53.751373+02:00  ok
    Execution: claimed  1e82ba95646b47d08e67005df1cf124e

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:20:54.084172+02:00  ok
    Execution: completed  b7c877c0ac6e4e98a4937cac0ba27cb4

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:20:54.327734+02:00  ok
    Execution: completed  1c0c6311f2614b9faeef1ca889dcc9ac

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:16:27.851174+02:00  ok
    Execution: completed  8d1363643eb94c49a85ce8e388ab04fe

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:16:28.115725+02:00  ok
    Execution: completed  6f2e516efff640bfae1bc0498a605087

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:07:51.094390+02:00  ok
    Execution: completed  972d37e324274c2ea8aa8bb184be5432

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:07:51.387972+02:00  ok
    Execution: completed  e02a1d3f58924672badea8e3e1fde66c

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T10:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:07:51.726413+02:00  ok
    Execution: completed  30e485c3d5764f65b36309dbef383bec

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:07:58.358332+02:00  ok
    Execution: completed  754041d450b64659a0d5bb3639ac8bb1

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
    Next run:  2026-08-30T09:30:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-30T09:07:58.637535+02:00  ok
    Execution: completed  9d29d51706bd47a49c9644347280a0ab

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-30T10:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-30T09:00:38.834696+02:00  ok
    Execution: completed  8136b06f71c84c3a8747ba13da6eb524

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
