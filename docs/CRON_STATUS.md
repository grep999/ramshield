# Cron Job Status — 2026-08-21 22:55 UTC

**Live snapshot from `hermes cron list`.** 29 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 7 |
| Error | 17 |
| Running | 1 |
| Pending | 0 |
| Scheduled | 4 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ✅ ok |  | 2026-08-21T08:39:59.736223+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error |  | 2026-08-21T09:00:26.987698+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | ❌ error | failed | 2026-08-22T00:50:00.804522+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-22T00:30:58.097824+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ❌ error |  | 2026-08-21T08:39:21.598831+02:00 |
| ramshield-reviewer | `0 3 * * *` | ❌ error |  | 2026-08-21T08:39:22.401426+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-22T00:50:02.832125+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-22T00:50:03.253739+02:00 |
| ramshield-research-agent | `0 * * * *` | ❌ error | failed | 2026-08-22T00:00:55.744203+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-08-22T00:46:27.060269+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-22T00:02:05.306575+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-08-22T00:46:27.354308+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-22T00:50:03.771999+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-22T00:50:04.179300+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-22T00:50:04.530025+02:00 |
| promo-fast-reddit | `*/10 * * * *` | ❌ error | failed | 2026-08-22T00:50:04.875117+02:00 |
| promo-fast-x | `*/10 * * * *` | ❌ error | failed | 2026-08-22T00:50:05.289969+02:00 |
| promo-std-devto | `*/15 * * * *` | ❌ error | failed | 2026-08-22T00:46:28.409707+02:00 |
| promo-std-hn | `*/15 * * * *` | ❌ error | failed | 2026-08-22T00:46:28.686480+02:00 |
| promo-deep-blog | `*/30 * * * *` | ❌ error | failed | 2026-08-22T00:31:46.840904+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ❌ error | failed | 2026-08-22T00:31:47.337873+02:00 |
| promo-strategic-plan | `0 * * * *` | ❌ error | failed | 2026-08-22T00:02:10.588511+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-22T00:31:50.679893+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error |  | 2026-08-21T08:40:13.560974+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-22T00:31:51.354023+02:00 |
| scalper-hourly | `0 * * * *` | ❌ error | failed | 2026-08-22T00:00:53.383410+02:00 |
| scalper-daily-morning | `0 6 * * *` | ❌ error |  | 2026-08-21T08:39:23.250029+02:00 |
| hourly_scalper_check | `*/1 * * * *` | ❌ error | failed | 2026-08-22T00:55:01.178627+02:00 |
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
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Last run:  2026-08-22T00:50:00.804522+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=e3652296ba99 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  a4c7e78cb69d449bb26f0b483a0cff36

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:30:58.097824+02:00  ok
    Execution: completed  a8c6ff1e05674efe924b0605d461443e

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
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:50:02.832125+02:00  ok
    Execution: running  762c3ee8b51f4137ad1bcebea454c287

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:50:03.253739+02:00  ok
    Execution: claimed  cec2e4e5bc41490ca400f6512c0ed866

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:00:55.744203+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=f270eaf2c891 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  ab9c642f40034cc0b87a12e84d9a99fc

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:46:27.060269+02:00  ok
    Execution: completed  fb7d43d862474f579729ed1bd2bbfad3

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:02:05.306575+02:00  ok
    Execution: completed  a0c20ec40707463f85c6d6d3c470e61e

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:46:27.354308+02:00  ok
    Execution: completed  957bbdb277f3478680a91aab35b57edf

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:50:03.771999+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  43ed8960b76443c489abdb3af07fd22d

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:50:04.179300+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  11e876b5531b4a04b6371adac036339b

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:50:04.530025+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  5564d13e187a4718942e43fab911b27f

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:50:04.875117+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  b6e9c67f093a41fdb0af260418580910

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:50:05.289969+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  700da744156f4999852ceac1b0326ee6

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:46:28.409707+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  22c3212f87a344d59bf3f4f26595e4fc

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:46:28.686480+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  6ea5fd58bdb44a05b1360c189a7962fd

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:31:46.840904+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  ad76dea556ba43158e0fb997db939e10

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:31:47.337873+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  0a09fd0e37b44bcf8f5380ece47140c4

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:02:10.588511+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  f4195105acb24493879b01cc8ef13125

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:31:50.679893+02:00  ok
    Execution: completed  4a39ca27faf94a8f8568711f0135bd26

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
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-22T00:31:51.354023+02:00  ok
    Execution: completed  e8008de9d48a42d592e885df7007f8ef

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-22T01:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Last run:  2026-08-22T00:00:53.383410+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=eef10d21be44 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  fb571975ea05430fa3a751fa2da9d0a3

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
    Next run:  2026-08-22T00:56:00+02:00
    Deliver:   local
    Script:    scalper.py --hourly-check
    Last run:  2026-08-22T00:55:01.178627+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=8c6313d1be80 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  314c0a644aef4ef583ab9ad46d69ed72

  707cf752b0de [active]
    Name:      morning_scalper_check
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-22T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
    Last run:  2026-08-21T08:39:23.466865+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=707cf752b0de provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
```
