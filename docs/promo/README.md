# RamShield promo video (90 s, 1280×720 @ 30 fps)

## File
`ramshield-promo.mp4` — 2.1 MB, 57.6 s

## Source
Manim CE 0.21 project at `beta/promo-video/`. Re-render with:

```bash
cd beta/promo-video
manim -qm script.py Scene1_Hook Scene2_Reveal Scene3_Detect \
                   Scene4_Decide Scene5_Enforce Scene6_Survive \
                   Scene7_Prove Scene8_CTA
ffmpeg -y -f concat -safe 0 -i concat_hq.txt -c copy final.mp4
```

Draft (`-ql`) finishes in ~3 min. HQ (`-qm`) ~5 min. 1080p60 (`-qh`) ~25 min.

## Scenes
| # | Title       | Beat                                             |
|---|-------------|--------------------------------------------------|
| 1 | Hook        | Edge proxy blind, origin dies 503                |
| 2 | Reveal      | Wordmark + "single binary / 50 ms / XDP drop"    |
| 3 | Detect      | 6 signals fan out, /24 cluster lights up         |
| 4 | Decide      | Batch window slides, composite score 0→0.95      |
| 5 | Enforce     | Userspace IPC path, then XDP kernel ✗ drop       |
| 6 | Survive     | WAL → CRASH → boot replay → 1247 blocks restored |
| 7 | Prove       | 0.2 ms · 4% CPU @ 100k req/s                     |
| 8 | CTA         | `cargo install ramshield` · GitHub               |

## Embed
```markdown
[![RamShield promo](ramshield-promo.mp4)](ramshield-promo.mp4)
```
