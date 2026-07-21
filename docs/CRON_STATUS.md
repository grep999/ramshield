# Cron Job Status — 2026-07-21 13:09 UTC

**Live snapshot from `hermes cron list`.** 27 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 17 |
| Error | 3 |
| Running | 1 |
| Pending | 0 |
| Scheduled | 5 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ✅ ok | completed | 2026-07-21T02:00:36.463241+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error | failed | 2026-07-21T10:21:32.484248+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | ✅ ok | completed | 2026-07-21T15:02:16.923795+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ❌ error | failed | 2026-07-21T15:00:12.252688+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ✅ ok | completed | 2026-07-21T01:07:11.719033+02:00 |
| ramshield-reviewer | `0 3 * * *` | ❌ error | failed | 2026-07-21T03:20:26.751139+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 📅 scheduled | claimed | 2026-07-21T15:00:14.342876+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-07-21T15:00:14.482950+02:00 |
| ramshield-research-agent | `0 * * * *` | ✅ ok | completed | 2026-07-21T15:04:11.699383+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-07-21T15:04:29.480526+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-07-21T15:04:46.887536+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-07-21T15:04:47.031082+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-07-21T15:04:47.160648+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-07-21T15:04:47.317286+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-07-21T15:04:47.469047+02:00 |
| promo-fast-reddit | `*/10 * * * *` | ✅ ok | completed | 2026-07-21T15:04:47.625477+02:00 |
| promo-fast-x | `*/10 * * * *` | ✅ ok | completed | 2026-07-21T15:04:47.773664+02:00 |
| promo-std-devto | `*/15 * * * *` | ✅ ok | completed | 2026-07-21T15:04:47.891544+02:00 |
| promo-std-hn | `*/15 * * * *` | ✅ ok | completed | 2026-07-21T15:04:48.030451+02:00 |
| promo-deep-blog | `*/30 * * * *` | ✅ ok | completed | 2026-07-21T15:04:48.160583+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ✅ ok | completed | 2026-07-21T15:04:48.296260+02:00 |
| promo-strategic-plan | `0 * * * *` | ✅ ok | completed | 2026-07-21T15:04:48.428310+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-07-21T15:04:48.582988+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ✅ ok | completed | 2026-07-21T05:07:34.301482+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-07-21T15:04:48.734682+02:00 |
| healer-solve-facts-clippy-warnings | `once at 2026-07-21 13:02` | 🏃 running | running |  |
| healer-verify-facts-clippy-warnings | `once at 2026-07-21 13:12` | ❓ unknown |  |  |

## Raw Output

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Scheduled Jobs                                  │
└─────────────────────────────────────────────────────────────────────────┘

  74d60ec059b9 [active]
    Name:      ramshield-backup
    Schedule:  0 2 * * *
    Repeat:    ∞
    Next run:  2026-07-22T02:00:00+02:00
    Deliver:   local
    Script:    backup_project.sh
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-07-21T02:00:36.463241+02:00  ok
    Execution: completed  af2a2611a104472a8576a2e70a4cd542

  18e3993ed6a0 [active]
    Name:      RamShield Promotion Agent
    Schedule:  0 9 * * *
    Repeat:    ∞
    Next run:  2026-07-22T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion
    Last run:  2026-07-21T10:21:32.484248+02:00  error: RuntimeError: Skipped to prevent unintended spend: global inference config drifted since this job was created (provider 'custom' -> 'opencode-go'; model 'ram' -> 'kimi-k2.7-code'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=18e3993ed6a0 provider=<provider> model=<model>` (or pin the original values to keep them). See #44585.
    Execution: failed  ebdf531154f04f089c2567a4d93720fd

  e3652296ba99 [active]
    Name:      ramshield-helper-agent
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:10:00+02:00
    Deliver:   local
    Last run:  2026-07-21T15:02:16.923795+02:00  ok
    Execution: completed  ad9846c14ac0478c88803a323087cfa4

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:30:00+02:00
    Deliver:   local
    Script:    .github/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:00:12.252688+02:00  error: Script not found: /home/m/.hermes/scripts/.github/scripts/facts_collector.py
    Execution: failed  6df1bb100f194beab3a883357c271c0d

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-07-22T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T01:07:11.719033+02:00  ok
    Execution: completed  f02067789eb4419e9d85900a595f5206

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-07-22T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T03:20:26.751139+02:00  error: TimeoutError: Cron job 'ramshield-reviewer' idle for 600s (limit 600s) — last activity: waiting for non-streaming API response
    Execution: failed  554ddcb231a944ef9ab56ee3a7b2188b

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:10:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:00:14.342876+02:00  ok
    Execution: claimed  924da557bde148d2be1d5e659347a7fd

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:10:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:00:14.482950+02:00  ok
    Execution: claimed  32f6c71e2bc94a048bf9d15b32d3f585

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T16:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:11.699383+02:00  ok
    Execution: completed  60724d2a9e9342e9982845e9c3e30e89

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:15:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:29.480526+02:00  ok
    Execution: completed  527d863226294e1a9458552459aae9e4

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T16:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:46.887536+02:00  ok
    Execution: completed  557fbf4ccb774a64b1055c5272218653

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:15:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:47.031082+02:00  ok
    Execution: completed  14079fd6795e4c79853860a197e29785

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:10:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:47.160648+02:00  ok
    Execution: claimed  ed8121249de945c2ade0dd6bf16a2974

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:10:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:47.317286+02:00  ok
    Execution: claimed  666b024be6eb4b468ab2479afb789f47

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:10:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:47.469047+02:00  ok
    Execution: claimed  b938a6bfcc98445482e2e3ee40e45adc

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:10:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:47.625477+02:00  ok
    Execution: completed  93f7c6b9b88f4f45bd08d00e06a9266a

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:10:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:47.773664+02:00  ok
    Execution: completed  6dae2e9de87f479d9934bed7711b75df

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:15:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:47.891544+02:00  ok
    Execution: completed  b286b6ef029a456abe66ee08fbfca436

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:15:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:48.030451+02:00  ok
    Execution: completed  ada5ec2406b94786a8126d02fbf55a45

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:30:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:48.160583+02:00  ok
    Execution: completed  06725b8a313c44d7bd006c53b8aafa8f

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:30:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:48.296260+02:00  ok
    Execution: completed  036c53d474c0480a8850fabd391c1b76

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T16:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:48.428310+02:00  ok
    Execution: completed  78c2192635504e70b9455b0a0f5fef89

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:30:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:48.582988+02:00  ok
    Execution: completed  7cecc31180f8432b81909874f9af6b94

  c0d0d4bc8275 [active]
    Name:      ramshield-dispatcher
    Schedule:  30 1 * * *
    Repeat:    ∞
    Next run:  2026-07-22T01:30:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T05:07:34.301482+02:00  ok
    Execution: completed  bd448c908d454f96be0fc15689c6448e

  26862e70b8a0 [active]
    Name:      ramshield-error-healer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-21T15:30:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-21T15:04:48.734682+02:00  ok
    Execution: completed  391264bf67e349dda802d2da1af4e538

  663593829920 [active]
    Name:      healer-solve-facts-clippy-warnings
    Schedule:  once at 2026-07-21 13:02
    Repeat:    1/1
    Next run:  2026-07-21T13:02:37+00:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Execution: running  d18ccea6ddce4bdea8e5b0d7a544b5cc

  08483125bcb8 [active]
    Name:      healer-verify-facts-clippy-warnings
    Schedule:  once at 2026-07-21 13:12
    Repeat:    0/1
    Next run:  2026-07-21T13:12:37+00:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
```
