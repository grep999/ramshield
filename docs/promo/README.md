# RamShield promo video

Two versions, same visuals, same scene timings.

| File | Audio | Duration | Size | Use case |
|------|-------|----------|------|----------|
| `ramshield-promo.mp4`         | yes (VO + drone) | 60.4 s | 2.2 MB | Primary — GitHub, YouTube, blog embed |
| `ramshield-promo-silent.mp4`  | no               | 57.6 s | 2.1 MB | Muted autoplay on feeds (X, LinkedIn, Bluesky) |

Both: 1280×720 @ 30 fps, H.264, `+faststart` for streaming.

## Source
`beta/promo-video/`
- `script.py` — 8 Manim scene classes
- `build_audio.sh` — pipeline: drone + VO + mux → `final_with_audio.mp4`
- `vo/` — TTS clips per scene (regenerate via Hermes `text_to_speech`)
- `concat_hq.txt` — scene concat list
- `plan.md` — scene breakdown

## Rebuild
```bash
cd beta/promo-video

# 1) render video
manim -qm script.py Scene1_Hook Scene2_Reveal Scene3_Detect \
                   Scene4_Decide Scene5_Enforce Scene6_Survive \
                   Scene7_Prove Scene8_CTA
ffmpeg -y -f concat -safe 0 -i concat_hq.txt -c copy final.mp4

# 2) silent version (just the video)
cp final.mp4 ../rs/docs/promo/ramshield-promo-silent.mp4

# 3) audio version (needs VO clips in vo/)
bash build_audio.sh
cp final_with_audio.mp4 ../rs/docs/promo/ramshield-promo.mp4
```

## Scenes
| # | Title    | Offset | Dur | Beat                                       |
|---|----------|--------|-----|--------------------------------------------|
| 1 | Hook     | 0.0 s  | 8 s | Flood → edge proxy → 503 origin death      |
| 2 | Reveal   | 8.0 s  | 6 s | Wordmark + "one binary / 50 ms / XDP"      |
| 3 | Detect   | 14.0 s | 9 s | 6 signals fan out, /24 cluster lights up   |
| 4 | Decide   | 22.9 s | 9 s | Batch window, composite score 0 → 0.95    |
| 5 | Enforce  | 31.4 s | 8 s | IPC userspace + XDP kernel ✗ drop          |
| 6 | Survive  | 39.4 s | 8 s | WAL → CRASH → boot replay, 1247 restored   |
| 7 | Prove    | 47.4 s | 7 s | 0.2 ms · 4% CPU @ 100k req/s               |
| 8 | CTA      | 54.4 s | 6 s | `cargo install ramshield` · GitHub         |

## Embed (with audio)
```markdown
[![RamShield promo](ramshield-promo.mp4)](ramshield-promo.mp4)
```

## Embed (silent — for muted autoplay on social feeds)
```markdown
[![RamShield promo](ramshield-promo-silent.mp4)](ramshield-promo-silent.mp4)
```
