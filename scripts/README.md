# RamShield Testing Suite

One entry point for every check. Authorized testing only — everything binds
and talks to `127.0.0.1` on scratch ports, never touching a live instance.

```bash
python3 scripts/suite.py <layer>
```

| Layer | What it does | Exit |
|-------|--------------|------|
| `lint` | `cargo fmt --all --check` + `cargo clippy --all-targets -- -D warnings` (CI gates) | fail count |
| `unit` | `cargo test --all` — Rust unit + integration tests | fail count |
| `e2e`  | Boots a release binary on scratch ports (IPC `:17890`, dash `:19999` via `RAMSHIELD_*` env overrides), drives the real JSON-over-TCP protocol: health, check/block/unblock, batch ingestion → EWMA auto-block, distinct-IP /24 subnet block, stats, dashboard snapshot, malformed-input error frames | fail count |
| `load` | Attack profiles through the retained simulator (`attack_nexus.py`): `profiles` list, `run --profile NAME --duration S`, `bench` (5-min subnet DDoS benchmark) | process rc |
| `all`  | `lint` + `unit` + `e2e` in CI order | total fails |

## Examples

```bash
# full CI pass
python3 scripts/suite.py all

# just the end-to-end protocol test (needs target/release/ramshield)
python3 scripts/suite.py e2e

# 30s HTTP-flood against a scratch server
python3 scripts/suite.py load run --profile l7_http_flood --duration 30

# the heavy benchmark (30 unique /24s per 15s rotation, 5 minutes)
python3 scripts/suite.py load bench
```

## e2e checks performed

1. `healthz` returns `{"status":"ok"}`
2. `check_ip` on an unknown IP → clean
3. `block_ip` → `check_ip` blocked → `unblock_ip` → clean (round-trip)
4. `report_connections` batch accepted (`proto_fp` field required)
5. Sustained batches above `rps_threshold` → EWMA auto-block fires
   (detector needs 2 consecutive hot EWMA samples — the suite drives ~20)
6. 250 distinct IPs from one /24 → subnet block fires on any member
7. `get_stats` returns counters
8. Dashboard `/api/snapshot` healthy with blocked > 0
9. Malformed IP → typed `error` frame with code 400; connection survives

## Removed legacy scripts

`attack_sim_100k.py`, `attack_extreme.py`, `cruel_ddos.py`, `attack_driver.py`
(stale port 19847), `scenario_runner.py`, `generate_scenarios.py`,
`map_and_run.py`, `create_mapped_scenarios.py`, `selftest.sh`,
`check_guardrails.sh` — all functionality lives in the suite layers above.
`attack_nexus.py` is retained as the load engine behind `suite.py load`.
