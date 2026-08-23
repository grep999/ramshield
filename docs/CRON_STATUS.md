# Cron Job Status — 2026-08-23 10:40 UTC

**Live snapshot from `hermes cron list`.** 27 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 18 |
| Error | 1 |
| Running | 2 |
| Pending | 0 |
| Scheduled | 6 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ❌ error | failed | 2026-08-23T10:50:28.698487+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ✅ ok | completed | 2026-08-23T11:02:43.079434+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | 🏃 running | running | 2026-08-23T12:30:42.483778+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-23T12:30:07.496465+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ✅ ok | unknown | 2026-08-23T11:07:30.365940+02:00 |
| ramshield-reviewer | `0 3 * * *` | ✅ ok | completed | 2026-08-23T11:12:08.711747+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-23T12:35:09.765036+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-23T12:35:10.049814+02:00 |
| ramshield-research-agent | `0 * * * *` | ✅ ok | completed | 2026-08-23T12:03:27.183585+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-08-23T12:30:29.736713+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-23T12:04:06.336056+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-08-23T12:30:30.154075+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-23T12:35:10.370460+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-23T12:35:10.751874+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-23T12:35:11.036850+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-23T12:30:31.328103+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-23T12:30:31.563468+02:00 |
| promo-std-devto | `*/15 * * * *` | ✅ ok | completed | 2026-08-23T12:30:31.824054+02:00 |
| promo-std-hn | `*/15 * * * *` | ✅ ok | completed | 2026-08-23T12:30:32.063249+02:00 |
| promo-deep-blog | `*/30 * * * *` | ✅ ok | completed | 2026-08-23T12:30:32.313725+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ✅ ok | completed | 2026-08-23T12:30:32.572439+02:00 |
| promo-strategic-plan | `0 * * * *` | ✅ ok | completed | 2026-08-23T12:04:09.546510+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-23T12:30:34.503026+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ✅ ok | completed | 2026-08-23T11:21:10.559694+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-23T12:30:34.899975+02:00 |
| scalper-hourly | `0 * * * *` | ✅ ok | completed | 2026-08-23T12:00:04.673458+02:00 |
| scalper-daily-morning | `0 6 * * *` | ✅ ok | completed | 2026-08-23T10:49:57.804235+02:00 |

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
    Next run:  2026-08-23T12:50:00+02:00
    Deliver:   local
    Last run:  2026-08-23T12:30:42.483778+02:00  ok
    Execution: running  2777b526d3914f15914e862f653d8603

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T13:00:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:30:07.496465+02:00  ok
    Execution: completed  4e510991ccd14083a742dd33b505ddef

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
    Next run:  2026-08-23T12:45:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:35:09.765036+02:00  ok
    Execution: running  edeb4f812b024405aebf9ebfffeaeb5e

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:45:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:35:10.049814+02:00  ok
    Execution: claimed  60dfef515faf40799997f798185ddb9d

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T13:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:03:27.183585+02:00  ok
    Execution: completed  0b9ef02b48ab45c1a00b8d3ec3092b28

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:45:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:30:29.736713+02:00  ok
    Execution: completed  2f9b4f80426d43cd85c4727ed200a98e

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T13:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:04:06.336056+02:00  ok
    Execution: completed  6c3650603b6649389c1080ef57f8cd6c

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:45:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:30:30.154075+02:00  ok
    Execution: completed  cc5ff3fba00542eca2062f350cd82f56

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:45:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:35:10.370460+02:00  ok
    Execution: claimed  588a2a38979740c28ea6f6e3d38d75dd

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:45:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:35:10.751874+02:00  ok
    Execution: claimed  f755964b7b2840b59ba466c7974f14cc

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:45:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:35:11.036850+02:00  ok
    Execution: claimed  2460a264790349f0b1bc9dd44665319f

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:50:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:30:31.328103+02:00  ok
    Execution: claimed  4fadd9b1dc4f4f06baa8ec8240eb0b34

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:50:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:30:31.563468+02:00  ok
    Execution: claimed  7f32f213a0b5483c8c2c084791288a54

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:45:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:30:31.824054+02:00  ok
    Execution: completed  8e5a33be279f48e1994d8201e6533165

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T12:45:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:30:32.063249+02:00  ok
    Execution: completed  ebda3307eded45629494fe0bd082872f

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T13:00:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:30:32.313725+02:00  ok
    Execution: completed  6e7a96c4c99b4b8bace2d31f1842ece3

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T13:00:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:30:32.572439+02:00  ok
    Execution: completed  b441844c3f5b4f6fb436a673f6af11fd

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T13:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:04:09.546510+02:00  ok
    Execution: completed  4ffaeada29574c009767d8e7e1fc0ab8

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T13:00:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:30:34.503026+02:00  ok
    Execution: completed  2c962be627e744b18d7f8d504e3ab233

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
    Next run:  2026-08-23T13:00:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T12:30:34.899975+02:00  ok
    Execution: completed  3c0f5fb9119e4de5af40a2306af3b46a

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-23T13:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-23T12:00:04.673458+02:00  ok
    Execution: completed  0999719120894db4a36c310a03ad9a36

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
```
