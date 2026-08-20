# Cron Job Status — 2026-08-20 02:50 UTC

**Live snapshot from `hermes cron list`.** 29 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 12 |
| Error | 10 |
| Running | 1 |
| Pending | 0 |
| Scheduled | 6 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ✅ ok | completed | 2026-08-20T03:55:08.098421+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error |  | 2026-08-19T09:00:50.752657+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | ❌ error | failed | 2026-08-20T04:50:06.975190+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-20T04:30:03.268345+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ❌ error | failed | 2026-08-20T03:54:56.809960+02:00 |
| ramshield-reviewer | `0 3 * * *` | ❌ error | failed | 2026-08-20T03:54:57.574285+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-20T04:45:08.133473+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:45:08.361311+02:00 |
| ramshield-research-agent | `0 * * * *` | ❌ error | failed | 2026-08-20T04:01:00.358806+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-08-20T04:45:26.310711+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-20T04:01:37.298065+02:00 |
| ramshield-git-automation | `*/15 * * * *` | ✅ ok | completed | 2026-08-20T04:45:26.539956+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:45:26.859840+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:45:27.116213+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:45:27.403299+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:40:08.813659+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:40:09.079870+02:00 |
| promo-std-devto | `*/15 * * * *` | ✅ ok | completed | 2026-08-20T04:45:27.626433+02:00 |
| promo-std-hn | `*/15 * * * *` | ✅ ok | completed | 2026-08-20T04:45:27.950143+02:00 |
| promo-deep-blog | `*/30 * * * *` | ✅ ok | completed | 2026-08-20T04:30:29.944432+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ✅ ok | completed | 2026-08-20T04:30:30.304744+02:00 |
| promo-strategic-plan | `0 * * * *` | ✅ ok | completed | 2026-08-20T04:01:40.214827+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-20T04:30:36.159511+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error | failed | 2026-08-20T03:55:40.782944+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-20T04:30:36.534243+02:00 |
| scalper-hourly | `0 * * * *` | ❌ error | failed | 2026-08-20T04:00:58.717950+02:00 |
| scalper-daily-morning | `0 6 * * *` | ❌ error |  | 2026-08-19T06:00:23.682795+02:00 |
| hourly_scalper_check | `*/1 * * * *` | ❌ error | failed | 2026-08-20T04:50:07.129540+02:00 |
| morning_scalper_check | `0 6 * * *` | ❌ error |  | 2026-08-19T06:00:23.799259+02:00 |

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
    Execution: completed  d6584cb3357d45aea8455bbbd80c54ee

  18e3993ed6a0 [active]
    Name:      RamShield Promotion Agent
    Schedule:  0 9 * * *
    Repeat:    ∞
    Next run:  2026-08-20T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion
    Last run:  2026-08-19T09:00:50.752657+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=18e3993ed6a0 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  e3652296ba99 [active]
    Name:      ramshield-helper-agent
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Last run:  2026-08-20T04:50:06.975190+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=e3652296ba99 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  4599f5065caa483f890a752c6f13b1af

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:30:03.268345+02:00  ok
    Execution: completed  fc66f7a40a404688960ed877beb9a803

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-08-21T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T03:54:56.809960+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=cd22edb2d5f2 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  eaa867cf11b74ea18b3be0a86695f1f8

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-08-21T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T03:54:57.574285+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=d72f32a35099 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  2921ee25a8d34fc5a3ea34ba8ece45d4

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T04:55:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:45:08.133473+02:00  ok
    Execution: running  f7e5dfe5d52a40df93f890341afd6d98

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T04:55:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:45:08.361311+02:00  ok
    Execution: claimed  13af0c991833415ab55f2782f4111c47

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:01:00.358806+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=f270eaf2c891 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  f9a52b72a9c54ac7a9eaba7036e3f389

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:45:26.310711+02:00  ok
    Execution: completed  adc35ebaa94947dea1dc8199ad2ef5d3

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:01:37.298065+02:00  ok
    Execution: completed  779e62ce010a4954b612e1be1329927f

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:45:26.539956+02:00  ok
    Execution: completed  01e2200f68724f02a89f6fd1703ef220

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T04:55:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:45:26.859840+02:00  ok
    Execution: claimed  3eecfd5e6c1a4e75b65ce2a255e59b5f

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T04:55:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:45:27.116213+02:00  ok
    Execution: claimed  d3b18a93b0d5461db62a6d6acff44983

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T04:55:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:45:27.403299+02:00  ok
    Execution: claimed  af7fc2d808c94ced8ade433ce3182827

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:40:08.813659+02:00  ok
    Execution: claimed  3f179f0e6880441693e29ee2db8f114f

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:40:09.079870+02:00  ok
    Execution: claimed  25272a3f06364f9abd0cd30e777abab3

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:45:27.626433+02:00  ok
    Execution: completed  95cd16fe2a6a46eda26366ea1185e035

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:45:27.950143+02:00  ok
    Execution: completed  fd579405dd444e14975a3606ad587d63

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:30:29.944432+02:00  ok
    Execution: completed  72c573c5ae764b618ea7d10431f5a4e4

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:30:30.304744+02:00  ok
    Execution: completed  818230969491432ca2b5ce2d03749926

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:01:40.214827+02:00  ok
    Execution: completed  f71ef3c60dd04ad5bd308dd523bbc4d9

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:30:36.159511+02:00  ok
    Execution: completed  4416e17787f34330a2817f0aa8030883

  c0d0d4bc8275 [active]
    Name:      ramshield-dispatcher
    Schedule:  30 1 * * *
    Repeat:    ∞
    Next run:  2026-08-21T01:30:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T03:55:40.782944+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (provider 'opencode-go' -> 'custom'; model 'kimi-k2.7-code' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=c0d0d4bc8275 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  4f6ad81779864f7fb47af37a53e6f435

  26862e70b8a0 [active]
    Name:      ramshield-error-healer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:30:36.534243+02:00  ok
    Execution: completed  887ee78bc6a24735b0e03c3070c58385

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Last run:  2026-08-20T04:00:58.717950+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=eef10d21be44 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  c2b70a8b657d4f24aecedd626b5989c7

  77b73c6cddb4 [active]
    Name:      scalper-daily-morning
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-20T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
    Last run:  2026-08-19T06:00:23.682795+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=77b73c6cddb4 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.

  8c6313d1be80 [active]
    Name:      hourly_scalper_check
    Schedule:  */1 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T04:51:00+02:00
    Deliver:   local
    Script:    scalper.py --hourly-check
    Last run:  2026-08-20T04:50:07.129540+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=8c6313d1be80 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  ccb019e8ee544510b8feadb0ab915307

  707cf752b0de [active]
    Name:      morning_scalper_check
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-20T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
    Last run:  2026-08-19T06:00:23.799259+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=707cf752b0de provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
```
