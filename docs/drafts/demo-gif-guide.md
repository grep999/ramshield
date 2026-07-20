# RamShield Dashboard Demo GIF Recording Guide

## Goal
Record a 30-second GIF demonstrating the RamShield dashboard for README, blog posts, and social media.

## Recording Specs
- **Duration**: 30 seconds max
- **Resolution**: 1280x720 (16:9) or 1920x1080
- **Frame rate**: 15-20 fps (GIF optimization)
- **Format**: `.gif` (max 10MB for GitHub, 5MB for Twitter)
- **Tools**: `peek` (Linux), `kap` (macOS), `screenToGif` (Windows), or `ffmpeg`

## Recording Script (30 seconds)

### 0-3s: Terminal Startup
```bash
cargo run --release -- --config config.toml
```
Show clean startup, version banner, config loaded.

### 3-8s: Dashboard Overview
- Pan to show: traffic graph, active rules, blocked IPs counter
- Highlight: real-time throughput counter updating

### 8-15s: Attack Simulation
```bash
# Terminal 2: run stress test
./scripts/stress_test.sh --rate 10000 --duration 10
```
- Dashboard: spike detected, auto-mitigation triggers
- Rules auto-deploy, blocked counter increments

### 15-22s: Rule Inspector
- Click "Rules" tab
- Expand a triggered rule: show match conditions, action, hit count
- Click "Explain" → rule AST dump

### 22-28s: Metrics Export
```bash
curl -s localhost:9090/metrics | head -20
```
- Show Prometheus metrics endpoint live

### 28-30s: Clean Shutdown
- Ctrl+C in main terminal
- Graceful shutdown banner, metrics flush

## Post-Processing
```bash
# Optimize GIF (ffmpeg)
ffmpeg -i raw.gif -vf "fps=15,scale=1280:-1:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse" -loop 0 demo.gif

# Target: <5MB for Twitter, <10MB for GitHub
gifsicle -O3 --lossy=80 -o demo-optimized.gif demo.gif
```

## Output Paths
- **Raw**: `docs/assets/demo-raw.gif`
- **Optimized**: `docs/assets/demo.gif` (commit this)
- **WebP fallback**: `docs/assets/demo.webp`

## Usage
| Platform | File | Max Size |
|----------|------|----------|
| GitHub README | `docs/assets/demo.gif` | 10MB |
| Twitter/X | `docs/assets/demo.gif` | 5MB |
| LinkedIn | `docs/assets/demo.webp` | 8MB |
| Blog embed | `docs/assets/demo.gif` | - |

## Checklist
- [ ] Record raw footage (multiple takes)
- [ ] Trim to 30s exactly
- [ ] Optimize with ffmpeg/gifsicle
- [ ] Verify <5MB
- [ ] Add to `docs/assets/demo.gif`
- [ ] Update README.md with `![demo](docs/assets/demo.gif)`
- [ ] Add to docs/PITCH.md
- [ ] Upload to social drafts

---

**Status**: [ ] RECORDED  [ ] OPTIMIZED  [ ] COMMITTED  [ ] EMBEDDED