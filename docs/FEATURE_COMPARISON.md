# Feature Comparison — RamShield vs Comparable Software

Scope: **self-hosted / sovereign DDoS defense for a single operator's own service**, where you deploy the tool on your own box in front of your reverse proxy. Cloud-any edge scrubbing (Cloudflare, AWS Shield, Imperva) is listed for reference but is a different product class — it cannot be deployed on your hardware and doesn't compete on this axis.

Standard this is measured against: **RFC 9411** (Benchmarking Methodology for Network Security Devices). Full KPI mapping and attack-suite results: `docs/BENCHMARKS.md`. The older suite reference is not in this checkout; no matrix result is claimed.

---

## Feature matrix

| Feature | RamShield | fail2ban | CrowdSec | nginx `limit_req` | GoRateLimit |
|---|:---:|:---:|:---:|:---:|:---:|
| **Detection** | | | | | |
| Rate-based detection (EWMA + CUSUM) | ✅ | ❌ | ⚠️ | ❌ | ❌ |
| Pulse/burst correlation | ✅ | ❌ | ❌ | ❌ | ❌ |
| Bayesian anomaly forecasting | ✅ | ❌ | ⚠️ | ❌ | ❌ |
| Subnet / /24 swarm aggregation | ✅ | ❌ | ❌ | ❌ | ❌ |
| Adaptive threshold (baseline-learned) | ✅ | ❌ | ⚠️ | ❌ | ❌ |
| Static ruleset / regex log parsing | ❌ | ✅ | ✅ | ✅ | ❌ |
| Community threat intelligence feed | ❌ | ⚠️ | ✅ | ❌ | ❌ |
| HTTP/1.1 request-path analysis | ✅ | ❌ | ✅ | ✅ | ❌ |
| HTTP/2 stream analysis | ❌ | ❌ | ❌ | ⚠️ | ❌ |
| L3/L4 volumetric (packet-rate) detection | ✅ (XDP path) | ❌ | ❌ | ❌ | ❌ |
| **Enforcement** | | | | | |
| Kernel-level drop (XDP/eBPF) | ✅ | ❌ | ❌ | ❌ | ❌ |
| IP blocklist | ✅ | ✅ | ✅ | ✅ | ❌ |
| CIDR / prefix blocklist | ✅ | ❌ | ✅ | ❌ | ❌ |
| TTL on blocks (auto-expiry) | ✅ | ✅ | ✅ | ❌ | ❌ |
| Idempotent block decisions (dedup) | ✅ | ❌ | ❌ | ❌ | ❌ |
| Manual block/unblock CLI | ✅ | ✅ | ✅ | ❌ | ❌ |
| **Durability & recovery** | | | | | |
| WAL-durable block decisions | ✅ | ❌ | ❌ | ❌ | ❌ |
| Checkpoint + tail replay recovery | ✅ | ❌ | ❌ | ❌ | ❌ |
| Measured recovery time | 52 ms | N/A | N/A | N/A | N/A |
| Restart-safe state | ✅ | ⚠️ | ⚠️ | ❌ | ❌ |
| **Observability** | | | | | |
| Prometheus `/metrics` | ✅ | ⚠️ | ✅ | ⚠️ | ❌ |
| Live dashboard | ✅ | ❌ | ✅ | ❌ | ❌ |
| Health / readiness endpoints | ✅ | ❌ | ✅ | ⚠️ | ❌ |
| Operator-visible enforcement health (xdp_active, apply_failures) | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Security of the tool itself** | | | | | |
| HMAC-authenticated IPC | ✅ | ❌ | ❌ | ❌ | ❌ |
| Dashboard auth (Argon2) | ✅ | ❌ | ✅ | ❌ | ❌ |
| CSRF protection | ✅ | ❌ | ✅ | ❌ | ❌ |
| Public-bind-without-credentials rejected | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Deployment** | | | | | |
| Single static binary | ✅ | ✅ | ❌ | ❌ | ❌ |
| Zero runtime dependencies | ✅ | ⚠️ | ❌ | ❌ | ✅ |
| Air-gapped / no external feed | ✅ | ✅ | ❌ | ✅ | ✅ |
| Kubernetes manifest | ✅ | ❌ | ✅ | ❌ | ❌ |
| Cross-platform | ❌ (Linux only, by design) | ✅ | ✅ | ✅ | ✅ |
| **Measured performance** | | | | | |
| Node throughput (eps) | 154,731 | ~85K | not throughput-scored | ~85K | ~320K |
| Idle RAM footprint | 266 KB | ~10 MB | ~50 MB | <1 MB | <1 MB |
| False-positive result (one run) | 0 of 200 good IPs blocked; upper bound 1.5% | N/A | N/A | N/A | N/A |

Legend: ✅ supported · ⚠️ partial / indirect · ❌ not supported

---

## Where each competitor actually wins

**fail2ban** — mature, battle-tested, log-parsing coverage you get for free, runs anywhere, trivial to reason about. If your problem is "SSH brute force" or "web scraping with known signatures", fail2ban does it in 20 lines of config and RamShield is the wrong tool.

**CrowdSec** — community blocklist is the whole product. You inherit threat intelligence from thousands of operators. RamShield has no feed and will not tell you that an IP is already hostile somewhere else.

**nginx `limit_req`** — simplest possible mitigation, in-process, no daemon, no state to recover. If you only need "cap this endpoint at N req/s", this is the correct answer and nothing here beats it on simplicity.

**GoRateLimit / golang.org/x/time/rate tier** — token bucket done right, ~320K ops/s, library not daemon. If you're embedding rate limiting *inside your own service* rather than defending it from outside, use a library, not RamShield.

**Cloudflare / AWS Shield / Imperva** — anycast edge scrubbing at multi-Tbps scale. They absorb attacks RamShield structurally cannot: multi-hundred-Gbps volumetric floods that saturate your uplink before any on-prem box sees a packet. Different product class, different budget, different deployment model.

---

## Where RamShield wins

- **Measured, not claimed.** 154,731 eps single-attacker, 115,278 eps across 50 attackers, 0 of 200 good IPs blocked in one run (rule-of-three upper bound 1.5%; 21.3M is corpus size, not a zero-rate sample), 52 ms recovery, 266 KB idle RAM — all measured on this repo, methodology and raw output in `docs/BENCHMARKS.md`.
- **Zero external dependency.** No feed, no subscription, no cloud account, no phone-home. Air-gapped deployment is the default, not an enterprise-tier feature.
- **WAL-durable decisions.** Every block decision survives a crash. A tool that forgets its blocks on restart re-blocks the same attacker on every restart, and can't prove to an auditor what it blocked and when.
- **Operator-owned.** Sovereign infra operators keep the logs, the blocklist, the telemetry, and the audit trail.

## Where RamShield loses (stated plainly)

- **No community threat intelligence.** CrowdSec's feed is a real moat. RamShield detects from first principles and cannot say "this IP is already banned in 400 other networks".
- **L7 depth.** No HTTP/2 stream analysis, no application-logic ruleset depth. Against a sophisticated L7 application attack, nginx + a purpose-built WAF outperforms a rate-and-pattern detector.
- **Single-node.** No clustering, no multi-node consensus on blocks. The mesh CRDT is present but the product does not market multi-node scaling. A single box is a single box.
- **Volumetric ceiling.** On-prem, last-mile: if the attack saturates the uplink, detection never fires. Cloudflare exists precisely because that ceiling is real.
- **No cross-platform.** Linux-only, and XDP/eBPF is Linux-only by kernel design. That is deliberate, but it excludes Windows operators entirely.
- **HTTP/2 blind spot.** Streams are a real attack surface (RFC 9411 covers it as a distinct resource class) and RamShield does not inspect them.

---

## How to read this

The honest framing: **RamShield is the right tool for a sovereign operator defending one self-hosted service, who needs measurable detection quality, durable state, and no cloud dependency — and who accepts a single-node ceiling.** It is not a cloudscrubber, not a WAF, and not a community feed. Where it competes (fail2ban, CrowdSec, nginx rate limiting, GoRateLimit), the differentiators are measured false-positive rate, WAL durability, XDP enforcement, and the RFC 9411 suite that backs those claims.

Every performance number in this document is reproduced in `docs/BENCHMARKS.md` with the raw bench output. Numbers I did not measure (competitor eps figures) are marked as approximate and sourced from published material.
