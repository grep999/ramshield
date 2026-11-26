# Reproduce RamShield claims

Use this before repeating a number in public. Every claim needs a command, a commit SHA, and an environment.

## Environment

Record it once:

```bash
git rev-parse HEAD
uname -r
lscpu | sed -n 's/^Model name:[[:space:]]*//p' | head -1
nproc
rustc --version
```

A raw 2026-11-25 capture is at `/home/m/out/ramshield_promotion/evidence/2026-11-25_demo_raw.txt`. It is evidence, not a checked-in result.

## Attack demo

```bash
cargo run --example attack_demo --quiet 2>&1 | tee attack_demo_$(date +%Y%m%d).txt
```

Expected shape, not exact timings:

- 10 IPv6 sources × 1 event: 0 promoted addresses.
- 60 sources × 50 events = 3,000 events: 60 promoted addresses, 1 subnet.
- 1 source × 10,000 events: 1 promoted address.

Say "promoted", not "blocked". The one exception is the good-IP observation: "0 of 200 good IPs blocked in one run". That is an observation, not a false-positive estimate; rule-of-three upper bound is 1.5%.

## Benchmarks

```bash
cargo bench --bench hot_paths
cargo bench --bench field_day
```

Numbers quoted in `README.md` and `docs/BENCHMARKS.md` are historical single-run E1 results. Raw data is not in-tree. Do not present them as wire-rate, independent, or reproducible until raw data and five sessions exist.

## Failures

Do not hide failed cases. `docs/BENCHMARKS.md` records T19, T13 and T8 failures. Any public claim must keep them attached.

## Report template

```text
Command:
Commit SHA:
Kernel / CPU / cores / rustc:
Exit code:
Raw output path:
What changed vs previous run:
```
