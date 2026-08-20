# Cron Job Status — 2026-08-20 03:00 UTC

**Live snapshot from `hermes cron list`.** 29 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 2 |
| Error | 8 |
| Running | 2 |
| Pending | 0 |
| Scheduled | 17 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ✅ ok | completed | 2026-08-20T03:55:08.098421+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error |  | 2026-08-19T09:00:50.752657+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | ❌ error | failed | 2026-08-20T05:00:08.914486+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-20T05:00:08.239870+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ❌ error | failed | 2026-08-20T03:54:56.809960+02:00 |
| ramshield-reviewer | `0 3 * * *` | ❌ error | failed | 2026-08-20T03:54:57.574285+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-20T04:55:09.372464+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:55:09.637818+02:00 |
| ramshield-research-agent | `0 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:01:00.358806+02:00 |
| ramshield-health-loop | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:45:26.310711+02:00 |
| ramshield-health-repair | `0 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:01:37.298065+02:00 |
| ramshield-git-automation | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:45:26.539956+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:55:09.966912+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:55:10.211350+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:55:10.515476+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:50:10.279686+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:50:10.583214+02:00 |
| promo-std-devto | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:45:27.626433+02:00 |
| promo-std-hn | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:45:27.950143+02:00 |
| promo-deep-blog | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:30:29.944432+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:30:30.304744+02:00 |
| promo-strategic-plan | `0 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:01:40.214827+02:00 |
| promo-reviewer | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:30:36.159511+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error | failed | 2026-08-20T03:55:40.782944+02:00 |
| ramshield-error-healer | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-20T04:30:36.534243+02:00 |
| scalper-hourly | `0 * * * *` | 🏃 running | running | 2026-08-20T04:00:58.717950+02:00 |
| scalper-daily-morning | `0 6 * * *` | ❌ error |  | 2026-08-19T06:00:23.682795+02:00 |
| hourly_scalper_check | `*/1 * * * *` | ❌ error | failed | 2026-08-20T05:00:08.969615+02:00 |
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
    Next run:  2026-08-20T05:10:00+02:00
    Deliver:   local
    Last run:  2026-08-20T05:00:08.914486+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=e3652296ba99 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  d50df9ef28c949dab759733e5838dcbf

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:30:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T05:00:08.239870+02:00  ok
    Execution: completed  cc4585efb6ef4be48a7aba21dcd3db26

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
    Next run:  2026-08-20T05:05:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:55:09.372464+02:00  ok
    Execution: running  344846193e0746a0809a5d1d7c1106e0

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:05:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:55:09.637818+02:00  ok
    Execution: claimed  ced238a9e37442e6b32e6afebf6181f7

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T06:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:01:00.358806+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=f270eaf2c891 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: claimed  55d6cc86121042b6a9f0387aa177b910

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:15:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:45:26.310711+02:00  ok
    Execution: claimed  24c4fd23a490451bb081a7256ce945d7

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T06:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:01:37.298065+02:00  ok
    Execution: claimed  0824fbecd9764ab6a2fa595bb74d34dc

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:15:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:45:26.539956+02:00  ok
    Execution: claimed  c9b6d8e3e99444a6bd634982eea38394

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:05:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:55:09.966912+02:00  ok
    Execution: claimed  7411a9d8eab74f6da2b1943dd726a81b

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:05:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:55:10.211350+02:00  ok
    Execution: claimed  57181e65e7644e5080dfa3f21c3a3824

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:05:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:55:10.515476+02:00  ok
    Execution: claimed  5f80db14fe4745d59605c3cae2eb1d58

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:10:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:50:10.279686+02:00  ok
    Execution: claimed  cfa6019ab6ea489d813c2c3b2aeb695e

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:10:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:50:10.583214+02:00  ok
    Execution: claimed  c4dc7c27d29a437cac00c28a7ad428c2

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:15:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:45:27.626433+02:00  ok
    Execution: claimed  0df95a34d2e242a1a6538c3f8d766931

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:15:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:45:27.950143+02:00  ok
    Execution: claimed  db2c0df46f334bf685bd8879b12ffa13

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:30:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:30:29.944432+02:00  ok
    Execution: claimed  4feeba0590854b96be5b6bab2ecda22a

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:30:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:30:30.304744+02:00  ok
    Execution: claimed  9c9d18197bad4390be6cb9a5f3951c2d

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T06:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:01:40.214827+02:00  ok
    Execution: claimed  3b7bb1bf0a844bcfa5cd01c12f74f826

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T05:30:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:30:36.159511+02:00  ok
    Execution: claimed  a6067491333549b7bc44c46c8ee6b369

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
    Next run:  2026-08-20T05:30:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-20T04:30:36.534243+02:00  ok
    Execution: claimed  a9e3b04422fa486cbf4f19779c46840a

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-20T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Last run:  2026-08-20T05:00:09.430546+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=eef10d21be44 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  5fb575f425cc4163a4368345d4210bd9

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
    Next run:  2026-08-20T05:01:00+02:00
    Deliver:   local
    Script:    scalper.py --hourly-check
    Last run:  2026-08-20T05:00:08.969615+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=8c6313d1be80 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  125f1543b8274fff94d958e87ec8b789

  707cf752b0de [active]
    Name:      morning_scalper_check
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-20T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
    Last run:  2026-08-19T06:00:23.799259+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=707cf752b0de provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
```
