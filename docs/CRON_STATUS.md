# Cron Job Status — 2026-08-21 13:50 UTC

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
| ramshield-backup | `0 2 * * *` | ✅ ok |  | 2026-08-21T08:39:59.736223+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error |  | 2026-08-21T09:00:26.987698+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | ❌ error | failed | 2026-08-21T15:50:33.541363+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-21T15:30:30.063764+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ❌ error |  | 2026-08-21T08:39:21.598831+02:00 |
| ramshield-reviewer | `0 3 * * *` | ❌ error |  | 2026-08-21T08:39:22.401426+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-21T15:45:35.833411+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-21T15:45:36.271213+02:00 |
| ramshield-research-agent | `0 * * * *` | ❌ error | failed | 2026-08-21T15:00:28.028651+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-08-21T15:46:04.333788+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-21T15:01:09.946940+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-08-21T15:46:04.669469+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-21T15:46:04.913522+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-21T15:46:05.230740+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-21T15:46:05.630503+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-21T15:40:36.500361+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-21T15:40:36.973057+02:00 |
| promo-std-devto | `*/15 * * * *` | ❌ error | failed | 2026-08-21T15:46:05.969106+02:00 |
| promo-std-hn | `*/15 * * * *` | ❌ error | failed | 2026-08-21T15:46:06.339075+02:00 |
| promo-deep-blog | `*/30 * * * *` | ❌ error | failed | 2026-08-21T15:30:55.270694+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ❌ error | failed | 2026-08-21T15:30:55.516122+02:00 |
| promo-strategic-plan | `0 * * * *` | ❌ error | failed | 2026-08-21T15:01:13.057911+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-21T15:30:57.199199+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error |  | 2026-08-21T08:40:13.560974+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-21T15:30:57.519094+02:00 |
| scalper-hourly | `0 * * * *` | ❌ error | failed | 2026-08-21T15:00:26.094631+02:00 |
| scalper-daily-morning | `0 6 * * *` | ❌ error |  | 2026-08-21T08:39:23.250029+02:00 |
| hourly_scalper_check | `*/1 * * * *` | ❌ error | failed | 2026-08-21T15:50:33.594272+02:00 |
| morning_scalper_check | `0 6 * * *` | ❌ error |  | 2026-08-21T08:39:23.466865+02:00 |

## Raw Output

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Scheduled Jobs                                  │
└─────────────────────────────────────────────────────────────────────────┘

  74d60ec059b9 [active]
    Name:      ramshield-backup
    Schedule:  0 2 * * *
    Repeat:    ∞
    Next run:  2026-08-22T02:00:00+02:00
    Deliver:   local
    Script:    backup_project.sh
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-21T08:39:59.736223+02:00  ok

  18e3993ed6a0 [active]
    Name:      RamShield Promotion Agent
    Schedule:  0 9 * * *
    Repeat:    ∞
    Next run:  2026-08-22T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion
    Last run:  2026-08-21T09:00:26.987698+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=18e3993ed6a0 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  e3652296ba99 [active]
    Name:      ramshield-helper-agent
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Last run:  2026-08-21T15:50:33.541363+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=e3652296ba99 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  c7007459f005471dbf94c8a5af0e0590

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:30:30.063764+02:00  ok
    Execution: completed  3227c1711f70438392ab08cd4af96d28

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T08:39:21.598831+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=cd22edb2d5f2 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-08-22T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T08:39:22.401426+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=d72f32a35099 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T15:55:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:45:35.833411+02:00  ok
    Execution: running  d8fae97a977942dca6b642ef7d50a6cc

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T15:55:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:45:36.271213+02:00  ok
    Execution: claimed  727255584dfe44848db23f23c76b2d47

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:00:28.028651+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=f270eaf2c891 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  86cc51d25b384ca89c466ecfa54cdfa2

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:46:04.333788+02:00  ok
    Execution: completed  50eea35b44b3405e8644e0d7d1a3fbf1

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:01:09.946940+02:00  ok
    Execution: completed  52c51d905c164150bc4e546c958ef7c9

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:46:04.669469+02:00  ok
    Execution: completed  8ae21c4827cf43139dbc95a94bbeddfd

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T15:55:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:46:04.913522+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  b4275068cd0d4ea0817cfcf7a30487b1

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T15:55:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:46:05.230740+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  d13b59e59b334aad814e313c45b0146d

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T15:55:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:46:05.630503+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  52560e5e74804da59722ee3f1d119538

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:40:36.500361+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  8a442ab0fd47499f93d9fc15c0e205ed

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:40:36.973057+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  883c3236ad7643499384f1427847702f

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:46:05.969106+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  2d0568a970514b60b4e3d32579128805

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:46:06.339075+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  c83724fe4bc9461f9d678ce708081f08

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:30:55.270694+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  c0d5ad151d61461ebe47bdf3784351d7

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:30:55.516122+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  cc42b4fa20ed4f88b5b33f46fb5014c0

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:01:13.057911+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  f6961401237940bba701f466b80343f2

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:30:57.199199+02:00  ok
    Execution: completed  13a643b1451b4e63aeaa4cbc682f4477

  c0d0d4bc8275 [active]
    Name:      ramshield-dispatcher
    Schedule:  30 1 * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:30:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T08:40:13.560974+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (provider 'opencode-go' -> 'custom'; model 'kimi-k2.7-code' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=c0d0d4bc8275 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  26862e70b8a0 [active]
    Name:      ramshield-error-healer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T15:30:57.519094+02:00  ok
    Execution: completed  47820f6a795147939f2ad8553bfbaf63

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T16:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Last run:  2026-08-21T15:00:26.094631+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=eef10d21be44 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  f00bedd236df46dca0de2422d95fdd60

  77b73c6cddb4 [active]
    Name:      scalper-daily-morning
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-22T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
    Last run:  2026-08-21T08:39:23.250029+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=77b73c6cddb4 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  8c6313d1be80 [active]
    Name:      hourly_scalper_check
    Schedule:  */1 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T15:51:00+02:00
    Deliver:   local
    Script:    scalper.py --hourly-check
    Last run:  2026-08-21T15:50:33.594272+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=8c6313d1be80 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  5ebf85dbed274ea59a35ce3e817885ca

  707cf752b0de [active]
    Name:      morning_scalper_check
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-22T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
    Last run:  2026-08-21T08:39:23.466865+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=707cf752b0de provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
```
