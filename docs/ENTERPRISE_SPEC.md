# RamShield Enterprise Application Requirements & System Architecture

## 1. Strategic Vision
RamShield evolves from a monolithic DDoS mitigation framework into a **distributed, sovereign enterprise network intelligence platform**. The goal is not just to block attacks, but to provide real-time, actionable traffic intelligence for European SMEs, compliant with local data residency laws (GDPR, Polish KSeF integration).

## 2. Core Enterprise Extensions (Post-Crate Decomposition)

### 2.1 Distributed State Sync (Raft Consensus Layer)
- **Problem**: Isolated DashMap nodes create inconsistent blocklist states across az regions (Warsaw + Frankfurt).
- **Solution**: Multi-Raft cluster using `openraft`. Commit replicated WAL entries to every node.
- **Enterprise Value**: Enables active-active clusters for European cloud startups (e.g., Hetzner PL residency). Zero data loss on node failures.

### 2.2 eBPF / XDP Network Acceleration
- **Problem**: Handling 1M+ events/sec via Axum/Tokio userspace consumes CPU cycles even with optimized batching and DashMaps.
- **Solution**: Load XDP programs that drop malicious packets at the kernel level before socket allocation.
- **Performance**: Push throughput above 10M pps per core. Instant, hardware-friendly drops for known bad IPs.

### 2.3 Enterprise Compliance & Auditing (KSeF, GDPR, BLIK)
- **Problem**: Polish/EU clients demand cryptographically verifiable enforcement trails for blocking, rate-limiting, and data access events.
- **Solution**: Append-only Merkle tree `.audit` ledger integrated with KSeF e-invoice reporting schemas and GDPR automated data-purging triggers.
- **Integration**: Compatible with Polish standards like mojeID and ePUAP for identity verification.

### 2.4 Zero-Trust mTLS Mesh for Edge Nodes
- **Problem**: Agent-to-Server communication (RPC 7890) currently lacks strong endpoint verification.
- **Solution**: Mandatory TLS 1.3 with automatic certificate provisioning via `rustls` and Let's Encrypt. Each agent and server presents strong identity for mutual trust.

---

## 3. Technology Stack Update
- **Raft Consensus**: `openraft` (or `raft-rs` with Atomic batch state).
- **eBPF Integration**: `aya` Rust eBPF library. Dust scripts for kernel hooks.
- **mTLS**: `rustls` + `rcgen` for auto-provisioning.
- **Audit Logging**: Merkle tree implementation using `sha3` hash functions with immutable block commitments.

---

## 4. Roadmap
- **Phase 3**: Integrate `openraft` consensus into `ramshield-engine`.
- **Phase 4**: Implement eBPF XDP drop loader.
- **Phase 5**: Deploy compliance daemon with KSeF integration.