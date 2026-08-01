# Cron Job Status — 2026-08-01 07:47 UTC

**Live snapshot from `hermes cron list`.** 29 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 6 |
| Error | 1 |
| Running | 4 |
| Pending | 0 |
| Scheduled | 17 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| ramshield-backup | `0 2 * * *` | ✅ ok | completed | 2026-08-01T06:25:49.382082+02:00 |
| RamShield Promotion Agent | `0 9 * * *` | ✅ ok | completed | 2026-08-01T09:47:49.210109+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | 🏃 running | running | 2026-08-01T08:50:09.113347+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-01T09:47:49.561975+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ✅ ok | completed | 2026-08-01T06:26:44.855524+02:00 |
| ramshield-reviewer | `0 3 * * *` | ✅ ok | completed | 2026-08-01T06:32:38.450509+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-01T08:55:06.622110+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:55:06.809542+02:00 |
| ramshield-research-agent | `0 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:01:29.149456+02:00 |
| ramshield-health-loop | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:45:25.295886+02:00 |
| ramshield-health-repair | `0 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:02:02.146889+02:00 |
| ramshield-git-automation | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:45:25.448159+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:55:06.949372+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:55:07.124618+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:55:07.317839+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:50:07.826557+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:50:08.048304+02:00 |
| promo-std-devto | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:45:26.052400+02:00 |
| promo-std-hn | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:45:26.231154+02:00 |
| promo-deep-blog | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:30:20.819588+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:30:20.960684+02:00 |
| promo-strategic-plan | `0 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:02:04.102638+02:00 |
| promo-reviewer | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:30:21.336905+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error | failed | 2026-08-01T06:37:29.253355+02:00 |
| ramshield-error-healer | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-01T08:30:21.507640+02:00 |
| scalper-hourly | `0 * * * *` | 🏃 running | running | 2026-08-01T08:01:47.080819+02:00 |
| scalper-daily-morning | `0 6 * * *` | ✅ ok | completed | 2026-08-01T06:27:10.108879+02:00 |
| hourly_scalper_check | `*/1 * * * *` | 🏃 running | running | 2026-08-01T09:04:43.965384+02:00 |
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
    Next run:  2026-08-01T09:50:00+02:00
    Deliver:   local
    Last run:  2026-08-01T08:50:09.113347+02:00  error: TimeoutError: Cron job 'ramshield-helper-agent' idle for 600s (limit 600s) — last activity: waiting for non-streaming API response
    Execution: running  72d5debeab2a47a696a892fac034965c

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T10:00:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T09:47:49.561975+02:00  ok
    Execution: completed  6373d4ee442842e3ac8ee82fbf7ddfba

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
    Next run:  2026-08-01T09:50:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:55:06.622110+02:00  ok
    Execution: running  3a1c8eaaa0bc43b6bb101cdb55649b48

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T09:50:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:55:06.809542+02:00  ok
    Execution: claimed  045ebe67b6444ac9aee5f6062beca5c4

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T10:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:01:29.149456+02:00  error: RuntimeError: HTTP 404: No active credentials for provider: cloudflare
    Execution: claimed  b02211a3aa20458bb787ee52b5d4e990

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T10:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:45:25.295886+02:00  ok
    Execution: claimed  0a0eb1dbd39f429d95ec196417d79c18

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T10:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:02:02.146889+02:00  ok
    Execution: claimed  fd48acc9b02b4a06af3c935e4d81fb3c

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T10:00:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:45:25.448159+02:00  ok
    Execution: claimed  eced5cdd78864caa8a97ab435b71bf55

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T09:50:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:55:06.949372+02:00  ok
    Execution: claimed  e7028c8896d847ca8fe852124360b0b4

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T09:50:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:55:07.124618+02:00  ok
    Execution: claimed  52b07f8f7e0647e384029afd740a2f52

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T09:50:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:55:07.317839+02:00  ok
    Execution: claimed  4799b518643b48759f3fd4c80c278da1

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T09:50:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:50:07.826557+02:00  ok
    Execution: claimed  c5787d8b730843a39676727a0c9a6a41

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T09:50:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:50:08.048304+02:00  ok
    Execution: claimed  97c065fac59a4b0284fa74a7861a7d71

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T10:00:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:45:26.052400+02:00  ok
    Execution: claimed  bcc88e38597647b2aeb4d11535678084

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T10:00:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:45:26.231154+02:00  ok
    Execution: claimed  6d3c029cb7f6404198e3e4a7a3008e86

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T10:00:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:30:20.819588+02:00  ok
    Execution: claimed  f84a1036303e4bee8ad156a8ec52fbf5

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T10:00:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:30:20.960684+02:00  ok
    Execution: claimed  b6fd9df767dc4983a57c47503b23ae40

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T10:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:02:04.102638+02:00  ok
    Execution: claimed  5d20afc3952b4007ad644ae5797cc57d

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T10:00:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:30:21.336905+02:00  ok
    Execution: claimed  8da9259991bf46528b526c830c480629

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
    Next run:  2026-08-01T10:00:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-01T08:30:21.507640+02:00  ok
    Execution: claimed  4fbf383bb38a4fe582b195917c1a9343

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-01T10:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Last run:  2026-08-01T08:01:47.080819+02:00  error: RuntimeError: HTTP 404: No active credentials for provider: cloudflare
    Execution: running  96d8d5c9fa5b4c4db1d5e5960c584e17

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
    Next run:  2026-08-01T09:48:00+02:00
    Deliver:   local
    Script:    scalper.py --hourly-check
    Last run:  2026-08-01T09:04:43.965384+02:00  ok
    Execution: running  f746d3d3f8d444be810941828ad524ec

  707cf752b0de [active]
    Name:      morning_scalper_check
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-02T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py --morning-check
```
