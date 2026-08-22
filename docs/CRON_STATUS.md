# Cron Job Status — 2026-08-22 18:50 UTC

**Live snapshot from `hermes cron list`.** 27 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 14 |
| Error | 5 |
| Running | 2 |
| Pending | 0 |
| Scheduled | 6 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ✅ ok |  | 2026-08-22T02:00:49.469934+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error |  | 2026-08-22T09:00:27.611117+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | 🏃 running | running | 2026-08-22T20:41:50.074401+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-22T20:30:09.718599+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ❌ error |  | 2026-08-22T01:00:03.486957+02:00 |
| ramshield-reviewer | `0 3 * * *` | ❌ error |  | 2026-08-22T03:00:24.327640+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-22T20:45:13.766539+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-22T20:45:14.000765+02:00 |
| ramshield-research-agent | `0 * * * *` | ✅ ok | completed | 2026-08-22T20:05:58.172892+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-08-22T20:45:33.200112+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-22T20:06:36.594827+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-08-22T20:45:33.467208+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-22T20:45:33.847451+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-22T20:45:34.126128+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-22T20:45:34.406413+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-22T20:40:14.594252+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-22T20:40:14.863205+02:00 |
| promo-std-devto | `*/15 * * * *` | ✅ ok | completed | 2026-08-22T20:45:34.731895+02:00 |
| promo-std-hn | `*/15 * * * *` | ✅ ok | completed | 2026-08-22T20:45:34.982764+02:00 |
| promo-deep-blog | `*/30 * * * *` | ✅ ok | completed | 2026-08-22T20:30:34.822033+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ✅ ok | completed | 2026-08-22T20:30:35.100963+02:00 |
| promo-strategic-plan | `0 * * * *` | ✅ ok | completed | 2026-08-22T20:06:40.010016+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-22T20:30:36.934095+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error |  | 2026-08-22T01:30:39.699385+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-22T20:30:37.270430+02:00 |
| scalper-hourly | `0 * * * *` | ✅ ok | completed | 2026-08-22T20:00:07.156815+02:00 |
| scalper-daily-morning | `0 6 * * *` | ❌ error |  | 2026-08-22T06:00:55.923877+02:00 |

## Raw Output

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Scheduled Jobs                                  │
└─────────────────────────────────────────────────────────────────────────┘

  74d60ec059b9 [active]
    Name:      ramshield-backup
    Schedule:  0 2 * * *
    Repeat:    ∞
    Next run:  2026-08-23T02:00:00+02:00
    Deliver:   local
    Script:    backup_project.sh
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-22T02:00:49.469934+02:00  ok

  18e3993ed6a0 [active]
    Name:      RamShield Promotion Agent
    Schedule:  0 9 * * *
    Repeat:    ∞
    Next run:  2026-08-23T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion
    Last run:  2026-08-22T09:00:27.611117+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=18e3993ed6a0 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  e3652296ba99 [active]
    Name:      ramshield-helper-agent
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Last run:  2026-08-22T20:41:50.074401+02:00  ok
    Execution: running  83bc4d59d1834da4b041dffdab7557a7

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:30:09.718599+02:00  ok
    Execution: completed  3b4675a0fa9e43569711515685b6e673

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-08-23T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T01:00:03.486957+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=cd22edb2d5f2 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-08-23T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T03:00:24.327640+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=d72f32a35099 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T20:55:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:45:13.766539+02:00  ok
    Execution: running  202e2c300de047a6b6f2e97893c3ffd0

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T20:55:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:45:14.000765+02:00  ok
    Execution: claimed  446623ce34bf4128adcc64f18b361a94

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:05:58.172892+02:00  ok
    Execution: completed  fa400ea0cd3a4076bd61cf4e35dcef1a

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:45:33.200112+02:00  ok
    Execution: completed  0131e19ee52c4e56a0f5bb09719c5a3b

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:06:36.594827+02:00  ok
    Execution: completed  9756e8eeb0f941afbf21f92a04b4b867

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:45:33.467208+02:00  ok
    Execution: completed  90e0517969f440f9972eb28c4071906a

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T20:55:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:45:33.847451+02:00  ok
    Execution: claimed  c6b253931d8c4b99aafc0e4d1361bcbd

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T20:55:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:45:34.126128+02:00  ok
    Execution: claimed  656b5e833afe4ac88b0917b299fd6a3d

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T20:55:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:45:34.406413+02:00  ok
    Execution: claimed  031433274a93438599e4402bf42de705

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:40:14.594252+02:00  ok
    Execution: claimed  c4661f308f8f4c2fa0f9545fd12a9fc0

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:40:14.863205+02:00  ok
    Execution: claimed  6ebfd2ebf9bc48818ebda3bff66e5375

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:45:34.731895+02:00  ok
    Execution: completed  6a175092351146448df8c0ae5d673ca8

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:45:34.982764+02:00  ok
    Execution: completed  928a611cc3d6428c9c53054eeecf897e

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:30:34.822033+02:00  ok
    Execution: completed  a5a0cffcdeb14f1ba5a2367c94283712

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:30:35.100963+02:00  ok
    Execution: completed  8a76137d28a84dc88440d05b7468d291

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:06:40.010016+02:00  ok
    Execution: completed  99e58f75a1bb475182e4daff8d0d8dda

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:30:36.934095+02:00  ok
    Execution: completed  819a53fcc29c4e07955eee156691602a

  c0d0d4bc8275 [active]
    Name:      ramshield-dispatcher
    Schedule:  30 1 * * *
    Repeat:    ∞
    Next run:  2026-08-23T01:30:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T01:30:39.699385+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (provider 'opencode-go' -> 'custom'; model 'kimi-k2.7-code' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=c0d0d4bc8275 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  26862e70b8a0 [active]
    Name:      ramshield-error-healer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T20:30:37.270430+02:00  ok
    Execution: completed  98029cc65f714d46bd91af134bb1f046

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T21:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-22T20:00:07.156815+02:00  ok
    Execution: completed  4c7df987f53e49e0a60ee7d8cb5de73b

  77b73c6cddb4 [active]
    Name:      scalper-daily-morning
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-23T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-22T06:00:55.923877+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=77b73c6cddb4 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
```
