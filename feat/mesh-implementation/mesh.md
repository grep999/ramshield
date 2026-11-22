# RamShield Mesh Implementation Design

## Overview
RamShield mesh is a production-ready edge synchronization engine that propagates block decisions across peer nodes. It replaces the experimental AWORSet-based mesh with a clean TTL Last-Write-Wins (LWW) state table bounded by `rambudget`, with zero tombstones and split-horizon broadcast.

## Target Architecture

```
  ┌─────────────────────────────────────────────────────────────┐
  │                 Remote Peer Nodes (TCP)                     │
  └───────────────▲─────────────────────────────▲───────────────┘
                  │ Fixed 48-byte frames                    │
                  │ (HMAC-SHA256 authenticated)             │
                  ▼                                         ▼
  ┌─────────────────────────────────────────────────────────────┐
  │                   ramshield-protocol                        │
  │   • Zero-alloc binary framing (No JSON)                     │
  │   • Truncated 16-byte HMAC verification                     │
  │   • Clock-skew clamping (max drift: ±5s)                    │
  └──────────────────────────────┬──────────────────────────────┘
                                 │
                                 ▼
  ┌─────────────────────────────────────────────────────────────┐
  │                      ramshield-mesh                         │
  │   • TTL-based LWW State Table (Replaces AWORSet)            │
  │   • Bounded by rambudget (Zero tombstones)                  │
  │   • Split-horizon broadcast with ramgate throttling         │
  └───────────────┬─────────────────────────────┬───────────────┘
                  │                             │
                  ▼ Local Updates               ▼ Reconciled Blocks
  ┌──────────────────────────────┐┌─────────────────────────────┐
  │    ramwal (Persistence)      ││    ramshield-enforcement    │
  │   • Append-only recovery     ││   • Updates XDP eBPF Maps   │
  │   • Fast replay on reboot    ││   • Prunes expired bans     │
  └──────────────────────────────┘└─────────────────────────────┘
```

## Phase 1: Wire Protocol & Serialization Overhaul (`ramshield-protocol`)

### 1.1 Fixed Frame Definition
- **File:** `crates/ramshield-protocol/src/frame.rs` (New)
- **Byte Layout (Fixed 48 Bytes):**
  - `[0..2]`: Magic bytes `0x524D` (`"RM"`)
  - `[2..3]`: Protocol version (`0x01`)
  - `[3..4]`: Message type (`0x01` = Block, `0x02` = Unblock, `0x03` = Heartbeat)
  - `[4..20]`: IP Address bytes (`[u8; 16]`, IPv4-mapped IPv6 or native IPv6)
  - `[20..21]`: Prefix length (`u8`, e.g., 32, 24, 128)
  - `[21..22]`: Flags (`0x01` = Autonomous, `0x02` = Manual)
  - `[22..24]`: Reserved/Padding
  - `[24..32]`: `blocked_until` (u64, Unix epoch seconds)
  - `[32..40]`: `updated_at` (u64, Unix epoch seconds)
  - `[40..48]`: Truncated HMAC-SHA256 (`[u8; 8]` or `[u8; 16]`) over bytes `0..32`

### 1.2 Framing Codec
- **File:** `crates/ramshield-protocol/src/codec.rs`
- Implement `tokio_util::codec::Decoder` and `Encoder` using `bytes::BytesMut`
- **Zero-copy read:** Read `[u8; 48]` directly from socket buffer without intermediate string conversions
- Verify `magic == 0x524D` and HMAC constant-time equality (`subtle::ConstantTimeEq`)

### 1.3 Clock Drift Mitigation
- **File:** `crates/ramshield-protocol/src/validate.rs`
- If `updated_at > now + 5s`, clamp `updated_at` to `now + 5s` to prevent remote nodes with desynchronized clocks from permanently anchoring an un-evictable block

## Phase 2: In-Memory State Overhaul (`ramshield-mesh`)

### 2.1 Remove Legacy CRDT Engine
- **Action:** Delete `crates/ramshield-mesh/src/aworset.rs`, `dots.rs`, and causal vector clock files
- No more AWORSet, dot-stores, or causal tombstones

### 2.2 Implement `LwwBlockTable`
- **File:** `crates/ramshield-mesh/src/state.rs`
- State container: `HashMap<IpNetwork, BlockRecord>` bounded by memory limits

```rust
#[derive(Clone, Copy, Debug)]
pub struct BlockRecord {
    pub blocked_until: u64,
    pub updated_at: u64,
    pub flags: u8,
}
```

**Merge Rules:**
1. If `now >= incoming.blocked_until`: Drop (already expired)
2. If IP not present: Insert
3. If IP present: Update **only if** `incoming.blocked_until > current.blocked_until` OR (`incoming.blocked_until == current.blocked_until && incoming.updated_at > current.updated_at`)

### 2.3 Periodic Expiration Pruning
- **File:** `crates/ramshield-mesh/src/gc.rs`
- Add `prune_expired(&mut self, now: u64) -> Vec<IpNetwork>`
- Scans the map, strips records where `blocked_until <= now`, returns expired IPs for kernel XDP cleanup

### 2.4 Wire Memory Tracking into `rambudget`
- Enforce hard byte ceilings on the table
- If memory crosses limits, evict entries nearest to their expiration time first

## Phase 3: Kernel Enforcement & WAL Persistence Integration

### 3.1 Unify Local and Remote Enforcement Hooks
- **File:** `crates/ramshield-mesh/src/sync_worker.rs`
- When an incoming block is successfully merged:
  1. Call `ramshield_storage::Wal::append_block(&wal, ip, ttl)`
  2. Call `ramshield_enforcement::Enforcer::apply_block(&mut enforcer, ip, duration)`

This ensures that whether an IP was detected locally by `ramshield-detection` or received over the mesh, it hits kernel XDP in sub-millisecond time.

### 3.2 Automated Prune Hook
On each maintenance tick (every 5 seconds):
```rust
let expired = mesh_table.prune_expired(now);
for net in expired {
    enforcer.remove_block(net);
}
```

### 3.3 Crash Recovery Flow (`ramwal` Replay)
On node startup:
1. Replay `ramwal` log
2. Populate `LwwBlockTable`
3. Re-project active, unexpired blocks into XDP maps before opening the network listening port

## Phase 4: Peer Connection Manager, Split-Horizon & Throttling

### 4.1 Topology & Configuration
- **File:** `crates/ramshield-config/src/mesh.rs`
- Add mesh configuration to `config.toml`:
```toml
[mesh]
enabled = true
listen = "0.0.0.0:9443"
psk_hmac_hex = "a3f5b7c8...32bytes"
max_sync_per_sec = 500 # Enforced by ramgate

[[mesh.peers]]
address = "198.51.100.2:9443"
name = "edge-us-east"

[[mesh.peers]]
address = "203.0.113.5:9443"
name = "edge-eu-west"
```

### 4.2 Connection Engine & Auto-Reconnect
- **File:** `crates/ramshield-mesh/src/transport.rs`
- Dedicated `tokio` task per peer
- Reconnect with exponential backoff (`1s, 2s, 4s, 8s... max 30s`) on connection drops

### 4.3 Split-Horizon Gossip Engine
- **File:** `crates/ramshield-mesh/src/gossip.rs`
- Every outgoing event carries an `origin_peer_id`
- **Split-Horizon Rule:** If Node A receives a block from Peer B, Node A replicates it locally and pushes it to Peer C, but **never echoes it back to Peer B**

### 4.4 Outbound Rate Limiting via `ramgate`
Protect mesh links during massive volumetric attacks:
```rust
if peer_rate_limiter.check_rate(peer_id).is_ok() {
    peer_tx.send(frame).await?;
} else {
    // Drop gossiping this block; peer will catch up during next state reconciliation
    metrics::increment_counter!("mesh_gossip_dropped_rate_limit");
}
```

## Phase 5: Observability, Metrics & CLI Integration

### 5.1 CLI Commands (`ramshield-cli`)
- `ramshield-cli mesh status` — Displays: Active peers, connection states (Connected/Reconnecting), round-trip latency, last sync timestamp
- `ramshield-cli mesh blocks` — Displays: Total synchronized blocks, blocks received from peers vs. generated locally
- `ramshield-cli mesh test-sync <ip> --ttl 60` — Injects a synthetic block and prints propagation latency across the cluster

### 5.2 Prometheus Telemetry Metrics
- `ramshield_mesh_connected_peers`: Gauge (current live peer count)
- `ramshield_mesh_blocks_received_total`: Counter
- `ramshield_mesh_blocks_sent_total`: Counter
- `ramshield_mesh_sync_bytes_total`: Counter
- `ramshield_mesh_clock_drift_seconds`: Gauge

## Phase 6: Test Suite & Acceptance Criteria

### 6.1 Unit Tests (`cargo test -p ramshield-protocol -p ramshield-mesh`)
1. **Zero-Alloc Codec Test:** Benchmark encoding/decoding 1,000,000 frames; assert **0 bytes** of heap allocation on the hot path
2. **LWW Merge Correctness:** Verify that older updates never overwrite newer updates regardless of receipt order
3. **HMAC Tamper Test:** Mutate 1 bit in the IP address or timestamp; assert packet drops immediately on HMAC failure

### 6.2 Multi-Node Loopback Integration Test
**Setup:** Launch 3 local `ramshield` instances listening on ports `9441`, `9442`, and `9443` in loopback mode

**Test Case:**
1. Trigger an autonomous block for `192.0.2.100` on Node 1
2. Verify Node 2 and Node 3 install the block in their respective XDP maps within ≤ 5 ms
3. Ensure no broadcast loops occur (verify packet counts match expected split-horizon behavior)

### 6.3 Churn & Memory Stability Test
**Setup:** Single node under simulated attack conditions

**Stress:** Inject **50,000 unique blocks per minute** with 10-second TTLs over the mesh connection for 30 minutes

**Pass Criteria:**
1. Resident Set Size (RSS) memory strictly honors `rambudget` limit without unbounded heap growth
2. Expired blocks are cleanly purged from both userspace memory and eBPF maps
3. CPU overhead for mesh frame processing stays < 5% on a modern core

## Implementation Workstream & Timeline

| Step | Workstream | Files Touched / Created | Target Output |
|------|-----------|------------------------|---------------|
| **W1** | **Wire Protocol Overhaul** | `crates/ramshield-protocol/src/{codec,frame,validate}.rs` | Fixed 48-byte binary framing with HMAC; delete JSON dependencies |
| **W2** | **LWW State Engine** | `crates/ramshield-mesh/src/{state,gc}.rs` | Purge AWORSet; implement TTL-LWW map; integrate with `rambudget` |
| **W3** | **Enforcement & WAL Wiring** | `crates/ramshield-mesh/src/sync_worker.rs` | Wire merged blocks directly to `ramwal` and `ramshield-enforcement` |
| **W4** | **Transport & Gossip Throttling** | `crates/ramshield-mesh/src/{transport,gossip}.rs` | Peer manager, split-horizon logic, and `ramgate` broadcast rate limiter |
| **W5** | **Config & CLI Visibility** | `crates/ramshield-config`, `crates/ramshield-cli` | TOML config parsing, metrics collection, and `ramshield-cli mesh` commands |
| **W6** | **Benchmarking & Hardening** | `tests/mesh_loopback.rs`, `benches/codec_bench.rs` | Loopback integration tests, memory boundary validation, and release validation |

Following this roadmap removes the architectural dead-ends of the old mesh, keeping the code footprint under ~400 lines of clean, testable, and memory-safe Rust that works with the rest of the `grep999/ramshield` stack.