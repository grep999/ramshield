# Cron Job Status — 2026-07-20 20:29 UTC

**Live snapshot from `hermes cron list`.** Updated every 5 minutes.

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Scheduled Jobs                                  │
└─────────────────────────────────────────────────────────────────────────┘

  74d60ec059b9 [active]
    Name:      ramshield-backup
    Schedule:  0 2 * * *
    Repeat:    ∞
    Next run:  2026-07-21T02:00:00+02:00
    Deliver:   local
    Script:    backup_project.sh
    Mode:      no-agent (script stdout delivered directly)
    Last run:  2026-07-20T05:55:06.223050+02:00  ok
    Execution: completed  e6a39fb0bdde491c85a259f19a5d836c

  18e3993ed6a0 [active]
    Name:      RamShield Promotion Agent
    Schedule:  0 9 * * *
    Repeat:    ∞
    Next run:  2026-07-21T09:00:00+02:00
    Deliver:   local
    Skills:    hermes-agent
    Workdir:   /home/m/out/ramshield_promotion
    Last run:  2026-07-20T09:17:29.465205+02:00  ok
    Execution: completed  94afd165fb2c44f38a15fa4973ebbd46

  e3652296ba99 [active]
    Name:      ramshield-helper-agent
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Last run:  2026-07-20T22:25:29.766991+02:00  error: TimeoutError: Cron job 'ramshield-helper-agent' idle for 600s (limit 600s) — last activity: waiting for non-streaming API response
    Execution: failed  6ef6f6b2110c4b2288c9b573f4f32502

  1cb5e490c826 [active]
    Name:      ramshield-facts-collector
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    facts_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T22:00:17.753962+02:00  ok
    Execution: completed  6d61b01a0217432da2cabdf12224b3ca

  cd22edb2d5f2 [active]
    Name:      ramshield-daily-planner
    Schedule:  0 1 * * *
    Repeat:    ∞
    Next run:  2026-07-21T01:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T05:56:12.073330+02:00  ok
    Execution: completed  4e8551a348d64f59aea3dc788b0c121d

  d72f32a35099 [active]
    Name:      ramshield-reviewer
    Schedule:  0 3 * * *
    Repeat:    ∞
    Next run:  2026-07-21T03:00:00+02:00
    Deliver:   local
    Skills:    autonomous-project-agents
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T05:58:41.914314+02:00  ok
    Execution: completed  210461978dd14c29b69fa83632b545f9

  53feb7ef060c [active]
    Name:      ramshield-cron-status
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    cron_status_collector.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T22:00:18.807083+02:00  ok
    Execution: claimed  8abe72c05a4a425ca7f6ffc3f7334459

  076a9de35470 [active]
    Name:      ramshield-pulse
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T22:15:29.265842+02:00  error: TimeoutError: Cron job 'ramshield-pulse' idle for 601s (limit 600s) — last activity: waiting for non-streaming API response
    Execution: claimed  9a71a63e759045fda212651bc51ab289

  f270eaf2c891 [active]
    Name:      ramshield-research-agent
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T23:00:00+02:00
    Deliver:   local
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T21:06:42.969155+02:00  ok
    Execution: running  489268954c334d8ca3d6b34072c02d1d

  3bc0c27129c2 [active]
    Name:      ramshield-health-loop
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T21:49:00.741565+02:00  ok
    Execution: claimed  6109f2b890b64b4ab25eb1c83fc19c28

  22f70c51ef6f [active]
    Name:      ramshield-health-repair
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T23:00:00+02:00
    Deliver:   local
    Script:    health_check_repair.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T21:07:14.222443+02:00  ok
    Execution: claimed  c3886267f0914f8aaa021231afac37bc

  51e8f561ed3e [active]
    Name:      ramshield-git-automation
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    git_automation.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T21:49:00.892610+02:00  ok
    Execution: claimed  8b5b7f8e0aa244519c1161878083c2e5

  cdc99e8f0b2c [active]
    Name:      promo-qw-github-topics
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    promo_batch.py quickwin-github-topics
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T21:56:57.734658+02:00  error: Script not found: /home/m/.hermes/scripts/promo_batch.py quickwin-github-topics
    Execution: claimed  a6553cc1a8b04a74989a8f97b02ae936

  4c68ff84646b [active]
    Name:      promo-qw-awesome-rust
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    promo_batch.py quickwin-awesome-rust
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T21:56:57.793148+02:00  error: Script not found: /home/m/.hermes/scripts/promo_batch.py quickwin-awesome-rust
    Execution: claimed  88de34ebc7a847949400c5257b0d57ec

  f192f20e812a [active]
    Name:      promo-qw-crates-io
    Schedule:  */5 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    promo_batch.py quickwin-crates-io
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T21:56:57.854884+02:00  error: Script not found: /home/m/.hermes/scripts/promo_batch.py quickwin-crates-io
    Execution: claimed  ee8c9f9da1d544d7a2905816b5b1b04c

  d758989bd22f [active]
    Name:      promo-fast-reddit
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    promo_batch.py fast-reddit
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T21:52:13.603456+02:00  error: Script not found: /home/m/.hermes/scripts/promo_batch.py fast-reddit
    Execution: claimed  adb6cd0ff4da4654a05a18e758bd2d82

  22cb958d90ef [active]
    Name:      promo-fast-x
    Schedule:  */10 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    promo_batch.py fast-x-twitter
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T21:52:13.648051+02:00  error: Script not found: /home/m/.hermes/scripts/promo_batch.py fast-x-twitter
    Execution: claimed  0ea10b0133b54de1bb5e19db96961ca4

  5d51ca4e9179 [active]
    Name:      promo-std-devto
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    promo_batch.py std-devto
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T21:49:02.431976+02:00  error: Script not found: /home/m/.hermes/scripts/promo_batch.py std-devto
    Execution: claimed  5fb98af921ac4b74b59ee5ead6b2fe4e

  c9aebd15e27c [active]
    Name:      promo-std-hn
    Schedule:  */15 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    promo_batch.py std-hn
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Last run:  2026-07-20T21:49:02.473832+02:00  error: Script not found: /home/m/.hermes/scripts/promo_batch.py std-hn
    Execution: claimed  da4ce44f1965432595defe66941ca46a

  5275947fb767 [active]
    Name:      promo-deep-blog
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    promo_batch.py deep-blog
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Execution: claimed  2f71b9105aad4ed489fc1cd0f957782e

  3c07c0e4bd8d [active]
    Name:      promo-deep-rust-weekly
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    promo_batch.py deep-rust-weekly
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Execution: claimed  83f665e10509437c84f79d3e8ce26e74

  370fce9c910e [active]
    Name:      promo-strategic-plan
    Schedule:  0 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T23:00:00+02:00
    Deliver:   local
    Script:    promo_batch.py strategic-plan
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Execution: claimed  2c63f09264ec43f48858b774a40848cd

  d00b405982ca [active]
    Name:      promo-reviewer
    Schedule:  */30 * * * *
    Repeat:    ∞
    Next run:  2026-07-20T22:30:00+02:00
    Deliver:   local
    Script:    promo_review.py
    Mode:      no-agent (script stdout delivered directly)
    Workdir:   /home/m/vehicle_of_rationalism/ramshield/beta/rs
    Execution: claimed  0702bbe0a5a94b80aa33c7328ba8921a
```
