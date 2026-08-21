# CPU-fix remaining scope — ramshield detection hot path

> **For Hermes:** remaining work only. Do not re-do items marked DONE. Do not implement plan items marked REJECT.

**Goal:** finish the in-flight CPU-per-flush cut on branch `cpu-fix` without changing block decisions, dashboard semantics, or prometheus field set.

**Architecture:** Detection already owns a dedicated OS thread (`rs-batch`) and a dedicated subnet thread (`rs-subnet`). Remaining wins are local to `src/detection/mod.rs` (+ one absorb-path consistency fix in `src/detection/batch.rs`). Metrics/dashboard stay as-is.

**Tech stack:** Rust edition 2024 workspace, nightly `rustc 1.100.0-nightly (34baba539 2026-08-16)`, tokio, DashMap, crossbeam_channel. Binary needs `-F full`.

**Branch:** `cpu-fix` @ `20f4727` (enforcement sole-writer). Uncommitted WIP lives in `src/detection/mod.rs` only. Cron-doc diffs are **not** part of this refactor — revert them.

---

## Current state (do not re-apply)

| # | Plan item | Status |
|---|-----------|--------|
| 1 | Drop events-Vec rebuild; flush `IpAgg` directly | DONE (`flush_batch(&aggs, total_events)`, `flush_events` test entry) |
| 2 | Single store lookup via `merge_record` `was_blocked` | DONE |
| 3 | Cheaper Bloom `slots()` (one hasher + rotate) | DONE (slots still computed twice per IP: contains then insert) |
| 4 | Remove `cold_skipped` counters | **REJECT** — dashboard + prometheus + `DashboardSnapshot` consume them |
| 5 | Drop `threat_sample` sort | **REJECT** — forecasting drains sample as-is; order is the top-N contract |
| 6 | Sample enforcement-queue-full warn (every 1024) | DONE |
| 7 | `subnet_batch_loop`: async → blocking OS thread | DONE |
| 8 | Lock-free Bloom (`SegQueue` bit array) | **REJECT** — SegQueue is a queue, not a bitset. Bloom is written only from `rs-batch` (uncontended `RwLock`). YAGNI. |
| 9 | `STATUS_BUCKET` const table | PARTIAL — **BUILD BROKEN** (see P0) |
| 10 | Cache `config.load()` in hot loops | PARTIAL — `flush_batch` loads once; `batch_processor_loop` loads 3× per iter |
| 11 | Derive `total_events` from events.len() | DONE on `flush_events`; `flush_pre_aggs_to_store` still must sum (no events vec) |

**Build status:** `cargo check -F full` **FAILS**:

```
error[E0658]: cannot call conditionally-const method `Range::<u16>::contains` in constant functions
  --> src/detection/mod.rs:103:19
     if (100..600).contains(&code) {
```

Nightly 1.100 does not stabilize `Range::contains` as `const fn`. Table is dead until this is a compare.

**WIP test added:** `flush_preserves_status_dist` — keep. It is the one assert that the reconstruct-events path is gone.

---

## Consequences map (read before any edit)

### KEEP (do not delete)

- `cold_skipped` / `cold_skipped_events` on `BatchRecord`, `Metrics`, prometheus emit, `DashboardSnapshot.cold_skipped`, dashboard `#cold` tile and batch-history column.
  - **If removed:** UI shows 0, operators lose the promotion funnel, prometheus `ramshield_cold_skipped_total` goes silent. Not a CPU win worth the contract break.
- `threat_sample.sort_by` before `truncate(128)`.
  - **If removed:** forecasting `snapshot_threat_sample` / drain gets first-128-above-0.5, not top-128-by-score. Block/anomaly quality drifts. Sort of N≤128 is noise vs DashMap.
- `Arc<RwLock<BloomFilter>>`.
  - **If replaced with SegQueue:** wrong data structure; false-positive semantics collapse. Real lock-free would be `[AtomicU64]` words + `fetch_or`. Not justified: single writer thread.
- Dual metrics trees (`src/metrics/mod.rs` AND `crates/ramshield-metrics`). Prior session already blew a crate-side emit reformat. Do not touch the crate unless a field is actually missing (it is not).

### CHANGE (safe, local)

#### P0 — unbreak const table (`src/detection/mod.rs:102-107`)

Replace `(100..600).contains(&code)` with `code >= 100 && code < 600` inside `const fn status_bucket`.

- **CPU:** none (compile-time).
- **Behavior:** identical.
- **Blast:** this file only. Unblocks `cargo check -F full`.
- **Do not** add `#![feature(const_range)]` / `const_trait_impl`.

#### P1 — reuse Bloom slots in the hot loop (`src/detection/mod.rs` BloomFilter + flush_batch)

Today: `contains(ip)` hashes, then on block `insert(ip)` hashes again.

Add:

```rust
fn contains_hashed(&self, a: usize, b: usize) -> bool { ... }
fn insert_hashed(&mut self, a: usize, b: usize) { ... }
```

Keep public `contains`/`insert` as thin wrappers (tests / future callers). In `flush_batch`:

```rust
let (a, b) = BloomFilter::slots(ip);
let bloom_hit = self.bloom.read().unwrap().contains_hashed(a, b);
// ...
self.bloom.write().unwrap().insert_hashed(a, b);
```

- **CPU:** one hash per IP instead of two on the block path.
- **Behavior:** identical bit positions.
- **Blast:** BloomFilter API grows two methods. No other crate uses BloomFilter (grep: detection/mod.rs only).
- **Lock:** still one read lock per IP + write lock only on block. Do **not** hold the write lock across the loop (would serialize nothing useful and stall contains). Optional micro: compute `bloom_hit` then drop guard before `merge_record` — already the case (`read().unwrap()` is a temporary).

#### P1 — cache config in `batch_processor_loop`

Today three loads per iteration: window/max at start (once, good), then `self.config.load()` inside `pre_aggs_needs_flush_due_to_timeout`, then again at the flush predicate.

Change:

- Load once at top of each loop iteration into `let cfg = self.config.load();`
- Pass `det.pre_aggs_flush_interval_ms` into the timeout helper (or inline the compare).
- Use that `cfg` for `pre_aggs_max_size`.

Keep the **initial** window/max load before the loop (interval is fixed for the process; changing batch_window_ms at runtime is not a supported op — if it becomes one, reload inside the loop).

- **CPU:** two fewer Arc atomic loads per recv-timeout cycle.
- **Behavior:** none. ConfigHandle is an arc-swap; a 1-iter stale read is already the model.
- **Blast:** `pre_aggs_needs_flush_due_to_timeout` signature. Only caller is the loop.

#### P1 — stop throwing away `aggregate()` subnet counts

`flush_events` (test/IPC path):

```rust
let (ip_aggs, _subnet_counts) = aggregate(events);
let aggs: Vec<(IpAddr, IpAgg)> = ip_aggs.into_iter().collect();
self.flush_batch(&aggs, events.len() as u64);
// flush_batch then calls subnet_counts_of(ip_aggs) AGAIN
```

Fix: `flush_batch` takes optional precomputed `&HashMap<u32,u32>` **or** split: `flush_batch` always takes subnet_counts as an arg.

Simplest (YAGNI):

```rust
fn flush_batch(&self, ip_aggs: &[(IpAddr, IpAgg)], subnet_counts: &HashMap<u32, u32>, total_events: u64)
```

- `flush_events`: pass `aggregate()`'s map.
- `flush_pre_aggs_to_store`: keep `subnet_counts_of(&aggs)` — there is no events slice.

- **CPU:** test/IPC path drops a second HashMap fill. Production pre-agg path unchanged.
- **Behavior:** identical counts (same `subnet_key` + `agg.count`).
- **Blast:** two call sites in this file. No external callers of `flush_batch` (it is private). `flush_events` stays public.

#### P2 — `IpAgg::absorb` vs `STATUS_BUCKET` mismatch (document, then align carefully)

`process_event_into_pre_aggs` uses `STATUS_BUCKET` (codes `<100` or `>=600` → skip, bucket 255).

`IpAgg::absorb` (`batch.rs:20`) does `((status_code / 100).saturating_sub(1)).min(4)` — so code `0..=99` lands in **bucket 0**, code `>=600` in **bucket 4**.

- Production ingest uses pre_aggs → STATUS_BUCKET.
- Tests/`flush_events` use `aggregate()` → absorb.

**Consequence of aligning absorb to STATUS_BUCKET:** tests that send `status_code: 0` (none currently assert buckets except `flush_preserves_status_dist` which uses 500) would stop incrementing bucket 0. Safer: make `absorb` call the same `status_bucket` logic (skip invalid). Share the helper — move `status_bucket` + `STATUS_BUCKET` next to `IpAgg` in `batch.rs`, or a one-line duplicate const fn in batch.rs.

**Do this in the same detection agent** so `flush_events` and the live path agree. One assert already covers 5xx.

#### NOT THIS PASS

- `crates/ramshield-metrics` emit reformat.
- `docs/CRON_STATUS.md` / `docs/OPERATOR_LOG.md` (cron noise). Revert, do not commit.
- `edition=2021` leftover on `crates/ramshield-protocol` — out of scope.
- Changing `flush_batch` callers outside detection — there are none.
- Adding a new dependency (xxh3, crossbeam bitset, etc.).
- Feature flags / `#![feature(...)]`.

---

## File blast radius

| File | Action | Why |
|------|--------|-----|
| `src/detection/mod.rs` | Modify | P0 const compare; bloom hashed insert/contains; config cache; flush_batch takes subnet_counts |
| `src/detection/batch.rs` | Modify | `absorb` uses same status-bucket rule as live path |
| `src/metrics/mod.rs` | **No change** | cold_skipped stays |
| `src/engine/mod.rs` | **No change** | snapshot field stays |
| `src/dashboard/static/index.html` | **No change** | `#cold` tile stays |
| `crates/ramshield-metrics/src/lib.rs` | **No change** | do not reformat emit! |
| `docs/CRON_STATUS.md` | `git restore` | not cpu-fix |
| `docs/OPERATOR_LOG.md` | `git restore` | not cpu-fix |
| `.hermes/plans/2026-08-22_cpu-fix-plan.md` | leave | superseded by this file |

---

## Tasks (agent-sized)

### Task A — P0 compile + remaining detection (ONE agent, sequential)

Workdir: `/home/m/vehicle_of_rationalism/ramshield/beta/rs`

1. Fix `status_bucket` to `code >= 100 && code < 600`.
2. `cargo check -F full` — must compile. If not, stop and report the exact rustc error.
3. Bloom `contains_hashed` / `insert_hashed`; use in `flush_batch`.
4. `flush_batch` takes `subnet_counts: &HashMap<u32,u32>`; wire both callers.
5. `batch_processor_loop` loads config once per iteration; timeout helper takes interval.
6. Align `IpAgg::absorb` with STATUS_BUCKET (skip invalid codes). Keep `flush_preserves_status_dist`.
7. `cargo test --lib detection` then `cargo test --all-targets` if lib tests pass.
8. Do **not** commit. Do **not** touch docs or metrics crate.

Verification:

```
cargo check -F full
cargo test --lib detection -- --nocapture
```

Expected: compile clean; existing 3 detection tests + `flush_preserves_status_dist` pass.

### Task B — revert cron-doc noise (ONE agent)

```
git restore docs/CRON_STATUS.md docs/OPERATOR_LOG.md
git status -sb   # must show only src/detection/*.rs (and this plan file untracked)
```

Do not restore `src/detection/mod.rs`.

### Task C — parent verifies after both return

```
cargo build -F full --release
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Nightly may still emit edition/let-chain lints from other files — those are pre-existing, not this refactor. New lints in `detection/mod.rs` / `batch.rs` must be zero.

---

## CPU remaining (honest)

| Change | Est. | Notes |
|--------|------|-------|
| P0 const compare | 0 | compile fix |
| Bloom slots reuse | ~1 hash/IP on block path | contains path already one hash |
| Config cache in loop | 2 atomic Arc loads / window | window is  batch_window_ms, not per-event |
| Reuse aggregate subnet map | test/IPC only | production pre-agg path still `subnet_counts_of` |
| absorb alignment | correctness, not CPU | live path already table-lookup |

Big wins (Vec rebuild, async→thread, warn sampling, merge_record lookup fold) are **already in the working tree**. This pass is the remainder + unbreak the build. Do not invent a lock-free Bloom to chase the original 40–50% claim; that claim mixed real wins with YAGNI.

---

## Risks

- Holding `bloom.write()` across merge_record would stall. Don't.
- Changing `absorb` bucket for `status_code < 100` is a behavior change on the **test/IPC** path only. Production already skips. Aligning is the correct call; if a test fails, fix the test fixture not the skip rule.
- `git restore` on detection would wipe the 70% already done. Task B restores docs only.
- `cargo build -F full --release` last run died on the const-range error; do not treat prior "Finished" logs as current.
