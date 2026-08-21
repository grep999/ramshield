# Cron Job Status — 2026-08-21 16:45 UTC

**Live snapshot from `hermes cron list`.** 29 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 5 |
| Error | 15 |
| Running | 1 |
| Pending | 0 |
| Scheduled | 8 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ✅ ok |  | 2026-08-21T08:39:59.736223+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error |  | 2026-08-21T09:00:26.987698+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | ❌ error | failed | 2026-08-21T18:40:01.517488+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-21T18:30:59.574146+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ❌ error |  | 2026-08-21T08:39:21.598831+02:00 |
| ramshield-reviewer | `0 3 * * *` | ❌ error |  | 2026-08-21T08:39:22.401426+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-21T18:40:03.265742+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-21T18:40:03.521713+02:00 |
| ramshield-research-agent | `0 * * * *` | ❌ error | failed | 2026-08-21T18:00:58.536726+02:00 |
| ramshield-health-loop | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-21T18:31:23.418140+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-08-21T18:01:52.460791+02:00 |
| ramshield-git-automation | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-21T18:31:23.682394+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-21T18:40:03.797969+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-21T18:40:04.071655+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-21T18:40:04.348926+02:00 |
| promo-fast-reddit | `*/10 * * * *` | ❌ error | failed | 2026-08-21T18:40:04.598016+02:00 |
| promo-fast-x | `*/10 * * * *` | ❌ error | failed | 2026-08-21T18:40:04.838546+02:00 |
| promo-std-devto | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-21T18:31:25.278950+02:00 |
| promo-std-hn | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-21T18:31:25.527769+02:00 |
| promo-deep-blog | `*/30 * * * *` | ❌ error | failed | 2026-08-21T18:31:25.810938+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ❌ error | failed | 2026-08-21T18:31:26.112245+02:00 |
| promo-strategic-plan | `0 * * * *` | ❌ error | failed | 2026-08-21T18:01:55.573687+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-08-21T18:31:27.940610+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error |  | 2026-08-21T08:40:13.560974+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-08-21T18:31:28.193015+02:00 |
| scalper-hourly | `0 * * * *` | ❌ error | failed | 2026-08-21T18:00:56.277397+02:00 |
| scalper-daily-morning | `0 6 * * *` | ❌ error |  | 2026-08-21T08:39:23.250029+02:00 |
| hourly_scalper_check | `*/1 * * * *` | ❌ error | failed | 2026-08-21T18:45:02.342919+02:00 |
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
    Next run:  2026-08-21T18:50:00+02:00
    Deliver:   local
    Last run:  2026-08-21T18:40:01.517488+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=e3652296ba99 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  599b047be9934ef39953962d2e0e0f2c

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T19:00:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:30:59.574146+02:00  ok
    Execution: completed  3a24a3204d1b40bca6214795874d01b6

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
    Next run:  2026-08-21T18:50:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:40:03.265742+02:00  ok
    Execution: running  1925519782d940fdb98cccdcc6ad6fb3

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T18:50:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:40:03.521713+02:00  ok
    Execution: claimed  7082a668c140472ab9d1a17e851c3885

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T19:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:00:58.536726+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=f270eaf2c891 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  95fa540bc1d54f22ab361da893d3cde2

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T19:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:31:23.418140+02:00  ok
    Execution: claimed  c0faf48c159b4ce488038451e8d5da80

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T19:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:01:52.460791+02:00  ok
    Execution: completed  982e8336371245ec82477f06c2c370e0

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T19:00:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:31:23.682394+02:00  ok
    Execution: claimed  aa32462e1cca4ff086e5d70b5c7372cd

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T18:50:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:40:03.797969+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  99cf4ef1a21e49fe9568c2de226ef123

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T18:50:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:40:04.071655+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  5103644b296c41ad90efa185abfe010c

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T18:50:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:40:04.348926+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  d342aef7ba664fc7bd71cfc77ab14f0e

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T18:50:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:40:04.598016+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  764d1716346445d4bc95058ed5d18822

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T18:50:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:40:04.838546+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  5037872294344400b7cfba285fb7818a

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T19:00:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:31:25.278950+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  37ec5891f529419c963ca12bdd6b3e4e

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T19:00:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:31:25.527769+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: claimed  b20802aec40c42cebeb0f117f845e988

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T19:00:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:31:25.810938+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  63c5965efac34709aed7eca46b987d70

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T19:00:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:31:26.112245+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  76b21818e182416ebd1d637a94ff63b7

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T19:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:01:55.573687+02:00  error: Script exited with code 2
stderr:
python3: can't open file '/home/m/vehicle_of_rationalism/ramshield/beta/rs/promo_batch.py': [Errno 2] No such file or directory
    Execution: failed  9a46fbe843ac474aa3d35268205832cb

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T19:00:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:31:27.940610+02:00  ok
    Execution: completed  0be70825bf204034a446d8daa3c67a1a

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
    Next run:  2026-08-21T19:00:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-21T18:31:28.193015+02:00  ok
    Execution: completed  f40703af2186482e844a2775dc1af6a3

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-21T19:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Last run:  2026-08-21T18:00:56.277397+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=eef10d21be44 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  9cffb69e28be47b08567ad2759f8ab2a

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
    Next run:  2026-08-21T18:46:00+02:00
    Deliver:   local
    Script:    scalper.py --hourly-check
    Last run:  2026-08-21T18:45:02.342919+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=8c6313d1be80 provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
    Execution: failed  1e66c2368874421e8962899e059c0d76

  707cf752b0de [active]
    Name:      morning_scalper_check
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-22T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
    Last run:  2026-08-21T08:39:23.466865+02:00  error: RuntimeError: [drift_skip:silent] Skipped to prevent unintended spend: global inference config drifted since this job was created (model 'ram' -> 'zombobobo'), and this job is unpinned. No inference call was made. To run on the new config, pin it explicitly: `cronjob action=update job_id=707cf752b0de provider=<provider> model=<model>` (or pin the original values to keep them). This alert is sent once; the job stays skipped until the config is pinned or restored. See #44585.
```
