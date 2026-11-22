# RamShield Mesh Implementation Design — PhD-Level Deep Dive

## 1. Distributed Systems Foundations

### 1.1 CAP Theorem Positioning
RamShield mesh prioritizes **Availability + Partition Tolerance (AP)** over consistency. During a DDoS attack, network partitions are expected; local state must remain writable and decisions must be locally enforceable.

**Consistency model:** Eventual consistency via Last-Write-Wins (LWW) with Hybrid Logical Clocks (HLC) for causality.

### 1.2 Clock Theory

| Clock | Space | Causality | Wall-time | Best For |
|-------|-------|-----------|-----------|----------|
| Lamport | O(1) | Yes | No | Partial ordering |
| Vector | O(n) | Yes | No | Concurrency detection |
| Hybrid Logical (HLC) | O(1) | Yes | Yes (~±ε) | **RamShield mesh** |

**HLC (Kulkarni et al., 2014):** 64-bit timestamp combining wall clock + logical counter. Maintains causality while remaining close to NTP. Used by CockroachDB, MongoDB, YugabyteDB.

**RamShield mesh timestamp format (64 bits):**
```
[ physical_time: 48 bits (ms since epoch) | logical: 14 bits (counter within ms) | node_id: 2 bits ]
```
- Physical time bounded to ±5s drift (clamped on receive)
- Logical counter ensures total order within same ms
- Node ID breaks ties for same (physical, logical) tuple

**Clock skew mitigation (per receive):**
```
if updated_at > now + 5s: clamp updated_at = now + 5s
if updated_at < now - 5s: reject (stale, possible replay)
```

### 1.3 CRDT Selection: LWW vs AWORSet

**Why AWORSet was rejected:**
- Requires per-element version vectors (tombstones) → unbounded growth
- Add-wins semantics means a removed IP can reappear if concurrent add exists
- For DDoS blocklists, **remove-wins** is safer (expired blocks must die)
- Tombstone garbage collection is complex and error-prone

**Chosen: TTL Last-Write-Wins (LWW)**
```
BlockRecord:
  blocked_until: u64    // absolute expiry time
  updated_at: u64       // HLC timestamp of last update
  flags: u8             // 0x01=Autonomous, 0x02=Manual
```

**Merge rule (deterministic, confluent):**
```
merge(current, incoming):
  1. if now >= incoming.blocked_until → Drop (already expired)
  2. if IP not present → Insert incoming
  3. if incoming.blocked_until > current.blocked_until → Update
  4. if incoming.blocked_until == current.blocked_until && incoming.updated_at > current.updated_at → Update
  5. otherwise → Keep current
```

**Property:** Converges to the same state on all nodes regardless of message order (CRDT confluence).

---

## 2. Gossip Protocol Design

### 2.1 Gossip Styles (Demers et al., 1987)

| Style | Mechanism | Use Case | RamShield Choice |
|-------|-----------|----------|------------------|
| Push | Sender broadcasts full state | Hot rumor propagation | **Used for new blocks** |
| Pull | Requester asks for missing state | Anti-entropy reconciliation | **Used for periodic sync** |
| Push-Pull | Both directions, diff-based | Most efficient | **Used for reconciliation** |

### 2.2 Split-Horizon Rule
```
If Node A receives block X from Peer B:
  → Replicate X locally
  → Push X to Peer C, D, E...
  → NEVER echo X back to Peer B
```

**Purpose:** Prevent broadcast storms and routing loops in mesh topology.

### 2.3 Anti-Entropy Periodic Reconciliation
```
Every T seconds (default 30s):
  1. Select random peer q from peer set
  2. Send digest: [(ip, blocked_until, updated_at), ...]
  3. Peer returns missing/newer entries
  4. Apply merge to all returned entries
  5. Prune expired locally
```

**Convergence time:** O(log(N)) cycles for epidemic spread (Demers et al.).

### 2.4 Rate Limiting via ramgate
```
struct SlidingWindowRateLimiter {
    window_ms: u64,
    max_per_window: u32,
    timestamps: VecDeque<Instant>,
}

impl SlidingWindowRateLimiter {
    fn check(&mut self) -> bool {
        let now = Instant::now();
        // Remove timestamps outside window
        while let Some(ts) = self.timestamps.front() {
            if now.duration_since(*ts) > self.window_ms { pop_front } else { break }
        }
        if self.timestamps.len() < self.max_per_window {
            self.timestamps.push_back(now);
            true
        } else {
            false  // drop gossiped block; peer catches up on next reconciliation
        }
    }
}
```

---

## 3. Wire Protocol (48-byte Fixed Frame)

### 3.1 Frame Layout
```
Offset  Size  Field              Description
[0..2]    2   Magic              0x524D ("RM")
[2..3]    1   Version            0x01
[3..4]    1   MsgType            0x01=Block, 0x02=Unblock, 0x03=Heartbeat
[4..20]   16  IP Address         [u8; 16] (IPv4-mapped IPv6 or native)
[20..21]  1   Prefix Length      u8 (32, 24, 128, etc.)
[21..22]  1   Flags              0x01=Autonomous, 0x02=Manual
[22..24]  2   Reserved           padding (0x0000)
[24..32]  8   blocked_until      u64 Unix epoch seconds
[32..40]  8   updated_at         u64 HLC timestamp
[40..48]  8   HMAC-SHA256        truncated to 8 bytes over bytes [0..40]
```

### 3.2 Zero-Allocation Codec
```rust
// Read directly from socket buffer — no intermediate String/Vec
fn decode_frame(buf: &[u8; 48]) -> Result<Frame, CodecError> {
    // Verify magic
    ensure!(buf[0..2] == [0x52, 0x4D], CodecError::BadMagic);
    // Verify version
    ensure!(buf[2] == 0x01, CodecError::BadVersion);
    // Verify HMAC (constant-time comparison)
    ensure!(subtle::ConstantTimeEq::ct_eq(&buf[40..48], &compute_hmac(&buf[0..40])),
            CodecError::BadHmac);
    // Decode fields directly from byte slices
    let ip = IpAddr::from_bytes(&buf[4..20]);
    let prefix_len = buf[20];
    let flags = buf[21];
    let blocked_until = u64::from_be_bytes(buf[24..32].try_into()?);
    let updated_at = u64::from_be_bytes(buf[32..40].try_into()?);
    Ok(Frame { ip, prefix_len, flags, blocked_until, updated_at })
}
```

**Properties:**
- 0 heap allocations on hot path
- Constant-time HMAC verification (timing attack resistant)
- Bounded frame size (48 bytes) — no variable-length parsing attack surface

### 3.3 HMAC-SHA256 Truncation
- Key: 32-byte pre-shared key (PSK) from config.toml `[mesh].psk_hmac_hex`
- Algorithm: HMAC-SHA256 over bytes [0..40]
- Output truncated to 8 bytes (sufficient for integrity in trusted mesh)
- Rejected if HMAC mismatch (drop frame, increment `mesh_hmac_failures_total`)

---

## 4. LWW State Table (`ramshield-mesh`)

### 4.1 State Structure
```rust
pub struct LwwBlockTable {
    table: HashMap<IpNetwork, BlockRecord>,
    budget: RambudgetHandle,       // hard byte ceiling
    order_counter: u64,            // for tie-breaking
}

pub struct BlockRecord {
    pub blocked_until: u64,        // expiry epoch
    pub updated_at: u64,           // HLC timestamp
    pub flags: u8,                 // Autonomous/Manual
    pub origin_peer: Option<PeerId>, // split-horizon tracking
}
```

### 4.2 Memory Bounding (rambudget integration)
```
if table_bytes > budget.limit:
    // Evict entries nearest to expiration first
    let mut candidates: Vec<(&IpNetwork, &BlockRecord)> = table.iter().collect();
    candidates.sort_by_key(|(_, r)| r.blocked_until);
    for (ip, _) in candidates.into_iter().take(evict_count) {
        table.remove(ip);
        metrics.inc("mesh_evictions_total");
    }
```

**Eviction order:** closest expiry first (expired entries naturally evicted by prune_expired before active ones).

### 4.3 Periodic Pruning (every 5s)
```rust
pub fn prune_expired(&mut self, now: u64) -> Vec<IpNetwork> {
    let expired: Vec<IpNetwork> = self.table.iter()
        .filter(|(_, r)| r.blocked_until <= now)
        .map(|(ip, _)| *ip)
        .collect();
    for ip in &expired {
        self.table.remove(ip);
    }
    expired  // caller removes from XDP maps + WAL
}
```

---

## 5. Transport & Connection Management

### 5.1 Per-Peer Connection Engine
```rust
struct PeerConnection {
    peer_id: PeerId,
    addr: SocketAddr,
    state: ConnectionState,  // Connected / Reconnecting / Disconnected
    rtt: Duration,           // smoothed round-trip time
    reconnect_delay: Duration, // exponential backoff: 1s, 2s, 4s... max 30s
    send_queue: mpsc::Sender<Frame>,
}
```

**Reconnect backoff:** `delay = min(base * 2^attempt, max_cap)` where base=1s, max=30s.

### 5.2 Topology Configuration (config.toml)
```toml
[mesh]
enabled = true
listen = "0.0.0.0:9443"
psk_hmac_hex = "a3f5b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6"
max_sync_per_sec = 500
clock_drift_tolerance_s = 5

[[mesh.peers]]
address = "198.51.100.2:9443"
name = "edge-us-east"
priority = 1

[[mesh.peers]]
address = "203.0.113.5:9443"
name = "edge-eu-west"
priority = 2
```

---

## 6. Enforcement & WAL Integration

### 6.1 Data Flow (Incoming Mesh Event)
```
Incoming Frame ──► HMAC Verify ──► LWW Merge ──► (if changed)
                                                    │
                                              ┌─────┴─────┐
                                              ▼           ▼
                                    wal.append_block()  enforcement.apply_block()
                                    (Crash persistence)  (XDP eBPF map update)
```

### 6.2 Data Flow (Local Detection)
```
Local Detection ──► LWW Merge ──► (if changed)
                                              │
                                              ▼
                                    wal.append_block()
                                    enforcement.apply_block()
                                    mesh.broadcast(frame)  // split-horizon
```

### 6.3 Crash Recovery (ramwal Replay)
```
Node Startup:
  1. Replay ramwal log → populate LwwBlockTable
  2. Filter: keep only records where blocked_until > now
  3. Re-project active blocks into XDP eBPF maps
  4. Open mesh listening port
  5. Begin gossip with peers
```

**Recovery guarantee:** Blocks survive node restart; no block lost if WAL fsynced before acknowledgment.

---

## 7. Observability

### 7.1 CLI Commands
```
ramshield-cli mesh status
  → Active peers, connection states, RTT, last sync timestamp

ramshield-cli mesh blocks
  → Total synchronized blocks, local vs peer-originated counts

ramshield-cli mesh test-sync <ip> --ttl 60
  → Inject synthetic block, measure propagation latency across cluster
```

### 7.2 Prometheus Metrics
| Metric | Type | Description |
|--------|------|-------------|
| `ramshield_mesh_connected_peers` | Gauge | Current live peer count |
| `ramshield_mesh_blocks_received_total` | Counter | Blocks received from peers |
| `ramshield_mesh_blocks_sent_total` | Counter | Blocks sent to peers |
| `ramshield_mesh_sync_bytes_total` | Counter | Bytes synced over mesh |
| `ramshield_mesh_clock_drift_seconds` | Gauge | Max observed clock drift |
| `ramshield_mesh_hmac_failures_total` | Counter | HMAC verification failures |
| `ramshield_mesh_gossip_dropped_rate_limit` | Counter | Gossip drops due to ramgate |
| `ramshield_mesh_evictions_total` | Counter | Memory-boundary evictions |

---

## 8. Test Suite

### 8.1 Unit Tests (`cargo test -p ramshield-protocol -p ramshield-mesh`)

**Test 1: Zero-Alloc Codec**
```rust
#[test]
fn codec_zero_alloc() {
    let frame = Frame::new(...);
    let encoded = frame.encode();  // 48 bytes
    let decoded = Frame::decode(&encoded).unwrap();
    assert_eq!(frame, decoded);
    // Benchmark: 1,000,000 encodes, assert 0 heap allocations
}
```

**Test 2: LWW Merge Correctness**
```rust
#[test]
fn lww_merge_order_independent() {
    let mut table = LwwBlockTable::new();
    // Insert same IP with different timestamps in different orders
    table.merge(ip, BlockRecord { blocked_until: 100, updated_at: 10, flags: 1 });
    table.merge(ip, BlockRecord { blocked_until: 200, updated_at: 20, flags: 1 });
    assert_eq!(table.get(ip).unwrap().blocked_until, 200);
    
    let mut table2 = LwwBlockTable::new();
    table2.merge(ip, BlockRecord { blocked_until: 200, updated_at: 20, flags: 1 });
    table2.merge(ip, BlockRecord { blocked_until: 100, updated_at: 10, flags: 1 });
    assert_eq!(table2.get(ip).unwrap().blocked_until, 200);
}
```

**Test 3: HMAC Tamper**
```rust
#[test]
fn hmac_tamper_detection() {
    let mut frame = Frame::new(...);
    frame.ip.octets()[0] ^= 0x01;  // flip 1 bit
    assert!(Frame::decode(&frame.encode()).is_err());
}
```

### 8.2 Multi-Node Loopback Integration Test
```
Setup: 3 local ramshield instances on ports 9441, 9442, 9443

Test:
  1. Block 192.0.2.100 on Node 1 (autonomous)
  2. Verify Node 2 and Node 3 install block in XDP maps ≤ 5ms
  3. Verify no broadcast loop: packet counts match split-horizon expectation
```

### 8.3 Churn & Memory Stability
```
Setup: Single node, simulated attack
Stress: 50,000 unique blocks/minute, 10s TTL, 30 minutes
Pass Criteria:
  1. RSS memory ≤ rambudget limit (no unbounded growth)
  2. Expired blocks purged from userspace + eBPF maps
  3. Mesh frame processing CPU < 5% on modern core
```

---

## 9. Implementation Workstream

| Step | Workstream | Files | Target |
|------|-----------|-------|--------|
| W1 | Wire Protocol | `crates/ramshield-protocol/src/{codec,frame,validate}.rs` | 48-byte frame, HMAC, zero-alloc codec |
| W2 | LWW State Engine | `crates/ramshield-mesh/src/{state,gc}.rs` | Purge AWORSet, LWW map, rambudget |
| W3 | Enforcement + WAL | `crates/ramshield-mesh/src/sync_worker.rs` | Wire to ramwal + ramshield-enforcement |
| W4 | Transport & Gossip | `crates/ramshield-mesh/src/{transport,gossip}.rs` | Peer manager, split-horizon, ramgate |
| W5 | Config & CLI | `crates/ramshield-config`, `crates/ramshield-cli` | TOML, `ramshield-cli mesh`, Prometheus |
| W6 | Benchmarks & Tests | `tests/mesh_loopback.rs`, `benches/codec_bench.rs` | Loopback convergence, memory stability |

---

## 10. Key Design Decisions (ponytail: ceilings)

| Decision | Rationale | Ceiling | Upgrade Path |
|----------|-----------|---------|--------------|
| LWW over AWORSet | No tombstones, bounded memory | No causal dependency tracking | Add HLC-based vector if needed |
| 48-byte fixed frame | Zero-allocation, bounded attack surface | No extensible fields | Version bump + new frame format |
| Split-horizon gossip | Prevents broadcast storms | No multicast/anycast | Add underlay routing if needed |
| HLC timestamps | Causality + wall time, O(1) space | ±5s clock tolerance | NTP sync enforcement if drift > 5s |
| ramgate rate limiting | Protects mesh during attacks | Fixed window, not token bucket | Token bucket for burst tolerance |
| ramwal for persistence | Crash recovery, append-only | No compaction/GC | WAL compaction at scale |

---

## 11. References (Authoritative)

- **RFC 4271** — BGP-4 (peer connectivity model)
- **Kulkarni et al. (2014)** — Hybrid Logical Clocks (causality + wall time, O(1) space)
- **Shapiro et al.** — CRDT theory (confluence, state-based vs op-based)
- **Demers et al. (1987)** — Epidemic algorithms/gossip (push/pull/push-pull, O(log N) convergence)
- **Cassandra architecture** — LWW conflict resolution, hinted handoff
- **CockroachDB / MongoDB / YugabyteDB** — HLC in production distributed databases
- **DSON (delta-state CRDT)** — tombstone-free CRDT alternative (blog.helsing.ai)
- **Rambudget** — RamShield internal memory accounting framework

---

**Bottom line:** RamShield mesh replaces complex AWORSet CRDTs with a minimal TTL-LWW table using HLC timestamps, bounded by rambudget, with split-horizon gossip and ramgate rate limiting — ~400 lines of clean, testable, memory-safe Rust.