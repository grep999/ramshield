# Cron Job Status — 2026-08-01 09:55 UTC

**Live snapshot from `hermes cron list`.** 29 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 18 |
| Error | 3 |
| Running | 3 |
| Pending | 0 |
| Scheduled | 4 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ✅ ok | completed | 2026-08-01T06:25:49.382082+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ✅ ok | completed | 2026-08-01T09:47:49.210109+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | 🏃 running | running | 2026-08-01T11:49:28.294597+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T11:39:53.860446+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ✅ ok | completed | 2026-08-01T06:26:44.855524+02:00 |
| ramshield-reviewer | `0 3 * * *` | ✅ ok | completed | 2026-08-01T06:32:38.450509+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-01T11:50:29.054321+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T11:50:29.225242+02:00 |
| ramshield-research-agent | `0 * * * *` | ❌ error | failed | 2026-08-01T11:39:16.644600+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-08-01T11:45:43.608597+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-01T11:39:49.149970+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-08-01T11:45:43.747843+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T11:50:29.409410+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T11:50:29.576390+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T11:50:29.703789+02:00 |
| promo-fast-reddit | `*/10 * * * *` | ✅ ok | completed | 2026-08-01T11:50:29.859285+02:00 |
| promo-fast-x | `*/10 * * * *` | ✅ ok | completed | 2026-08-01T11:50:30.001480+02:00 |
| promo-std-devto | `*/15 * * * *` | ✅ ok | completed | 2026-08-01T11:45:44.397164+02:00 |
| promo-std-hn | `*/15 * * * *` | ✅ ok | completed | 2026-08-01T11:45:44.572236+02:00 |
| promo-deep-blog | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T11:39:50.654678+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T11:39:50.804082+02:00 |
| promo-strategic-plan | `0 * * * *` | ✅ ok | completed | 2026-08-01T11:39:50.979934+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T11:39:51.396462+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error | failed | 2026-08-01T06:37:29.253355+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T11:39:51.589118+02:00 |
| scalper-hourly | `0 * * * *` | ❌ error | failed | 2026-08-01T11:10:25.174539+02:00 |
| scalper-daily-morning | `0 6 * * *` | ✅ ok | completed | 2026-08-01T06:27:10.108879+02:00 |
| hourly_scalper_check | `*/1 * * * *` | 🏃 running | running | 2026-08-01T11:49:25.617777+02:00 |
| morning_scalper_check | `0 6 * * *` | ❓ unknown |  |  |

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
    Next run:  2026-08-02T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion
    Last run:  2026-08-01T09:47:49.210109+02:00  ok
    Execution: completed  b1a3cf8d3f2f4ff19991daeff21e9657

  e3652296ba99 [active]
    Name:      ramshield-helper-agent
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Last run:  2026-08-01T11:49:28.294597+02:00  error: TimeoutError: Cron job 'ramshield-helper-agent' idle for 600s (limit 600s) — last activity: waiting for non-streaming API response
    Execution: running  b9358bbf2ca642b48ca60c58cd2f1633

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:39:53.860446+02:00  ok
    Execution: completed  1ea54b7beebc447d9393f5a2318ee2ac

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
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:50:29.054321+02:00  ok
    Execution: running  9787d265f7424dd19abc302894c26592

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:50:29.225242+02:00  ok
    Execution: claimed  b715970469404dd8bbb80b5866b9826e

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:39:16.644600+02:00  error: TimeoutError: Cron job 'ramshield-research-agent' idle for 600s (limit 600s) — last activity: waiting for non-streaming API response
    Execution: failed  e59ef7695b384112a8e5d97a348090dc

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:45:43.608597+02:00  ok
    Execution: completed  e37f49c023424b889a80bc39f1dba90f

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:39:49.149970+02:00  ok
    Execution: completed  e519bda93eba4ea1b19f2603db2fbd2e

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:45:43.747843+02:00  ok
    Execution: completed  fef74e3a1f234873948a86ba5f035b69

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:50:29.409410+02:00  ok
    Execution: claimed  79a73196d7624cff96cdd3286bba9cf3

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:50:29.576390+02:00  ok
    Execution: claimed  6ac8834cf8144399b29731da1e901d56

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:50:29.703789+02:00  ok
    Execution: claimed  8b6e56bf7ff84554a5be7ab2c9d92fe6

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:50:29.859285+02:00  ok
    Execution: completed  5cb83b0e3d1841898b163d884471d36f

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:50:30.001480+02:00  ok
    Execution: completed  644a97a452b14da380c5e9c3ffc58a49

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:45:44.397164+02:00  ok
    Execution: completed  361417cc408045c0bad7644f5c27b5ae

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:45:44.572236+02:00  ok
    Execution: completed  c10834cacc204138adeef8adfb9387db

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:39:50.654678+02:00  ok
    Execution: completed  a25fe30ff48f42d4be482f3bc8f50ad3

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:39:50.804082+02:00  ok
    Execution: completed  92b772a18bea4f0db0960e705469a29a

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:39:50.979934+02:00  ok
    Execution: completed  ea68b28a7e554cdd9181a9bfbad961cd

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:39:51.396462+02:00  ok
    Execution: completed  e919d535a1034a0aa3fbcaf222981170

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
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T11:39:51.589118+02:00  ok
    Execution: completed  decdc5b0fe384d1185b8c6eecd2fd5b1

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T12:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Last run:  2026-08-01T11:10:25.174539+02:00  error: TimeoutError: Cron job 'scalper-hourly' idle for 600s (limit 600s) — last activity: waiting for non-streaming API response
    Execution: failed  84c746cd5b8046979901ee0f4927dfe9

  77b73c6cddb4 [active]
    Name:      scalper-daily-morning
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-02T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
    Last run:  2026-08-01T06:27:10.108879+02:00  ok
    Execution: completed  2bd9bc3f3a264c35abaaf6825b6f92a8

  8c6313d1be80 [active]
    Name:      hourly_scalper_check
    Schedule:  */1 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T11:56:00+02:00
    Deliver:   local
    Script:    scalper.py --hourly-check
    Last run:  2026-08-01T11:49:25.617777+02:00  error: TimeoutError: Cron job 'hourly_scalper_check' idle for 600s (limit 600s) — last activity: waiting for non-streaming API response
    Execution: running  254675f1afe1444cb77e5a6d1df082bc

  707cf752b0de [active]
    Name:      morning_scalper_check
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-02T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
```
