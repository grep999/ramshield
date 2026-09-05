# Cron Job Status — 2026-09-05 10:05 UTC

**Live snapshot from `hermes cron list`.** 24 jobs tracked. Updated every 5 minutes.

| State | Count |
| :--- | :--- |
| OK | 19 |
| Error | 4 |
| Running | 1 |
| Pending | 0 |
| Scheduled | 0 |

| Job | Schedule | Status | Execution | Last Run |
| :--- | :--- | :--- | :--- | :--- |
| RamShield Promotion Agent | `0 9 * * *` | ❌ error | failed | 2026-09-05T09:09:04.295316+02:00 |
| ramshield-facts-collector | `*/30 * * * *` | ✅ ok | completed | 2026-09-05T12:00:05.286021+02:00 |
| ramshield-daily-planner | `0 1 * * *` | ❌ error | failed | 2026-09-05T09:09:08.983560+02:00 |
| ramshield-reviewer | `0 3 * * *` | ❌ error | failed | 2026-09-05T09:09:07.073165+02:00 |
| ramshield-cron-status | `*/5 * * * *` | 🏃 running | running | 2026-09-05T12:00:07.988111+02:00 |
| ramshield-pulse | `*/5 * * * *` | ✅ ok | completed | 2026-09-05T12:05:03.600592+02:00 |
| ramshield-health-loop | `*/15 * * * *` | ✅ ok | completed | 2026-09-05T12:00:38.063060+02:00 |
| ramshield-health-repair | `0 * * * *` | ✅ ok | completed | 2026-09-05T12:00:38.183149+02:00 |
| promo-qw-github-topics | `*/5 * * * *` | ✅ ok | completed | 2026-09-05T12:05:03.903063+02:00 |
| promo-qw-awesome-rust | `*/5 * * * *` | ✅ ok | completed | 2026-09-05T12:05:04.056466+02:00 |
| promo-qw-crates-io | `*/5 * * * *` | ✅ ok | completed | 2026-09-05T12:05:03.768847+02:00 |
| promo-fast-reddit | `*/10 * * * *` | ✅ ok | completed | 2026-09-05T12:00:04.898519+02:00 |
| promo-fast-x | `*/10 * * * *` | ✅ ok | completed | 2026-09-05T12:00:06.374556+02:00 |
| promo-std-devto | `*/15 * * * *` | ✅ ok | completed | 2026-09-05T12:00:07.065396+02:00 |
| promo-std-hn | `*/15 * * * *` | ✅ ok | completed | 2026-09-05T12:00:06.718107+02:00 |
| promo-deep-blog | `*/30 * * * *` | ✅ ok | completed | 2026-09-05T12:00:07.673336+02:00 |
| promo-deep-rust-weekly | `*/30 * * * *` | ✅ ok | completed | 2026-09-05T12:00:07.232621+02:00 |
| promo-strategic-plan | `0 * * * *` | ✅ ok | completed | 2026-09-05T12:00:08.975572+02:00 |
| promo-reviewer | `*/30 * * * *` | ✅ ok | completed | 2026-09-05T12:00:17.525742+02:00 |
| ramshield-dispatcher | `30 1 * * *` | ❌ error | failed | 2026-09-05T09:08:27.670500+02:00 |
| ramshield-error-healer | `*/30 * * * *` | ✅ ok | completed | 2026-09-05T12:00:09.160213+02:00 |
| scalper-hourly | `0 * * * *` | ✅ ok | completed | 2026-09-05T12:00:09.754308+02:00 |
| scalper-daily-morning | `0 6 * * *` | ✅ ok | completed | 2026-09-05T09:06:55.522755+02:00 |
| ramshield-backup | `0 2 * * *` | ✅ ok | completed | 2026-09-05T09:06:55.830351+02:00 |

## Raw Output

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Scheduled Jobs                                  │
└─────────────────────────────────────────────────────────────────────────┘

  18e3993ed6a0 [active]
    Name:      RamShield Promotion Agent
    Schedule:  0 9 * * *
    Repeat:    ∞
    Next run:  2026-09-06T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion
    Last run:  2026-09-05T09:09:04.295316+02:00  error: RuntimeError: HTTP 502: [openrouter/openrouter/free] [502]: fetch failed (cause: EAI_AGAIN: getaddrinfo EAI_AGAIN openrouter.ai) (reset after 8s)  (2 failures in a row)
    Dispatch:  ⚠ late: scheduled 2026-09-05T09:00:00+02:00, ran 2026-09-05T09:06:30.695134+02:00 (6m late)
    Execution: failed  e0f3859f0fae4e89a8062427b232eaa8

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:30:00+02:00
    Deliver:   local
    Script:    /home/m/.hermes/scripts/facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:00:05.286021+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:00:00+02:00)
    Execution: completed  316e896338cf4b519b547ba0f22096fa

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-09-06T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T09:09:08.983560+02:00  error: RuntimeError: HTTP 503: [openrouter/minimax/minimax-m3:free] [502]: fetch failed (cause: EAI_AGAIN: getaddrinfo EAI_AGAIN openrouter.ai) (reset after 3s)  (2 failures in a row)
    Dispatch:  ⚠ catch-up after missed fire: scheduled 2026-09-05T01:00:00+02:00, ran 2026-09-05T09:06:30.695134+02:00 (8h 6m late)
    Execution: failed  6c2305e186794079b7eb11b2970af824

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-09-06T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T09:09:07.073165+02:00  error: RuntimeError: HTTP 503: [openrouter/minimax/minimax-m3:free] [502]: fetch failed (cause: EAI_AGAIN: getaddrinfo EAI_AGAIN openrouter.ai) (reset after 5s)  (2 failures in a row)
    Dispatch:  ⚠ catch-up after missed fire: scheduled 2026-09-05T03:00:00+02:00, ran 2026-09-05T09:06:30.695134+02:00 (6h 6m late)
    Execution: failed  c855bf5a5d7b46f1be5e9c783e770fa2

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:10:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:00:07.988111+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:05:00+02:00)
    Execution: running  7b54056a12164755ab20da8db017d5de

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:10:00+02:00
    Deliver:   local
    Script:    pulse_agent.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:05:03.600592+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:05:00+02:00)
    Execution: completed  4e8d31b4294e40b2899fc5a547b530f8

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:15:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:00:38.063060+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:00:00+02:00)
    Execution: completed  05a7d282f05e46b7bcfb25e8f7e7d881

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T13:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:00:38.183149+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:00:00+02:00)
    Execution: completed  0fd68c0655d14c18911f63d1e73332c8

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:10:00+02:00
    Deliver:   local
    Script:    promo_qw_github_topics.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:05:03.903063+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:05:00+02:00)
    Execution: completed  cd3baf726ba44a07a71edfe25d513466

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:10:00+02:00
    Deliver:   local
    Script:    promo_qw_awesome_rust.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:05:04.056466+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:05:00+02:00)
    Execution: completed  8f5438fb629c4b2a99d625c635f2a33a

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:10:00+02:00
    Deliver:   local
    Script:    promo_qw_crates_io.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:05:03.768847+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:05:00+02:00)
    Execution: completed  71ff6f5b46b44a68ab8674b360bfaee6

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:10:00+02:00
    Deliver:   local
    Script:    promo_fast_reddit.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:00:04.898519+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:00:00+02:00)
    Execution: completed  b1171879a5a74ae4b1dc2a8d5ee2a530

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:10:00+02:00
    Deliver:   local
    Script:    promo_fast_x.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:00:06.374556+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:00:00+02:00)
    Execution: completed  c80e501063e94f1c878a79e212e12cc9

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:15:00+02:00
    Deliver:   local
    Script:    promo_std_devto.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:00:07.065396+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:00:00+02:00)
    Execution: completed  c8c084eee3644ec0aec85a8d1f13cc0f

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:15:00+02:00
    Deliver:   local
    Script:    promo_std_hn.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:00:06.718107+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:00:00+02:00)
    Execution: completed  1631469829d3493c9bb13e3ba440e27f

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:30:00+02:00
    Deliver:   local
    Script:    promo_deep_blog.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:00:07.673336+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:00:00+02:00)
    Execution: completed  b9eef5464bc74bba9a85bbfeca1175fd

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:30:00+02:00
    Deliver:   local
    Script:    promo_deep_rust_weekly.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:00:07.232621+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:00:00+02:00)
    Execution: completed  6cf656e9b16f49ed92206885cab991b3

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T13:00:00+02:00
    Deliver:   local
    Script:    promo_strategic_plan.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:00:08.975572+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:00:00+02:00)
    Execution: completed  1ecc88beb7764a0fb4dfff4901c95e30

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:30:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:00:17.525742+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:00:00+02:00)
    Execution: completed  bdc82be51d48475d9f9de5051e101463

  c0d0d4bc8275 [active]
    Name:      ramshield-dispatcher
    Schedule:  30 1 * * *
    Repeat:    ∞
    Next run:  2026-09-06T01:30:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T09:08:27.670500+02:00  error: RuntimeError: HTTP 503: [openrouter/minimax/minimax-m3:free] [502]: fetch failed (cause: EAI_AGAIN: getaddrinfo EAI_AGAIN openrouter.ai) (reset after 1s)  (2 failures in a row)
    Dispatch:  ⚠ catch-up after missed fire: scheduled 2026-09-05T01:30:00+02:00, ran 2026-09-05T09:06:30.695134+02:00 (7h 36m late)
    Execution: failed  360e70348a934cec8e5a69a4b1e47512

  26862e70b8a0 [active]
    Name:      ramshield-error-healer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T12:30:00+02:00
    Deliver:   local
    Script:    ramshield_error_healer.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T12:00:09.160213+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:00:00+02:00)
    Execution: completed  931ee03d70c4494389a08bbc58f6289a

  eef10d21be44 [active]
    Name:      scalper-hourly
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-09-05T13:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-09-05T12:00:09.754308+02:00  ok
    Dispatch:  on time (scheduled 2026-09-05T12:00:00+02:00)
    Execution: completed  89a6244c547347508c95552ebb5db40c

  77b73c6cddb4 [active]
    Name:      scalper-daily-morning
    Schedule:  0 6 * * *
    Repeat:    ∞
    Next run:  2026-09-06T06:00:00+02:00
    Deliver:   local
    Script:    scalper.py
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-09-05T09:06:55.522755+02:00  ok
    Dispatch:  ⚠ catch-up after missed fire: scheduled 2026-09-05T06:00:00+02:00, ran 2026-09-05T09:06:30.695134+02:00 (3h 6m late)
    Execution: completed  10f76c61460047c28a0e1c6f2952c0db

  b4a3b9b01db6 [active]
    Name:      ramshield-backup
    Schedule:  0 2 * * *
    Repeat:    ∞
    Next run:  2026-09-06T02:00:00+02:00
    Deliver:   local
    Script:    backup_project.sh
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-09-05T09:06:55.830351+02:00  ok
    Dispatch:  ⚠ catch-up after missed fire: scheduled 2026-09-05T02:00:00+02:00, ran 2026-09-05T09:06:30.695134+02:00 (7h 6m late)
    Execution: completed  1eefbc519d194c348855b5872c0f3bf3
```
