# Cron Job Status — 2026-08-20 15:20 UTC

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
| ramshield-helper-agent | `*/10 * * * *` | ❌ error | failed | 2026-08-20T17:20:28.979140+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-20T17:00:24.940305+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ❌ error |  | 2026-08-20T03:54:56.809960+02:00 |
| ramshield-reviewer | `0 3 * * *` | ❌ error |  | 2026-08-20T03:54:57.574285+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-20T17:15:30.427737+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T17:15:31.033609+02:00 |
| ramshield-research-agent | `0 * * * *` | ❌ error | failed | 2026-08-20T17:00:28.256334+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-08-20T17:15:48.767837+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-20T17:01:02.131792+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-08-20T17:15:49.066159+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T17:15:49.416691+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T17:15:49.811048+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T17:15:50.069123+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-20T17:10:30.607617+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-20T17:10:30.988481+02:00 |
| promo-std-devto | `*/15 * * * *` | ❌ error | failed | 2026-08-20T17:15:50.336847+02:00 |
| promo-std-hn | `*/15 * * * *` | ❌ error | failed | 2026-08-20T17:15:50.668422+02:00 |
| promo-deep-blog | `*/30 * * * *` | ❌ error | failed | 2026-08-20T17:01:05.099861+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ❌ error | failed | 2026-08-20T17:01:05.569149+02:00 |
| promo-strategic-plan | `0 * * * *` | ❌ error | failed | 2026-08-20T17:01:05.935309+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-20T17:01:07.596912+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error |  | 2026-08-20T03:55:40.782944+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-20T17:01:07.872948+02:00 |
| scalper-hourly | `0 * * * *` | ❌ error | failed | 2026-08-20T17:00:26.195912+02:00 |
| scalper-daily-morning | `0 6 * * *` | ❌ error |  | 2026-08-20T06:00:19.734962+02:00 |
| hourly_scalper_check | `*/1 * * * *` | ❌ error | failed | 2026-08-20T17:20:29.035696+02:00 |
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
    Next run:  2026-08-20T17:30:00+02:00
    Deliver:   local
    Last run:  2026-08-20T17:20:28.979140+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=e3652296ba99 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  e2e82119e0d346c9a18586c066c3cf00

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:30:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:00:24.940305+02:00  ok
    Execution: completed  014972ab167d485c9fe6850bd2a39a1d

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
    Next run:  2026-08-20T17:25:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:15:30.427737+02:00  ok
    Execution: running  77549ac7b19b4f9bb0b60746e2c20582

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:25:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:15:31.033609+02:00  ok
    Execution: claimed  9ce2983c1cf94b8db1ccd9f68c17641b

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T18:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:00:28.256334+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=f270eaf2c891 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  ff5d629fc2744eafa35de9d0c9a5cce6

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:30:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:15:48.767837+02:00  ok
    Execution: completed  d38c940ad2a6487b8bbeb39877a3ae20

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T18:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:01:02.131792+02:00  ok
    Execution: completed  f29067bd2b4843b0b0abdbf93b6af6a4

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:30:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:15:49.066159+02:00  ok
    Execution: completed  493253e65bb04ba0a6ec2a412b4acc9d

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:25:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:15:49.416691+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  37cdb29e672540a987b10ff979785743

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:25:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:15:49.811048+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  22861b77bfd448aeb50a1a8245b49189

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:25:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:15:50.069123+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  4d06f752a42b43aea5e33eb55acbff44

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:30:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:10:30.607617+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  3180ccd490414bf393e9cef311af3dda

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:30:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:10:30.988481+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  b7a67abc91b54195aa73fb1ba19ea0da

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:30:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:15:50.336847+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  d654a5855bd34245a385f9ab0909ce1d

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:30:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:15:50.668422+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  31ef8276af474a928983c4b21c678d4c

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:30:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:01:05.099861+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  16987d8e28c844948fed2fad6db43747

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:30:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:01:05.569149+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  0dda06f7fbdd476e9f2df7e3e5ab545f

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T18:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:01:05.935309+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  f4ac3f365865456680f53b54c390a13c

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T17:30:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:01:07.596912+02:00  ok
    Execution: completed  644c18ab6b1c42598878f66b39005d7b

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
    Next run:  2026-08-20T17:30:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T17:01:07.872948+02:00  ok
    Execution: completed  df1431b325eb4266b58b3244f0fbfd6c

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T18:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Last run:  2026-08-20T17:00:26.195912+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=eef10d21be44 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  1c26c94ff6464e04b321aa4df9195558

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
    Next run:  2026-08-20T17:21:00+02:00
    Deliver:   local
    Script:    scalper.py --hourly-check
    Last run:  2026-08-20T17:20:29.035696+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=8c6313d1be80 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  8065b20c6db34e199e5d692bc85aade4

  707cf752b0de [active]
    Name:      morning_scalper_check
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-21T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
    Last run:  2026-08-20T06:00:19.922556+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=707cf752b0de provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
```
