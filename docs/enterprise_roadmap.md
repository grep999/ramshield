# RamShield Enterprise Architecture & Roadmap Extensions

## 1. Executive Summary
RamShield is transforming into an enterprise-grade, distributed DDoS mitigation and traffic rationalization engine. To support sovereign cloud environments (e.g., Polish SMEs, Hetzner PL residency, KSeF/Allegro/InPost compliance), we propose three strategic extensions:
1. **Distributed State Sync (Raft-backed Clustering)**: Replaces single-node DashMap with a replicated, multi-region threat state.
2. **eBPF / XDP Data-Plane Acceleration**: Offloads layer-4 packet drop logic straight to the Linux kernel network stack, bypassing userspace overhead for known bad IPs.
3. **Zero-Trust mTLS Mesh & KSeF/GDPR Auditing**: Enforces strict mutual TLS between edge nodes and provides cryptographically verifiable compliance trails.

---

## 2. Distributed State Sync (Raft Consensus)
### Problem
Currently, RamShield instances are isolated, running localized mitigation. In a multi-region cloud setup (e.g., Warsaw + Frankfurt), a blocked IP in Warsaw must be instantly propagated to Frankfurt without waiting for full attack saturation.
### Architecture
- **Protocol**: Multi-Raft using `openraft`.
- **State Machine**: Replicated `ramshield-storage` WAL (Write-Ahead Log) entries. When an IP is blocked via detection or manual operator action, a Raft proposal is committed.
- **Latency Target**: < 5ms intra-region, < 35ms inter-region state propagation.

---

## 3. eBPF / XDP Network Acceleration
### Problem
Handling 1,000,000+ events/sec entirely in user-space Axum/Tokio consumes CPU cycles even with optimized DashMaps and batching.
### Architecture
- **Hook Point**: `XDP_DROP` at the network driver level.
- **Map Type**: BPF HashMap containing blocked IPs synced directly from `ramshield-storage`.
- **Performance**: Drops malicious packets before kernel socket allocation, increasing throughput to 10M+ pps per core.

---

## 4. Compliance & Enterprise Auditing (KSeF & GDPR)
### Problem
Polish/EU enterprise clients require immutable, cryptographically signed audit trails of all security enforcement actions (blocking, rate-limiting, data scrubbing).
### Architecture
- **Audit Ledger**: Append-only Merkle tree logging all block decisions.
- **Integration**: Direct export formats compatible with KSeF (Krajowy System e-Faktur) audit schemas and automated GDPR data-purge triggers.

---

## 5. Implementation Roadmap
1. **Phase 1**: Finalize monolithic crate decomposition (current step).
2. **Phase 2**: Introduce Raft consensus layer into `ramshield-engine`.
3. **Phase 3**: Develop eBPF XDP loader script and kernel integration.
4. **Phase 4**: Enterprise compliance reporting daemon.
