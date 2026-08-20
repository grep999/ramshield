# Cron Job Status — 2026-08-20 17:50 UTC

**Live snapshot from `hermes cron list`.** 29 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 7 |
| Error | 15 |
| Running | 1 |
| Pending | 0 |
| Scheduled | 6 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ✅ ok |  | 2026-08-20T03:55:08.098421+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error |  | 2026-08-20T09:00:54.751716+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | ❌ error | failed | 2026-08-20T19:50:52.684489+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-20T19:30:49.175207+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ❌ error |  | 2026-08-20T03:54:56.809960+02:00 |
| ramshield-reviewer | `0 3 * * *` | ❌ error |  | 2026-08-20T03:54:57.574285+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-20T19:45:53.617198+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T19:45:53.902385+02:00 |
| ramshield-research-agent | `0 * * * *` | ❌ error | failed | 2026-08-20T19:00:48.198395+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-08-20T19:46:11.570179+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-20T19:01:25.309248+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-08-20T19:46:11.832548+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T19:46:12.058133+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T19:46:12.301094+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T19:46:12.601380+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-20T19:40:54.088692+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-20T19:40:54.361617+02:00 |
| promo-std-devto | `*/15 * * * *` | ❌ error | failed | 2026-08-20T19:46:12.848331+02:00 |
| promo-std-hn | `*/15 * * * *` | ❌ error | failed | 2026-08-20T19:46:13.118861+02:00 |
| promo-deep-blog | `*/30 * * * *` | ❌ error | failed | 2026-08-20T19:31:11.564401+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ❌ error | failed | 2026-08-20T19:31:11.824087+02:00 |
| promo-strategic-plan | `0 * * * *` | ❌ error | failed | 2026-08-20T19:01:28.598469+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-20T19:31:13.467705+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error |  | 2026-08-20T03:55:40.782944+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-20T19:31:13.726256+02:00 |
| scalper-hourly | `0 * * * *` | ❌ error | failed | 2026-08-20T19:00:46.365760+02:00 |
| scalper-daily-morning | `0 6 * * *` | ❌ error |  | 2026-08-20T06:00:19.734962+02:00 |
| hourly_scalper_check | `*/1 * * * *` | ❌ error | failed | 2026-08-20T19:50:52.717922+02:00 |
| morning_scalper_check | `0 6 * * *` | ❌ error |  | 2026-08-20T06:00:19.922556+02:00 |

## Raw Output

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Scheduled Jobs                                  │
└─────────────────────────────────────────────────────────────────────────┘

  74d60ec059b9 [active]
    Name:      ramshield-backup
    Schedule:  0 2 * * *
    Repeat:    ∞
    Next run:  2026-08-21T02:00:00+02:00
    Deliver:   local
    Script:    backup_project.sh
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-20T03:55:08.098421+02:00  ok

  18e3993ed6a0 [active]
    Name:      RamShield Promotion Agent
    Schedule:  0 9 * * *
    Repeat:    ∞
    Next run:  2026-08-21T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion
    Last run:  2026-08-20T09:00:54.751716+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=18e3993ed6a0 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  e3652296ba99 [active]
    Name:      ramshield-helper-agent
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Last run:  2026-08-20T19:50:52.684489+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=e3652296ba99 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  d495e3d335844dd49b32e9e2d36ea00a

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:30:49.175207+02:00  ok
    Execution: completed  62d221d7fdbe4188b41c2c0a1d2792fe

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-08-21T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T03:54:56.809960+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=cd22edb2d5f2 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-08-21T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T03:54:57.574285+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=d72f32a35099 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T19:55:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:45:53.617198+02:00  ok
    Execution: running  5656f6a7731e4e4a895393630ad08dc1

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T19:55:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:45:53.902385+02:00  ok
    Execution: claimed  e8903cd968f1469f8e87d73e64e0c465

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:00:48.198395+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=f270eaf2c891 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  d487ef24d1704443b33853837f2e8e8e

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:46:11.570179+02:00  ok
    Execution: completed  035dccabca894ba89b5184190fdba39f

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:01:25.309248+02:00  ok
    Execution: completed  21e6b78f48104eebb311d60bfb93dceb

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:46:11.832548+02:00  ok
    Execution: completed  1c328d53f59c491ebbd2566a7fe08683

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T19:55:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:46:12.058133+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  01c24c5e772a4fc18457f56f995d76b4

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T19:55:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:46:12.301094+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  f8e8b18f0f3f4a04896f9079e99fe757

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T19:55:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:46:12.601380+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  0b5b70bd97bc466b94569e284746ab43

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:40:54.088692+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  28067caccffc4a2497faab03532b4eaa

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:40:54.361617+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  db9bc8161c9340a0aabffa8b449cee02

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:46:12.848331+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  b6a6f27097394c30a2b2c9acdfe2f607

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:46:13.118861+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  d2edbc1581f24dd29e32200987044779

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:31:11.564401+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  4cfe1c90d2e1423e8572dffcbab29c73

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:31:11.824087+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  79319206dc5b4950a9696356892fb9e9

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:01:28.598469+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  f9d57c7ede04425c93f51ae960827a62

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:31:13.467705+02:00  ok
    Execution: completed  ed9bbd9c5aa144a28e16e14d837e3d62

  c0d0d4bc8275 [active]
    Name:      ramshield-dispatcher
    Schedule:  30 1 * * *
    Repeat:    ∞
    Next run:  2026-08-21T01:30:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T03:55:40.782944+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (provider 'opencode-go' -> 'custom'; model 'kimi-k2.7-code' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=c0d0d4bc8275 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  26862e70b8a0 [active]
    Name:      ramshield-error-healer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T19:31:13.726256+02:00  ok
    Execution: completed  64b72630293f45fdbcc1ef9dfe733e61

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T20:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Last run:  2026-08-20T19:00:46.365760+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=eef10d21be44 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  b2543624a7614fa1823825d2d72e1a3b

  77b73c6cddb4 [active]
    Name:      scalper-daily-morning
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-21T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
    Last run:  2026-08-20T06:00:19.734962+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=77b73c6cddb4 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  8c6313d1be80 [active]
    Name:      hourly_scalper_check
    Schedule:  */1 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T19:51:00+02:00
    Deliver:   local
    Script:    scalper.py --hourly-check
    Last run:  2026-08-20T19:50:52.717922+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=8c6313d1be80 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  5edea925a17a45c38fb9e10b986029d4

  707cf752b0de [active]
    Name:      morning_scalper_check
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-21T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
    Last run:  2026-08-20T06:00:19.922556+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=707cf752b0de provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
```
