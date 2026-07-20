# RamShield Promotion Agency — Scope & Playbook

## Directory Layout (`docs/promotion/`)
- `campaigns/`  — active campaign briefs, one .md per campaign
- `content/`    — drafted posts, threads, copy blocks
- `social/`     — platform-specific ready-to-post assets (X, LinkedIn, Reddit, HN)
- `lists/`      — target channel lists (subreddits, forums, newsletters, directories)
- `articles/`   — long-form blog posts, tutorials, case studies
- `metrics/`    — outcome logs per campaign (JSONL), aggregated to dashboard

## Operating Model — Act Like an Agency
Run 10 independent promotion jobs on tiered cadences by complexity:

| Tier | Cadence | Complexity | Jobs |
|------|---------|-----------|------|
| Quick win  | */5m  | trivial (1 action)      | 3 |
| Fast        | */10m | low (short copy)        | 2 |
| Standard    | */15m | medium (thread/post)    | 2 |
| Deep        | */30m | high (article section)  | 2 |
| Strategic   | 0 * (hourly) | campaign planning  | 1 |

## Channel Universe (breadth-first outreach)
- Social: X/Twitter, LinkedIn, Mastodon, Bluesky
- Communities: Reddit (r/rust, r/netsec, r/selfhosted, r/devops), HackerNews, Lobste.rs
- Dev directories: awesome-rust, awesome-security, libhunt, crates.io keywords
- Content: dev.to, Hashnode, Medium, personal blog
- Newsletters: Rust Weekly, TLDR sec, Console.dev
- Docs/SEO: README badges, GitHub topics, keyword pages

## Timeline Phases
1. **Week 1 — Foundation**: README/badges/topics, awesome-list PRs, crates.io metadata
2. **Week 2 — Community seeding**: Reddit/HN launch posts, Show HN, dev.to intro
3. **Week 3 — Content engine**: weekly blog, comparison articles, benchmarks
4. **Week 4+ — Amplification**: newsletters, influencer outreach, champions program

## Campaign Structure (each brief in `campaigns/`)
```
# Campaign: <name>
Goal: <metric>
Channels: <list>
Cadence: <tier>
Quick wins: [...]
Long-term: [...]
Status: active|paused|done
```

## Quick Wins (do first, high ROI/low effort)
- Add GitHub topics + description keywords
- README shields (build, crates, license, stars)
- Submit to awesome-rust / awesome-security
- Post to r/rust "what are you working on" thread
- crates.io keywords + categories

## Long-Term Improvements
- Weekly technical blog cadence
- Benchmark vs competitors (data-driven posts)
- Video demos / asciinema
- Conference CFP submissions
- Champions/ambassador program

## Priority
Promotion is now a **P0 priority track**. Outcomes surface on the helper dashboard
(`docs/promotion/metrics/*.jsonl` → aggregated → PROMOTION_LOG.md → dashboard panel).
