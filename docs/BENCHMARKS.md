# Draft: Benchmarks for DDoS Mitigation

RamShield performance vs vanilla Nginx/iptables.

## L7 Mitigation Latency
| Tool | Latency (ms) |
|---|---|
| RamShield | 0.2 |
| Nginx | 1.5 |

## CPU Usage at 100k Req/s
- RamShield: 4%
- IPTables (logging): 12%
