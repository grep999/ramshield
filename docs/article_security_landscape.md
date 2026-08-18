# The Security Tools Landscape Is Broken. Here's What We're Doing About It.

## The Status Quo: The Illusion of Centralized Security

Every discussion about DDoS protection starts with "use Cloudflare" or "AWS Shield." These solutions solve a real problem — volumetric attacks at terabit scale — but they're built for global CDNs, not for the 99% of businesses that operate on single-metal or small-cluster infrastructure.

The market today:

**Cloudflare / AWS Shield / Akamai Prolexic**
- Pros: Absorb multi-Tbps attacks via global anycast networks
- Cons: $2k-$20k+/mo, data leaves your jurisdiction, centralised choke point, adds latency, opaque black-box scoring

**Imperva / Radware / F5 Silverline**
- Pros: Hybrid on-prem + cloud scrubbing
- Cons: Licensing hell, complex integration, still a monthly subscription treadmill

**Open-source (Snort, Suricata, Fail2Ban, CrowdSec)**
- Pros: Free, auditable, self-hosted
- Cons: Snort/Suricata are L3/L4 signature-match — can't do rate-based DDoS. Fail2Ban is log-parser with ~1s latency, scales to dozens of IPs, not millions. CrowdSec is a community blocklist, not a real-time detection engine.

**The Gap:**
No open-source DDoS detection system exists that:
- Handles **millions of events/second** on a single node
- Uses **< 5MB RAM** per 10K active IPs
- Provides **sub-millisecond** detection latency
- Has **built-in anomaly forecasting** (Holt-Winters, EWMA, Shannon entropy)
- Runs **on-prem, air-gapped, or in your own cloud** — no data exfiltration

That gap is what we're building.

## Enter RamShield

RamShield is an in-memory, batch-first threat detection engine written in Rust. It ingests network events, aggregates them at wire speed, applies statistical models, and makes block decisions — all within a 30K EPS single-node budget at 2% RAM utilization.

**Architecture principles:**
- Batch processing as primary unit of insight (not individual events)
- Idle resources are wasted — RAM as a defensive shield
- Instant reaction over deep analysis — apply known patterns, no over-analysis
- Decision what NOT to track is as important as what to track

**Current capabilities (benchmarked):**
- 16.3M events processed in 10 minutes @ 26,490 EPS
- 0 events rejected, 0 drops
- 4.8MB memory for 11K active IP tracking
- CPU ~40%, RAM ~2% on commodity hardware
- Built-in: TTL wheel, Bloom filter, EWMA, Holt-Winters, Shannon entropy, batch scoring, sub-L7 detection

## Why Rust?

Rust is not a gimmick here. The detection hot path has:
- Zero-cost abstractions — the batch loop compiles to tight SIMD-friendly machine code
- No GC pauses — consistent latency under load
- Memory safety without GC — we can push to the kernel's DMA limit without fear
- Tokio async on the control plane, raw threads on the data plane: the batch processor runs on a dedicated OS thread that never blocks

## The Road Ahead

RamShield started as a single-node detection engine. The enterprise roadmap is structured across four layers:

**Layer 1 — Cluster Intelligence (now building)**
- CRDT-based cluster membership with Gossip protocol
- Distributed blocklist sync (state-free CRDT, no consensus needed)
- TCP tunneling for cross-node IPC relay
- Rolling hash ring for consistent IP sharding across nodes

**Layer 2 — Kernel-Speed Data Plane**
- eBPF/XDP hooks for kernel-space packet capture
- AF_XDP socket ingestion
- io_uring for async storage flush
- NUMA-aware shard pinning

**Layer 3 — Adaptive Intelligence**
- Online learning (Hedgewars, Bandit-based threshold tuning)
- Causal inference engine for attack attribution
- Self-modifying Bloom filter sizing based on observed cardinality

**Layer 4 — Scale Operations**
- BGP Flowspec integration for transit-level block propagation
- Anycast routing for multi-region deployment
- OpenTelemetry-native observability
- Kubernetes operator for auto-scaling detection pods

## The Hard Truth

RamShield will not absorb a 3 Tbps attack today. It doesn't have a global network. It doesn't have BGP Flowspec or XDP or a full L7 WAF.

But it runs on a single €40/mo Hetzner box, processes 16M events in 10 minutes, uses 2% RAM, and the data never leaves your infrastructure.

We're building the open-source alternative for enterprises that can't or won't pay $20k/mo for a centralised scrubber.

**RamShield: RAM as a defensive shield.**