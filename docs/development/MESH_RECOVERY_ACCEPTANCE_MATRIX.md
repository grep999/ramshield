# Mesh Recovery Acceptance Matrix

**Status:** active work ledger for PR #19; acceptance is not yet complete.
**Target branch:** `prototype/condition-driven-mesh-sync-20261011`
**Baseline observed before this document commit:** `9f24516c61f40df79d25536988b67251dfcba933`
**Safety:** keep PR #19 Draft; keep production mesh disabled. No merge, deploy, force-push, or rebase.

This document is the durable handoff contract for the recovery-protocol coder, test coder, coordinator, and independent reviewer. It records required behavior; it does not claim that unimplemented behavior or unrun tests have passed.

## Fixed design decisions

- WAL is durable mutation history. Append and required sync precede publishing a local mutation.
- CRDT state, including origin dots and tombstones, is the convergence model.
- WAL LSNs are local to each sender/source WAL. They are not a global sequence.
- While retained history is available, catch up from the peer's acknowledged source-WAL cursor.
- On a retention gap, capture one immutable deterministic snapshot and its source-WAL boundary under a new recovery generation. Send that fixed snapshot in bounded frames.
- **Live-map cursor/keyset pagination is dropped. Do not reintroduce it.**
- Individual frame ACKs prove only acceptance of those frames. They do not prove whole-recovery completion.
- Keep the peer WAL cursor pinned and WAL replay gated until a receiver-confirmed completion proof for the same generation, captured boundary, and canonical snapshot digest has been accepted. Then resume at boundary + 1.
- Keep the existing periodic repair safety net. No blockchain or global ordering layer is needed.

## Protocol contract to implement

1. Each recovery attempt has an unambiguous generation/session identity, source-WAL boundary, canonical snapshot digest, and fixed frame count.
2. Every frame carries enough authenticated context to reject a frame belonging to another generation/boundary/digest. Frame identity/order and duplicate semantics must be explicit.
3. The receiver records successful frame application, not merely socket receipt. It may issue whole-recovery completion only after it has accepted the complete frame set for one consistent recovery identity and independently verifies the canonical digest of the resulting snapshot.
4. Canonical digest input must be specified and deterministic: stable ordering, normalized fields, and both live entries and tombstones. Never hash map iteration order.
5. Completion must be authenticated and idempotent. Exact duplicate completion is safe; conflicting duplicate, stale generation, wrong boundary, wrong digest, incomplete frame set, malformed proof, and replay from an obsolete session must be rejected.
6. Sender accepts only a completion proof matching the active generation, boundary, and digest. Frame ACKs, TCP writes, and local transfer markers cannot release the cursor.
7. On timeout, disconnect, partial transfer, invalid proof, or restart with missing receiver recovery state, fail closed: preserve the pending recovery and cursor pin; safely retry/restart the fixed recovery transaction rather than skipping history.
8. Mutations after snapshot capture belong to the WAL tail. After completion, replay from boundary + 1 and prove those mutations are not lost.
9. Document the actual wire/API contract in the protocol coder's handoff before the test coder binds tests to field names. Do not invent API names to make a test compile.

## Acceptance matrix

| Case | Required observable result |
|---|---|
| All frames ACKed, no completion proof | Recovery remains pending; WAL cursor stays pinned |
| Complete valid frame set and matching proof | Recovery clears only for matching generation/boundary/digest; replay resumes at boundary + 1 |
| Missing, duplicate, reordered, or delayed frame | No premature completion; duplicates are idempotent or explicitly rejected |
| Incomplete frame set or digest mismatch | Completion rejected; cursor remains pinned |
| Stale generation, wrong boundary, wrong digest, conflicting proof | Rejected; current recovery state is not overwritten |
| Lost completion response / duplicate completion request | Safe retry; no duplicate side effect or cursor jump |
| Empty snapshot | Valid deterministic digest and completion path; no special-case cursor skip |
| Mutation after capture | Fixed snapshot remains unchanged; WAL tail applies after boundary |
| Disconnect/reconnect or process restart mid-transfer | Partial state cannot masquerade as complete; retry/recovery converges |
| Tombstones, unblock/re-ban, concurrent origins | No resurrection or deletion of a concurrent origin's live dot |
| Multi-frame and large state | Bounded frames; eventual CRDT and Store/XDP projection convergence |
| Queue pressure, slow/unreachable peer, shutdown | Bounded resources; retry/repair visible; no hot loop or leaked task |

At minimum, a deterministic two-process fault-injection harness must exercise frame loss/duplication/reordering, completion loss/replay, reconnect/restart, concurrent mutation, and convergence digests for CRDT blocks + tombstones and the effective Store/XDP projection. A single-node unit test or green compile is not a substitute.

## File ownership and handoff sequence

1. **Protocol coder** owns only `crates/ramshield-mesh/src/transport.rs` and `crates/ramshield-enforcement/src/service/mesh.rs`.
2. **Test coder** owns `crates/ramshield-mesh/tests/mesh_acceptance.rs`, `scripts/test-mesh-recovery-sandbox.sh`, and test-only support. Do not overlap production protocol files.
3. **Coordinator** tracks exact SHA, handoffs, and gates; does not patch either coder's files while work is in flight.
4. **Independent reviewer** is read-only and reviews the exact same candidate SHA named by both coder handoffs. A SHA mismatch or missing handoff means BLOCKED, not approval.
5. Each handoff is a durable PR #19 comment containing role, exact old/new SHA, changed files, contract/invariants, commands actually run and outcomes, known gaps, and one next owner/action.
6. Commit bounded, reviewable changes with truthful messages. Before each write, fetch current branch head and target blob SHA. Do not overwrite concurrent changes.
7. Run the focused sandbox and relevant Rust tests where Cargo is available; record actual exit status. Then verify CI, Review Pipeline, and Backlog Gate on the exact candidate SHA. Missing, queued, skipped, cancelled, inaccessible, or stale-SHA results are not PASS.
8. Reviewer reports `APPROVE`, `REQUEST CHANGES`, or `BLOCKED` with file/line evidence and exact-SHA check evidence. Do not self-approve.

## Current known state at baseline

- The prototype already uses a fixed deterministic snapshot protected during capture, bounded frames with application ACKs, and deliberately pins the WAL cursor while recovery completion is pending.
- The explicit receiver-confirmed generation/boundary/digest completion handshake remains the top protocol blocker.
- Multi-process fault injection, memory qualification for full-snapshot cloning, exact-SHA required gates, and independent review remain open.
- The environment used for the previous coordination pass could not run Cargo locally. No Rust test result is claimed here.
- This matrix is a committed execution contract, not evidence that any acceptance case passed.

## Per-commit record template

Append a short PR #19 handoff for every meaningful change with:

- **Role / result:** implementation, tests, docs, or independent review.
- **Base SHA → resulting SHA:** full immutable hashes.
- **Changed paths:** only paths actually changed.
- **Checks:** exact commands and observed exit statuses; link exact-SHA workflow runs/jobs.
- **Acceptance rows covered:** refer to the matrix above.
- **Open risks:** what remains unproved.
- **Next owner/action:** one bounded task.


## Candidate record — frozen integration candidate `3308049` (on base `3dfc80d`)

**Role / result:** implementation + regression tests + one independent read-only review round with findings (triaged in place, see "Independent review findings"). Two further read-only review rounds were dispatched against this final tree; both stalled in file-by-file exploration and were stopped without producing a written verdict, so **no second independent verdict exists for this tree** — that gap is recorded below rather than papered over.
**Base SHA → resulting SHA:** `3dfc80df79e748d3b13690a42bcbf350ed2dc75e` → `33080496e0c5c73dcf406aee5bbe9708b4d8dcb8` (tree `cefc02b84f632a57be2f491f5bbcfa395b2ec6bb`, byte-identical to the verified working tree — the tree hash was captured before the commit and compared after it). The patch was frozen into a single commit on `integration/fixfixfix-sync` after explicit authorization; no merge, rebase, force-push, or history rewrite was performed, and **nothing was pushed**. `refs/backup/pre-merge` still points at `1ebfc2ca0095d25c5d782bf0d577d62f10574b0b`.
**Changed paths (14 tracked files):** mesh scope — `crates/ramshield-mesh/src/aworset.rs`, `crates/ramshield-mesh/src/transport.rs`, `crates/ramshield-mesh/tests/mesh_acceptance.rs`, `crates/ramshield-mesh/tests/frame_boundary_tests.rs` (formatting only); enforcement scope — `crates/ramshield-enforcement/src/service/enforce.rs`, `crates/ramshield-enforcement/src/service/mesh.rs`, `crates/ramshield-enforcement/src/tests/mod.rs`; separate XDP scope — `crates/ramshield-enforcement/src/xdp.rs`; storage scope — `crates/ramshield-storage/src/wal.rs`; tooling — `crates/ramshield-protocol/benches/protocol_bench.rs`, `scripts/test-mesh-recovery-sandbox.sh`; separate legacy-root scope — `src/ipc/server/handlers.rs`, `src/engine/native.rs` (dead-code removal only, see below); documentation — this file. All 11 pre-existing untracked files (including both `.bak` files) are untouched, and all 24 stashes are intact.

**Checks — exact commands and observed exit status (2026-10-20, frozen candidate `3308049`):**

| Command | Exit | Observed result |
|---|---|---|
| `cargo test -p ramshield-mesh --locked` | 0 | 71 lib + 4 acceptance + 2 doc tests passed |
| `cargo test -p ramshield-mesh --locked --test mesh_acceptance` (×20 loop) | 0 | 20/20 passed |
| `cargo test -p ramshield-enforcement --locked --all-features` | 0 | 56 passed (55 before the WAL-durability regression was added) |
| `cargo test -p ramshield-storage --locked` | 0 | 39 + 1 + 1 passed |
| `cargo check --workspace --locked --all-targets --features mesh --exclude ramshield` | 0 | passed (diagnostic isolation only, **not** the required gate) |
| `cargo clippy --workspace --locked --all-targets --features mesh --exclude ramshield -- -D warnings` | 0 | passed (diagnostic isolation only) |
| `cargo check --workspace --locked --all-targets --features mesh` | **0** | **passes** — the stale legacy root package was repaired in this working tree (see "Legacy root package repair" below) |
| `cargo clippy --workspace --locked --all-targets -- -D warnings` | **0** | **passes** (exact runbook gate) |
| `cargo check --workspace --locked --all-targets` (CI core-only) | 0 | passes |
| `cargo check --workspace --locked --all-targets --all-features` (CI all features) | 0 | passes |
| `cargo clippy --workspace --locked --features full --all-targets -- -D warnings` (CI full) | 0 | passes |
| `cargo clippy --workspace --locked --all-features --all-targets -- -D warnings` | 0 | passes |
| `cargo test --workspace --locked` (CI core-only) | 0 | passes — includes the root package suites (89 lib + 2/3/5/3/29 integration) that previously could not even build |
| `cargo test --workspace --locked --features full` (CI full) | 0 | passes |
| `cargo test --workspace --locked --all-features` (CI all features) | 0 | passes |
| `cargo test -p ramshield` (root package, previously unbuildable) | 0 | 89 passed / 1 ignored + integration suites green |
| `python3 .github/scripts/ci_filter_testcode.py . </dev/null` | 0 | passed — this script is a stdin filter, not an independent build or test gate |
| `cargo fmt --all -- --check` | **0** | **passes** — the historical drift was confined to three mesh-scope files (`aworset.rs`, `transport.rs`, `frame_boundary_tests.rs`) and was cleared with `cargo fmt --all`; no unrelated file was reformatted |
| `cargo test -p ramshield-enforcement --locked --all-features mesh_source_label_does_not_suppress_wal_durability` | 0 | 1 passed (new regression; mutation-tested — restoring the string-based WAL suppression fails it) |
| CI-equivalent ladder (core / mesh / all-features check + `full` / all-features Clippy + core / full / all-features workspace test + mesh crate test + CI filter + format check) | 0 | 12/12 gates `OK`, `fmt_exit=0 diff_hunks=0` (`/tmp/ladder3.sh`, log `/tmp/ladder3_run4.log`) |
| Working-tree recovery sandbox equivalent (captured-boundary full-sync, TCP mesh acceptance, enforcement-with-mesh) | 0 | `PASS: all minimal mesh-recovery sandbox gates passed (working-tree source)` — re-run after the WAL-durability change |
| `scripts/test-mesh-recovery-sandbox.sh` | **0** | **PASS: all minimal mesh-recovery sandbox gates passed.** Run after the freeze, so the script archived the committed candidate `3308049` (all three gates: captured-boundary full-sync, TCP mesh acceptance, enforcement-with-mesh). This closes the previously recorded gap that the script could only ever archive the non-compiling `3dfc80d`. |
| `cargo test -p ramshield-mesh --locked --lib gc_tombstones_respects_causal_safety` (×30, and ×3 with `--all-features`) | 0 | 33/33 passed — a failure reported by one late background run (`left: 0, right: 2`, `aworset.rs:1364`) does not reproduce on this tree; it was a pre-fix snapshot. |
| working-tree recovery sandbox (same three gates, candidate tree instead of `HEAD`) | 0 | **PASS** — captured-boundary full-sync regression, TCP mesh acceptance, and enforcement-with-mesh (44 tests) all green in a disposable copy of the candidate tree |
| `git diff --exit-code 3dfc80d -- crates/ramshield-cgnat/include/ramshield_shm.h scripts/shm_abi_stress.c` | 0 | SHM ABI surface untouched by this patch |
| `scripts/verify_shm_header.sh` (no TSAN) | 0 | "SHM C ABI and concurrent reader/writer stress: PASS" |
| `RAMSHIELD_SHM_TSAN=1 scripts/verify_shm_header.sh` | 139 | **pre-existing, environment-specific**: the ThreadSanitizer phase segfaults on this host. Reproduced identically from a pristine detached `HEAD` worktree; the plain ABI stress passes and the SHM sources are unchanged by this patch. Not a mesh finding. |

**Acceptance rows covered by this patch:**

- *Frame ACK is not completion proof* → `transport::frame_tests::full_sync_recovery_requires_receiver_digest_proof_before_cursor_release`; the sender cursor is released only after a proof matching generation, boundary LSN, and canonical digest is verified.
- *Completion must be authenticated and idempotent; stale generation / wrong boundary / wrong digest rejected* → `recovery_completion_mac_is_stable_across_multiple_tombstone_acks` (ordered `Vec<TombstoneAck>` replaced the order-unstable, integer-key-unparseable `HashMap`; the MAC-covered bytes are now identical on both sides and tamper detection is asserted).
- *Deterministic canonical snapshot digest* → ordered tombstone-ack list plus the existing canonical snapshot ordering; `recovery_completion_tombstone_acks_are_canonically_ordered` pins that the MAC-covered acknowledgment list is ordered by peer node id regardless of map iteration order (mutation-tested: removing the ordering fails the test).
- *No resurrection / concurrent origin dots survive* → `aworset::tests::tombstone_gc_waits_for_every_acknowledging_peer`, `tombstone_gc_prevents_resurrection_after_acknowledgment`, `tombstone_gc_collects_locally_removed_dots_once_peers_advance`; a removal is now collectable only after every declared peer has acknowledged **since the removal existed** (ack epochs are never compared against dot counters).
- *Peer identity and per-peer authentication* → `transport::frame_tests::application_ack_is_bound_to_the_intended_peer` (an ACK is accepted only when the claimed node id is the intended peer and the MAC verifies under that peer's own configured key), `bind_rejects_duplicate_peer_identity_and_reused_keys`, `bind_rejects_invalid_node_key_and_peer_configuration`.
- *Mutations after capture are not lost* → `enforcement::tests::seeded_replay_matches_full_replay` (snapshot-prefix + WAL-tail replay equals full replay); fault injection confirmed the test fails if the tail unblock is skipped or the phantom-block reconciliation is removed.
- *Bounded resources / no vacuous gates* → `scripts/test-mesh-recovery-sandbox.sh` now uses `run_named_test`, which fails when a named Cargo filter matches no test.

**Independent review findings (round 1, read-only, no files modified):**

- *Claimed MAJOR — outbound ACK not bound to the claimed sender*: **not reproducible**. `send_to_ack` resolves the intended peer from the configured peer list by address (`peer_by_address`) and verifies the MAC with that peer's configured key; the claimed `node_id` must equal it. A forged ACK using another peer's key is rejected. Pinned by `application_ack_is_bound_to_the_intended_peer`, which was mutation-tested against exactly this weaker behavior and failed as expected.
- *Tombstone GC can be blocked indefinitely → unbounded tombstone growth*: **partially addressed**. Collection now requires an acknowledgment that happened after the removal existed, and every hold-back is logged (`tracing::warn!`). The residual risk is inherent to the current wire format, which carries an acknowledgment epoch rather than a per-removal watermark; a per-removal WAL watermark is the strictly stronger future fix. An empty declared peer set now skips collection entirely (conservative) instead of disabling the protection.
- *Empty/transient peer list silently disables the protection*: **addressed** — an empty declared set now retains tombstones and logs a warning.
- *Acknowledgement LSN only advanced for `WalBatch`*: **accepted limitation**. The sender's per-peer cursor is driven by the WAL-batch path; no caller consumes the ACK LSN for single `Block`/`Unblock` messages, so the gap is reporting-only and no behavior change was made.

**Re-triage of the round-1 findings against this tree (2026-10-20).**

The round-1 report was written against an earlier snapshot of this diff. Every finding was re-checked line by line against the current tree:

- *Outbound ACK not bound to the claimed sender* — **not reproducible** (see above). The report's proposed remedy, selecting the verification key from the untrusted wire claim `ack.node_id`, is *weaker* than the shipped design, which resolves the intended peer by address, uses that peer's configured key, and requires the claimed node id to equal it. It must not be applied.
- *An ACK captured in one recovery round could advance another round's cursor* — **not reproducible**. `complete_recovery` compares the received generation, recovery id, boundary LSN, snapshot digest and frame count against the in-flight session and recomputes the canonical digest of the assembled frames before a proof is issued, so a proof from another round cannot release a cursor.
- *Collection compares acknowledgement epochs with CRDT dot counters* — **already fixed in this tree**: `TombstoneState { counter, created_at_epoch }` keeps the dot high-water mark and the acknowledgement watermark in separate domains, and a removal is collectable only once every configured remote peer presents an acknowledgement epoch strictly greater than the watermark captured when the removal was recorded. Mutation-tested both ways.
- *Empty/transient peer list disables the protection* — **already fixed**: an empty declared set retains tombstones and warns. `set_expected_peers` is called from `MeshHandle::bind`, the only point at which the peer set is configured, so it cannot go stale independently of the handle.
- *Peer incarnation is locally generated* — **real, accepted as a pre-ship design item**. `PeerEpoch.incarnation` is a locally generated UUID, so a peer restart is not detected and that peer's acknowledgement baseline is not reset. The wire format carries no peer incarnation, so the fix is a protocol change (incarnation inside the authenticated envelope, baseline reset on change). The current behaviour fails safe: a restarted peer cannot advance its acknowledgement epoch, so removals are retained rather than collected early.
- *Acknowledgement epoch advances only on a strictly higher LSN* — **real and deliberately kept**. Advancing the epoch on every acknowledgement would let an acknowledgement for a state that predates the removal satisfy the collection rule, weakening the safety property. The correct fix is the per-removal watermark below.
- *A decision whose `source` is `"mesh"` suppresses its WAL record* — **fixed here.** The suppression was a free-form string comparison, so any decision that merely claimed the `mesh` source would silently never reach the log. It is now an explicit caller contract: `WalDurability::Append` / `WalDurability::AlreadyDurable`, with `enforce_durable_mesh_projection` used by the four mesh sites whose dot is already durable. Regression: `enforcement::tests::mesh_source_label_does_not_suppress_wal_durability` — mutation-tested (restoring the string-based suppression fails it with 0 journalled records; production was restored byte-identically afterwards).
- *`replay.rs` treats `UnblockIp` as origin-agnostic* — **pre-existing and out of scope**: `crates/ramshield-enforcement/src/replay.rs` is unchanged by this patch, and changing the local-unblock WAL shape is a separate change set.
- *`MeshApplyOutcome::Ignored` after a successful persist/merge* — **naming only**: `state_changed` is reported separately from the `applied`/`ignored` counters, and the durable tombstone is merged before the outcome is produced. No behaviour change.
- *Duplicated MAC payload construction* — **nit**, not changed.

**Pre-ship protocol design items (not fixable inside this patch):**

1. *Per-removal acknowledgement watermark.* The wire carries an acknowledgement epoch — an event counter in the sender's WAL LSN space — not the removal's own LSN. The shipped rule proves "every declared remote peer acknowledged something newer than the removal's creation watermark"; it does not prove "every declared remote peer consumed a state that contains this removal". A removal carrying its origin LSN, compared against the peer's acknowledged LSN, is the strictly stronger fix.
2. *Peer incarnation on the wire*, with an acknowledgement-baseline reset when it changes (see above).

### Legacy root package repair (separate scope, non-mesh)

The candidate branch carried a **broken release binary**: the root `ramshield` package (the shipped `target/release/ramshield`, per `docs/WORKSPACE_SCOPE.md`) did not compile, which blocked the exact workspace gates. Two branch-added blocks were the cause, and both were **unreferenced dead code that had never compiled**:

- `src/ipc/server/handlers.rs` — a "T19 implementation" priority dispatcher (`dispatch_request`) appended to the published file, written against a pre-refactor API (`Request::Block(ip)` tuple variants, `EnforceCommand { id, ttl_secs, source_ip, source_port }`, `Store::is_blocked` / `get_threat_score` / `get_ewma_rps`, `Response::IpStatus`). `src/ipc/server.rs` only uses `handlers::is_low_signal` and `handlers::process_request`; `dispatch_request` has no caller.
- `src/engine/native.rs` — a `parse_packet_safe` helper referencing an `Ipv4Header` type that exists nowhere in the tree; it has no caller either.

Repair: both files were restored to the published, CI-verified `origin/master` content (`git show origin/master:<path> > <path>`; the index was left untouched, no checkout/merge/reset). No protocol, identity, or enforcement behavior was invented. Consequence: the root package compiles and its tests run again, and the exact gates pass.

Provenance and reachability: both blocks were introduced by the same branch commit, `39b9ea1` ("Apply top 10 urgent stability fixes from 1810b.txt audit report"), and neither ever compiled. A whole-tree grep (`.rs`, `.toml`, `.md`, `.sh`, excluding `target/`) finds **no reference to `dispatch_request` or `parse_packet_safe` anywhere except this record** — the IPC server only uses `handlers::is_low_signal` and `handlers::process_request`, which are untouched and still exercised by `tests/queue_under_attack.rs` and `src/ipc/server/connection.rs`. Both blocks remain recoverable from `39b9ea1` if the T19 priority dispatcher is wanted later; it would have to be rewritten against the current struct-variant protocol API as its own change set.

**Open risks (unproved):**

- Only one independent review produced findings. The two follow-up rounds against the final tree (after the tombstone-GC/ACK/canonical-ordering changes and after the legacy-root repair) were stopped while still exploring, so the final tree has no second independent verdict.
- Two protocol-level weaknesses are documented above and are **not** closed by this patch: the collection rule uses an acknowledgement epoch rather than a per-removal watermark, and the peer incarnation is locally generated rather than learned from an authenticated handshake. Both fail safe today (retain rather than collect early) and both need a wire-format change plus a protocol review.

- Recovery fault injection is exercised by the repository sandbox script against the frozen candidate `3308049`: all three gates pass. That script still covers a single-process run only; the two-process fault-injection harness below is still missing.
- `gc_tombstones` has no production caller yet; the GC rule is exercised by unit tests only.
- Multi-process fault injection (frame loss/duplication/reordering, sender and receiver restart) is not yet covered by an automated two-process harness.
- No hosted CI run is linked for this candidate: the frozen commit `3308049` was not pushed, so no workflow result can be attached to the exact SHA. The local CI-equivalent ladder reproduces the workflow's matrix and passes.
- The legacy root package was repaired by *removing* dead code, not by porting the T19 dispatcher to the current API. If the T19 priority dispatcher is still wanted, it needs to be rewritten against the current struct-variant protocol API as its own change set.

**Next owner/action:** the patch is frozen at `33080496e0c5c73dcf406aee5bbe9708b4d8dcb8` on `integration/fixfixfix-sync`, and the repository recovery sandbox has run green against that exact SHA. Remaining, in order: (1) push the branch and attach the hosted CI run for `3308049`; (2) implement the two protocol design items above — the per-removal acknowledgement watermark and the peer incarnation on the wire — as their own change sets with a protocol review; (3) obtain the missing second independent review verdict for the frozen tree.
