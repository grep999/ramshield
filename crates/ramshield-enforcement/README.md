# ramshield-enforcement

## Problem

The detection and forecasting modules produce decisions: "block this IP" or "unblock this IP." But a decision sitting in memory is useless — it needs to survive crashes, propagate to the kernel's packet filter, and be deduplicated so the same IP isn't blocked twice. The enforcement module is the bridge between a detection decision and actual traffic blocking.

Without this module, a server restart would lose all block state. Without WAL persistence, a crash mid-attack would leave the server wide open.

## How it works

### EnforcementService (actor pattern)

The enforcement layer is a single-writer actor. All block/unblock commands flow through a `tokio::sync::mpsc` channel to one `EnforcementService` that serializes every mutation. This eliminates race conditions between concurrent detection and forecasting decisions.

```
EnforceCommand (from detection or forecasting)
        │
        ▼
EnforcementService::run()  ← main event loop
        │
        ├── enforce(cmd)
        │     ├── Store::update_ip() — set block_state
        │     ├── blocked_set — update reverse index
        │     ├── Wal::append() — persist for crash recovery
        │     └── XdpApplier::apply_block() — kernel BPF map update
        │
        └── expire_due() — TTL ring drain (O(due), not O(all))
```

### TTL ring (bucketed priority queue)

Blocked IPs have optional time-to-live values. The enforcement service maintains a `BTreeMap<u64, Vec<IpAddr>>` where keys are second-bucket deadlines. Expiry drains only past-due buckets — O(due) instead of O(all-blocked). This matters when thousands of IPs are blocked simultaneously.

### Decision deduplication

Each `EnforceCommand` carries a UUID `decision_id`. The service tracks a 65K-entry LRU of seen IDs. Duplicate commands (e.g., from WAL replay after a crash) return `EnforceResult { applied: false }` instead of double-counting.

### WAL crash recovery

On startup, `replay_wal_into_store()` reads all WAL records sequentially:
1. Block records → re-apply the block (skip if TTL expired).
2. Unblock records → remove the block.
3. Return remaining TTL pairs for re-arming the expiry ring.

WAL replay is idempotent — applying the same block twice is caught by dedup.

### XDP integration (optional, feature-gated)

The `XdpApplier` trait abstracts kernel-level blocking:

```rust
#[async_trait]
pub trait XdpApplier {
    async fn apply_block(&mut self, ip: IpAddr, decision_id: Uuid) -> Result<()>;
    async fn apply_unblock(&mut self, ip: IpAddr, decision_id: Uuid) -> Result<()>;
    async fn reconcile(&mut self, expected: &[IpAddr]) -> Result<ReconciliationState>;
}
```

Two implementations:
- `AyaXdpApplier` — loads the BPF ELF from `ramshield-xdp`, attaches to a NIC, writes to `BLOCKED_IPS` / `BLOCKLIST6` hashmaps. IPv4 uses packed u32 keys; IPv6 uses u128 keys.
- `StubXdpApplier` — no-ops everything. Used when XDP is disabled or capabilities are missing.

Design: XDP errors are **non-fatal** (fail-open). If the kernel rejects a map update, the Store-level block is still committed. This prevents XDP permission issues from disabling all blocking.

## Dependencies

```
ramshield-enforcement
  ← ramshield-types    (EnforceCommand, EnforceResult, BlockReason)
  ← ramshield-storage  (Store, Wal)
  ← ramshield-metrics  (Metrics — record_block)
  ← ramshield-xdp      (BPF_ELF — optional, feature-gated)
  ← aya, ahash, tokio, uuid, async-trait, thiserror, anyhow

Receives from: ramshield-detection, ramshield-forecasting
  (via EnforceCommand channel)
Sends to: ramshield-storage (Store mutations)
         ramshield-xdp (kernel BPF map updates)
```

## Key types

```rust
pub struct EnforcementService { ... }  // actor, owns all mutable state
pub trait XdpApplier { ... }           // kernel dataplane abstraction
pub struct AyaXdpApplier { ... }       // real eBPF implementation
pub struct StubXdpApplier { ... }      // no-op fallback
pub struct EnforceResult {
    pub decision_id: Uuid,
    pub committed: bool,     // WAL written
    pub applied: bool,       // Store changed
    pub xdp_applied: bool,   // kernel updated
    pub error: Option<String>,
}
```

## What to read next

- `crates/ramshield-detection/` — produces the EnforceCommands this module consumes
- `crates/ramshield-forecasting/` — also produces EnforceCommands
- `crates/ramshield-xdp/` — the BPF ELF loaded by AyaXdpApplier
- `crates/ramshield-storage/` — the Store and WAL this module writes to
