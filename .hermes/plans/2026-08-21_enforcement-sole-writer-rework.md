# Enforcement Sole-Writer Rework Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Wire the enforcement command queue so every block/unblock flows through a single `EnforcementService` actor (storage-first, then XDP, then TTL lease). Eliminate direct Store mutations from Detection, Forecasting, and IPC.

**Architecture:** Bounded `mpsc` queue owned by Engine → single consumer `EnforcementService` → Store mutation → XDP apply → TTL lease → metrics. Detection/Forecasting/IPC use `try_send` (no `.await` on hot paths). Startup reconciliation syncs XDP map with authoritative Store state.

**Tech Stack:** Rust 2024, Tokio, mpsc, crossbeam, DashMap, SegQueue, uuid v7/serde, anyhow, thiserror, tracing.

---

## FILES TO CHANGE

| File | Change Type |
|------|-------------|
| `src/engine/mod.rs` | Modify: add queue, spawn EnforcementService |
| `src/enforcement/mod.rs` | Rewrite: sole-writer with TTL leases + startup reconcile |
| `src/detection/mod.rs` | Modify: `try_send EnforceCommand`, direct IpAgg flush |
| `src/forecasting/mod.rs` | Modify: `try_send EnforceCommand` for anomaly blocks |
| `src/ipc/server.rs` | Modify: thread `enforcement_tx` to `handle_connection`, emit commands |
| `src/ipc/mod.rs` | Verify Request types (BlockIp/UnblockIp stay) |
| `src/storage/mod.rs` | Add `ram_limit_bytes()` getter if missing |
| `crates/ramshield-enforcement/src/lib.rs` | Deprecate/align (optional) |

---

## TASK 1: Rewrite src/enforcement/mod.rs — Sole Writer

**Objective:** Single source of truth for block/unblock state. Storage mutation before XDP. TTL expiry re-enters as internal unblock command. Bounded dedup set for idempotency.

**Files:**
- Modify: `src/enforcement/mod.rs` (complete rewrite)

**Step 1: Write failing test** - Test idempotency, block→unblock roundtrip, TTL expiry path, XDP reconcile stub.

**Step 2: Implement EnforcementService**
- Fields: `store: Arc<Store>`, `metrics: Arc<Metrics>`, `xdp: Box<dyn XdpApplier>`, `processed: HashSet<Uuid>`, `order: VecDeque<Uuid>`, `blocked_ips: HashSet<IpAddr>`, `expirations: Vec<(Instant, IpAddr)>`, `shutdown: Arc<AtomicBool>`
- `run()`: startup reconcile → loop: `tokio::select!` on 250ms TTL tick + command_rx + shutdown
- `expire_due()`: drain due expirations, emit internal Unblock `EnforceCommand`, `self.enforce()` (recursive)
- `enforce()`: 
  1. Dedup check (`processed.contains(decision_id)`)
  2. Validate IP not unspecified
  3. Block: load/create record, set `BlockState::Blocked { reason, since_ns }`, `store.insert()` with `None` ttl (we own TTL)
  4. On success: `blocked_ips.insert(ip)`, if `ttl>0` push to `expirations`
  5. XDP `apply_block` (best-effort, warn on fail)
  6. Unblock: `store.get` → set `BlockState::Clean` → `store.insert`
  7. `blocked_ips.remove(&ip)`, XDP `apply_unblock`
  8. `remember_decision(id)` with bounded 65k eviction
  9. Metrics `inc_blocks()` on block
  10. Return `EnforceResult`

**Step 3: Helper fns** - `reason_to_block_reason(str) -> BlockReason`, `epoch_seconds() -> i64`

**Step 4: Run test** - Verify compiles, test passes.

**Step 5: Commit**

---

## TASK 2: Engine — Queue + Spawn EnforcementService

**Objective:** Engine owns the bounded mpsc channel (4096). `boot_pipeline()` spawns the single EnforcementService task, passes `enforcement_tx` to Detection/Forecaster/IPC.

**Files:**
- Modify: `src/engine/mod.rs` lines 17-24, 159-216

**Step 1: Write failing test** - Engine has `enforcement_tx`/`enforcement_rx` fields; `start_async` spawns service.

**Step 2: Engine struct changes**
```rust
pub struct Engine {
    // ...existing
    enforcement_tx: mpsc::Sender<EnforceCommand>,
    enforcement_rx: std::sync::Mutex<Option<mpsc::Receiver<EnforceCommand>>>,
}
```

**Step 3: Engine::new** - Create channel, store sender + wrapped receiver.

**Step 4: boot_pipeline()**
```rust
// Take receiver ONCE
let enforcement_rx = engine.enforcement_rx.lock().unwrap().take()
    .ok_or_else(|| Error::new(ErrorKind::AlreadyExists, "enforcement already started"))?;

// Dedicated shutdown signal for service
let enforcement_shutdown = Arc::new(AtomicBool::new(false));
let service = EnforcementService::new(store.clone(), metrics.clone(), Box::new(StubXdpApplier), enforcement_shutdown);

// Mirror engine shutdown → service shutdown (poll every 100ms)
let engine_shutdown = engine.clone();
tokio::spawn(async move {
    loop {
        if engine_shutdown.is_shutting_down() { enforcement_shutdown.store(true, Release); break; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
});
tokio::spawn(async move { service.run(enforcement_rx).await });

// Pass engine.enforcement_tx.clone() to DetectionEngine, Forecaster, IpcServer::bind
```

**Step 5: Run test** - Verify compiles.

**Step 6: Commit**

---

## TASK 3: Detection — try_send + Direct IpAgg Flush

**Objective:** Detection never `.await` on enforcement send. Uses `try_send` (drops if full, logs warn). Direct aggregate evaluation — no synthetic event reconstruction.

**Files:**
- Modify: `src/detection/mod.rs` (imports, struct, new(), flush_aggregates, subnet_batch_loop)

**Changes:**
1. Imports: add `use crate::enforcement::{EnforceAction, EnforceCommand};` + `use uuid::Uuid;`
2. Struct: `enforcement_tx: mpsc::Sender<EnforceCommand>`
3. new(): accept `enforcement_tx`
4. `flush_aggregates()` (new helper): evaluates `ip_aggs` + `subnet_counts` directly, builds `blocks: Vec<(IpAddr, BlockReason, Option<u64>)>`
5. After metrics: iterate `blocks`, construct `EnforceCommand`, `enforcement_tx.try_send(cmd)`
6. `subnet_batch_loop()`: same pattern — `try_send` per IP in hot subnet
7. Keep `BlockDecision` public for compatibility, but do NOT emit it anywhere.

**Step 1: Write failing test** - Detection flush emits EnforceCommand, no BlockDecision sent.

**Step 2: Apply patches** - surgical diffs per above.

**Step 3: Run test** - Verify compiles.

**Step 4: Commit**

---

## TASK 4: Forecasting — try_send Anomaly Blocks

**Objective:** Forecaster uses enforcement queue for preemptive/entropy blocks. No broadcast sender.

**Files:**
- Modify: `src/forecasting/mod.rs` (struct, new(), preemptive_block(), entropy_block())

**Changes:**
1. Struct: `enforcement_tx: mpsc::Sender<EnforceCommand>`
2. new(): accept `enforcement_tx`
3. `preemptive_block()`: pop from `store.traffic.threat_sample` (SegQueue drain+push-back), for each `threat > 0.7` construct `EnforceCommand { action: Block, reason: "forecast_anomaly" }`, `try_send`
4. `entropy_block()`: same, `reason: "entropy_anomaly"`, top 10% clamped 1..50
5. Remove `block_tx` broadcast entirely.

**Step 1: Write failing test** - Forecaster anomaly produces EnforceCommand.

**Step 2: Apply patches**.

**Step 3: Run test** - Verify compiles.

**Step 4: Commit**

---

## TASK 5: IPC Server — Route Block/Unblock Through Queue

**Objective:** IPC admin requests become EnforceCommand via queue. No direct Store mutation.

**Files:**
- Modify: `src/ipc/server.rs` (IpcServer struct, bind(), handle_connection signature, process_request)

**Changes:**
1. IpcServer: add `enforcement_tx: mpsc::Sender<EnforceCommand>`
2. bind(): accept + store `enforcement_tx`
3. start(): pass `self.enforcement_tx.clone()` to `handle_connection`
4. handle_connection: add `enforcement_tx: &mpsc::Sender<EnforceCommand>` param
5. process_request: add `enforcement_tx` param
6. Request::BlockIp: construct `EnforceCommand { source: "ipc", actor: "admin", action: Block }`, `try_send` → Ok("queued") or 503
7. Request::UnblockIp: construct `EnforceCommand { action: Unblock, reason: "manual_unblock" }`, `try_send`

**Step 1: Write failing test** - IPC block/unblock returns "queued" and command reaches service.

**Step 2: Apply patches**.

**Step 3: Run test** - Verify compiles, integration test `ipc_wiring` passes.

**Step 4: Commit**

---

## TASK 6: Verify Full Build + Tests

**Objective:** Zero warnings, all tests pass.

**Commands:**
```bash
cd /home/m/vehicle_of_rationalism/ramshield/beta/rs
cargo build --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

**Expected:**
- 0 clippy warnings
- 61+ tests pass (unit + integration)
- No "unused" warnings on enforcement types
- Binary builds with `-F full`

**Step 7: Commit** (if green)

---

## RISKS / TRADEOFFS

| Risk | Mitigation |
|------|------------|
| Bounded queue backpressure drops blocks under extreme load | Queue size 4096; `try_send` warns; metrics track drops |
| TTL lease in EnforcementService duplicates Store passive expiry | Enforcement uses `None` ttl on insert; owns expirations fully |
| XDP reconcile on startup needs CAP_BPF | StubXdpApplier works without; real XDP needs caps |
| `uuid::Uuid` alloc on hot path | Acceptable: <1μs, only on block decisions (not per-event) |
| Forecasting `SegQueue` drain pattern (pop+push) retains sample visibility | Correct for lock-free bounded sample; no iter() needed |

---

## OPEN QUESTIONS

1. Should `EnforcementService` own WAL append before Store mutation? (Patched lib has it; current src/enforcement does not). **Decision: skip WAL for now — store is authoritative; add later if needed.**

2. Should `crates/ramshield-enforcement` be the canonical impl and `src/enforcement` re-export? **Decision: keep `src/enforcement` as canonical; crate is for external consumers.**

3. Integration test port collision? **Use unique ports per test (already done in `ipc_wiring.rs`).**

---

## VALIDATION CHECKLIST (Self-Healing Protocol)

- [ ] `cargo build --all-targets` — clean
- [ ] `cargo clippy --all-targets -- -D warnings` — zero lints
- [ ] `cargo test --all-targets` — all pass
- [ ] Manual: no `.unwrap()`/`.expect()` in production code
- [ ] Manual: no unnecessary `.clone()` on hot paths
- [ ] Manual: `try_send` used (no `.await` on enforcement_tx) in Detection/Forecasting/IPC
- [ ] Manual: EnforcementService is ONLY writer of BlockState + XDP