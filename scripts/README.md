# RamShield attack simulators (authorized testing only)

Use only against **your own** RamShield instance (`127.0.0.1:7890`).

## Recommended: `attack_nexus.py`

Next-gen simulator based on common multi-vector DDoS taxonomies (L3 volumetric, L4 protocol, L7 application) and patterns seen in open-source stress frameworks: mixed rotation, entropy botnets, slow/low + flood combos, k6-style ramps.

```bash
# List profiles
./scripts/attack_nexus.py profiles list

# HTTP flood (L7)
./scripts/attack_nexus.py run --profile l7_http_flood --duration 60 --workers 256

# Full red-team chain (4 phases)
./scripts/attack_nexus.py run --profile red_team_full

# Ramp-up like k6 (approximate EPS scaling)
./scripts/attack_nexus.py run --profile botnet_distributed --ramp 1000 50000 120 --duration 120

# Interactive shell — full customization
./scripts/attack_nexus.py shell
```

### Shell highlights

```
nexus> profiles load l4_syn_wave
nexus> set workers 512
nexus> set jitter 0 20
nexus> set pareto 64 8192 2.0
nexus> set hot_ip_ratio 0.3
nexus> seed 42
nexus> ramp 5000 80000 90
nexus> macro baseline          # save current settings
nexus> flood 120
nexus> stats
```

Edit or add scenarios in `profiles.json`.

## Legacy scripts

| Script | Purpose |
|--------|---------|
| `attack_sim_100k.py` | Simple fixed 100k burst |
| `attack_extreme.py` | Burst/flood/phase + basic REPL |

## Profile → RamShield mapping

Real attacks are simulated via IPC `report_connections`:

| Profile class | Simulated behavior |
|---------------|-------------------|
| L7 HTTP flood | High RPS, weighted 2xx/4xx/5xx, browser byte sizes |
| RUDY / slowloris | Large or tiny payloads, hot IP |
| SYN wave | Subnet bursts, small packets, mixed 503 |
| Botnet | High IP entropy (Zipf/Pareto pools) |
| DNS amp | Multi-/24 spikes |
| Credential spray | 401/403/429 weighted, scan rotation |
