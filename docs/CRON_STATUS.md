# Cron Job Status — 2026-08-23 09:31 UTC

**Live snapshot from `hermes cron list`.** 30 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 10 |
| Error | 1 |
| Running | 2 |
| Pending | 0 |
| Scheduled | 14 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ❌ error | failed | 2026-08-23T10:50:28.698487+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ✅ ok | completed | 2026-08-23T11:02:43.079434+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | 🏃 running | running | 2026-08-23T11:23:06.440691+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-23T11:31:00.054877+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ✅ ok | unknown | 2026-08-23T11:07:30.365940+02:00 |
| ramshield-reviewer | `0 3 * * *` | ✅ ok | completed | 2026-08-23T11:12:08.711747+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-23T11:26:03.263043+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:26:03.765892+02:00 |
| ramshield-research-agent | `0 * * * *` | ✅ ok | completed | 2026-08-23T11:17:21.630495+02:00 |
| ramshield-health-loop | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:17:41.093833+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-23T11:17:59.994970+02:00 |
| ramshield-git-automation | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:18:00.267976+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:26:04.241685+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:26:04.703236+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:26:05.150560+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:21:14.889061+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:21:15.144121+02:00 |
| promo-std-devto | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:18:02.108045+02:00 |
| promo-std-hn | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:18:02.464295+02:00 |
| promo-deep-blog | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:18:02.739282+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:18:03.004265+02:00 |
| promo-strategic-plan | `0 * * * *` | ✅ ok | completed | 2026-08-23T11:18:03.320725+02:00 |
| promo-reviewer | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:18:05.367133+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ✅ ok | completed | 2026-08-23T11:21:10.559694+02:00 |
| ramshield-error-healer | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-23T11:21:10.832025+02:00 |
| scalper-hourly | `0 * * * *` | ✅ ok | completed | 2026-08-23T11:00:58.442203+02:00 |
| scalper-daily-morning | `0 6 * * *` | ✅ ok | completed | 2026-08-23T10:49:57.804235+02:00 |
| ramshield-worker-T1 | `once at 2026-08-23 11:45` | ❓ unknown |  |  |
| ramshield-worker-T2 | `once at 2026-08-23 12:00` | ❓ unknown |  |  |
| ramshield-worker-T3 | `once at 2026-08-23 12:15` | ❓ unknown |  |  |

## Raw Output

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Scheduled Jobs                                  │
└─────────────────────────────────────────────────────────────────────────┘

  74d60ec059b9 [active]
    Name:      ramshield-backup
    Schedule:  0 2 * * *
    Repeat:    ∞
    Next run:  2026-08-24T02:00:00+02:00
    Deliver:   local
    Script:    backup_project.sh
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-23T10:50:28.698487+02:00  error: Script exited with code 1
stderr:
tar: ./.git: file changed as we read it
stdout:
Starting backup at Sun 23 Aug 10:49:56 CEST 2026
Project dir: /home/m/vehicle_of_rationalism/ramshield/beta/rs
Backup dir: /home/m/vehicle_of_rationalism/ramshield/beta/rs/backups
    Execution: failed  16266227a8ba46ef9bdb4b33256b1c53

  18e3993ed6a0 [active]
    Name:      RamShield Promotion Agent
    Schedule:  0 9 * * *
    Repeat:    ∞
    Next run:  2026-08-24T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion
    Last run:  2026-08-23T11:02:43.079434+02:00  ok
    Execution: completed  82ba15842af54fea83b73429489a8422

  e3652296ba99 [active]
    Name:      ramshield-helper-agent
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T11:40:00+02:00
    Deliver:   local
    Last run:  2026-08-23T11:23:06.440691+02:00  ok
    Execution: running  7aae3d566a954ddba63af9d36b39bc75

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:00:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:31:00.054877+02:00  ok
    Execution: completed  8e7aa77c104d4689bd5b95441171cd65

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-08-24T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:07:30.365940+02:00  ok
    Execution: unknown  aff89f469390475ca50ec5de7be5f7d3

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-08-24T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:12:08.711747+02:00  ok
    Execution: completed  c5319f273151473aac3c480b933b7099

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T11:35:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:26:03.263043+02:00  ok
    Execution: running  907a4751231f4557a3c15f8c4343f657

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T11:35:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:26:03.765892+02:00  ok
    Execution: claimed  c127494100e1491b880427218dbc6424

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:17:21.630495+02:00  ok
    Execution: completed  ad9865f0bf364909baf2b03749f5eafe

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T11:45:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:17:41.093833+02:00  ok
    Execution: claimed  79767150ae3b4b7c9c6baf6e4a411b29

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:17:59.994970+02:00  ok
    Execution: completed  c020a45abd9141a891928a7e04a89702

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T11:45:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:18:00.267976+02:00  ok
    Execution: claimed  c14ad299703649adaa73fcff530deb0f

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T11:35:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:26:04.241685+02:00  ok
    Execution: claimed  493d14a8a856482ea2e004434f65bceb

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T11:35:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:26:04.703236+02:00  ok
    Execution: claimed  b67158a8b6dc44f6a07957cee44125c1

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T11:35:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:26:05.150560+02:00  ok
    Execution: claimed  bd2e158298134da9aec07710cc89e7a9

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T11:40:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:21:14.889061+02:00  ok
    Execution: claimed  6fb8412cf97b4695853a2702f7226bd3

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T11:40:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:21:15.144121+02:00  ok
    Execution: claimed  0d1a04815165425898b2c9916857a15a

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T11:45:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:18:02.108045+02:00  ok
    Execution: claimed  631ef7403a9747629d8b3f027b40286b

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T11:45:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:18:02.464295+02:00  ok
    Execution: claimed  264b0b03ceb54724a413939517a5ff34

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:00:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:18:02.739282+02:00  ok
    Execution: claimed  4f2dbb15881148a59640de9245e819e5

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:00:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:18:03.004265+02:00  ok
    Execution: claimed  7f62e6064f034dde83a45e9579ad7c19

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:18:03.320725+02:00  ok
    Execution: completed  66c0ea88b3f44b6b8d6448e8b0c1fe02

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:00:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:18:05.367133+02:00  ok
    Execution: claimed  d45e542fe6444a30942949608ea9d2d1

  c0d0d4bc8275 [active]
    Name:      ramshield-dispatcher
    Schedule:  30 1 * * *
    Repeat:    ∞
    Next run:  2026-08-24T01:30:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:21:10.559694+02:00  ok
    Execution: completed  710b67a7d4d54ae5af923e740a1531d3

  26862e70b8a0 [active]
    Name:      ramshield-error-healer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:00:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:21:10.832025+02:00  ok
    Execution: claimed  e2af30f978c24ad9aeec766b3e9a1ead

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-23T11:00:58.442203+02:00  ok
    Execution: completed  cbc9f8b797bb4634ad94648eacaf59e8

  77b73c6cddb4 [active]
    Name:      scalper-daily-morning
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-24T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-23T10:49:57.804235+02:00  ok
    Execution: completed  47e97cb5444c4e97b8ecdfa99dd7c420

  2824b63537f1 [active]
    Name:      ramshield-worker-T1
    Schedule:  once at 2026-08-23 11:45
    Repeat:    0/1
    Next run:  2026-08-23T11:45:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  9eb7f70179ce [active]
    Name:      ramshield-worker-T2
    Schedule:  once at 2026-08-23 12:00
    Repeat:    0/1
    Next run:  2026-08-23T12:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs

  c68e5a43bd69 [active]
    Name:      ramshield-worker-T3
    Schedule:  once at 2026-08-23 12:15
    Repeat:    0/1
    Next run:  2026-08-23T12:15:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
```
