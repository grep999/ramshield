# Cron Job Status — 2026-08-29 08:20 UTC

**Live snapshot from `hermes cron list`.** 25 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 5 |
| Error | 1 |
| Running | 2 |
| Pending | 0 |
| Scheduled | 17 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error | failed | 2026-08-29T10:16:23.381073+02:00 |
| ramshield-helper-agent | `*/10 * * * *` | 🏃 running | running | 2026-08-29T10:11:38.951733+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-08-29T10:16:25.636730+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ✅ ok | completed | 2026-08-29T10:17:39.405682+02:00 |
| ramshield-reviewer | `0 3 * * *` | ✅ ok | completed | 2026-08-29T10:20:46.115472+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-08-23T22:10:36.430814+02:00 |
| ramshield-pulse | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:10:36.750850+02:00 |
| ramshield-health-loop | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:00:56.185479+02:00 |
| ramshield-health-repair | `0 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:01:14.607064+02:00 |
| ramshield-git-automation | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:01:14.919919+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:10:37.074105+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:10:37.362246+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:10:37.636212+02:00 |
| promo-fast-reddit | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:10:37.928876+02:00 |
| promo-fast-x | `*/10 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:10:38.247178+02:00 |
| promo-std-devto | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:01:16.846910+02:00 |
| promo-std-hn | `*/15 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:01:17.172731+02:00 |
| promo-deep-blog | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:01:17.455000+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:01:17.713872+02:00 |
| promo-strategic-plan | `0 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:01:18.057969+02:00 |
| promo-reviewer | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:01:19.975153+02:00 |
| ramshield-dispatcher | `30 1 * * *` | 📅 scheduled | claimed | 2026-08-23T11:21:10.559694+02:00 |
| ramshield-error-healer | `*/30 * * * *` | 📅 scheduled | claimed | 2026-08-23T22:01:20.323828+02:00 |
| scalper-hourly | `0 * * * *` | ✅ ok | completed | 2026-08-29T10:08:42.606584+02:00 |
| scalper-daily-morning | `0 6 * * *` | ✅ ok | completed | 2026-08-29T10:08:42.631118+02:00 |

## Raw Output

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Scheduled Jobs                                  │
└─────────────────────────────────────────────────────────────────────────┘

  18e3993ed6a0 [active]
    Name:      RamShield Promotion Agent
    Schedule:  0 9 * * *
    Repeat:    ∞
    Next run:  2026-08-30T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion
    Last run:  2026-08-29T10:16:23.381073+02:00  error: RuntimeError: HTTP 401: [openrouter/nvidia/nemotron-3-ultra-550b-a55b:free] [404]: {"error":{"message":"Provider returned error","code":404,"metadata":{"raw":"","provider_name":"Nvidia","is_byok":false}},"user_id":"user_3IGHD8oZegZCxCNYK4ghOoIvpqu"} (reset after 2m)
    Execution: failed  a6b909604ad54ec6a5df98197842e6d2

  e3652296ba99 [active]
    Name:      ramshield-helper-agent
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:30:00+02:00
    Deliver:   local
    Last run:  2026-08-29T10:11:38.951733+02:00  ok
    Execution: running  853637ac3b9f4c7ebe201cb3b38dfa83

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:30:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T10:16:25.636730+02:00  ok
    Execution: completed  941c752919dd4e028651153463842fff

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-08-30T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T10:17:39.405682+02:00  ok
    Execution: completed  63bcbbb51e9c45e7b4a4ecbd3e348690

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-08-30T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-29T10:20:46.115472+02:00  ok
    Execution: completed  8a1f3e0ba445418f8576da3ff3c9c4bd

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:25:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:10:36.430814+02:00  ok
    Execution: running  101046ec8c414b5db301de3fab7ee0c0

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:25:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:10:36.750850+02:00  ok
    Execution: claimed  baae4cf15dbd45ed9553ebb8e093e515

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:30:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:00:56.185479+02:00  ok
    Execution: claimed  9dc022589d324c039883ee293a58b642

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:01:14.607064+02:00  ok
    Execution: claimed  1515720a00bd47b49f2d10b67b51a4de

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:30:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:01:14.919919+02:00  ok
    Execution: claimed  5e95407544fa42cd954c1b678a5240fe

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:25:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:10:37.074105+02:00  ok
    Execution: claimed  808d9928f707446b9d6b82e35767b3c1

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:25:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:10:37.362246+02:00  ok
    Execution: claimed  512a1ee473be4febb4b75f0ed23c90ff

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:25:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:10:37.636212+02:00  ok
    Execution: claimed  d8e25f0e80bb4f20a89ccbd2a13b298c

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:30:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:10:37.928876+02:00  ok
    Execution: claimed  77169af30b874e0fa7f51c167d0d414c

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:30:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:10:38.247178+02:00  ok
    Execution: claimed  cb02f46942f44b00a38deb0b48219537

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:30:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:01:16.846910+02:00  ok
    Execution: claimed  6ef145f7d9dc4e9ba2a4c43de6c8aeb9

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:30:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:01:17.172731+02:00  ok
    Execution: claimed  70898ee867184022929339d73f3fd833

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:30:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:01:17.455000+02:00  ok
    Execution: claimed  4626e112bdbd4936a0c5c0f05afd8856

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:30:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:01:17.713872+02:00  ok
    Execution: claimed  556199b9b778499ab6f0111c7649812a

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:01:18.057969+02:00  ok
    Execution: claimed  ed7b1f04576143de88883e94215dcdb2

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:30:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:01:19.975153+02:00  ok
    Execution: claimed  c8c4cfef29024eb6a08b72bdf224405c

  c0d0d4bc8275 [active]
    Name:      ramshield-dispatcher
    Schedule:  30 1 * * *
    Repeat:    ∞
    Next run:  2026-08-30T01:30:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T11:21:10.559694+02:00  ok
    Execution: claimed  9ac54019d858492b8a32260955c2f378

  26862e70b8a0 [active]
    Name:      ramshield-error-healer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T10:30:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-08-23T22:01:20.323828+02:00  ok
    Execution: claimed  26bc5ae14d8b4f4f83b986336a4640bb

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-08-29T11:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-29T10:08:42.606584+02:00  ok
    Execution: completed  61570748fc5d4268b7d7c071da9cc0a4

  77b73c6cddb4 [active]
    Name:      scalper-daily-morning
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-08-30T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-08-29T10:08:42.631118+02:00  ok
    Execution: completed  e1bc7501503146cb9455d2741d7ccabc
```
