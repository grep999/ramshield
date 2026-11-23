# Mesh production readiness

This document is the mesh boundary. It supersedes the old MESH working
roadmap where the code differs. Status is grounded in the current tree at the
commit listed below, not in a branch snapshot.

## Current status

The mesh crate is implemented, feature-gated, and exercised end to end over
loopback TCP. It is not enabled by default.

Implemented on the current tree:

- `ramshield-mesh`: bounded TCP framing, per-peer HMAC authentication,
  authenticated application ACK, condition-driven anti-entropy, CRDT Aworset
  with tombstones, snapshot export/restore, chunked sync, and receiver-confirmed
  recovery frames.
- `ramshield-enforcement`: mesh receive loop, durable WAL append + CRDT merge
  before ACK, per-peer WAL cursor, XDP projection, stale-repair loop, and
  recovery-proof handling.
- `src/engine/boot.rs`: mesh startup, snapshot CRDT seed, WAL replay into CRDT,
  and fail-closed behavior when mesh is enabled and history is pruned without
  CRDT provenance.
- `ramshield-config`: `[mesh]` validation for unique node IDs, peer IDs, and
  per-peer keys; debug redaction for local and peer keys.
- Tests: mesh crate unit + acceptance tests, including real TCP, forged HMAC,
  negative application ACK, WAL cursor, recovery proof, tombstone GC, and
  convergence cases. The multi-process fault-injection suite covers crash/ACK,
  receiver-confirmed recovery, and duplicate/reorder convergence.

Current implementation is a production candidate, not a production rollout.
The remaining gaps are operational evidence and explicit owner decisions, not
missing CRDT mechanics.

## Boundaries

B1. **Feature boundary.** Mesh stays off unless `mesh.enabled = true`. The
default build and default config remain unchanged.

B2. **Identity boundary.** Every peer has a distinct HMAC key. A frame is
accepted only when the envelope node ID matches the configured peer key. A peer
cannot authenticate as the local node or as another peer. The current
implementation does not add pairwise HMAC, asymmetric signatures, or per-dot
origin signatures. Changing that is an owner decision and is out of scope here.

B3. **Origin trust boundary.** Per-peer HMAC authenticates the sending node, not
the originating node inside a WAL batch. A trusted but compromised peer can
claim another origin node ID. This is bounded by trusted-peer membership and by
WAL replay/idempotent merge; it is not prevented without an owner-approved
per-origin cryptographic change. Mesh peers are trusted infrastructure, not
untrusted internet clients.

B4. **Durability boundary.** A receiver ACK means the message was applied
locally before the ACK was sent. The enforcement path appends the WAL event and
merges the CRDT before acknowledging. WAL replication cursors advance only on
an authenticated positive ACK whose LSN matches the sent page boundary. A
recovery cursor advances only after a receiver proof for the generation,
boundary, digest, and frame count.

B5. **Resource boundary.** Frames are bounded to 64 KiB, peers to 256,
incoming queue to 1024, writer permits to 16, sync tasks to 8, application ACK
wait to 2s, read timeout to 5s, peer write timeout to 3s, and clock skew to
30s. Config validation rejects public/unspecified binds when credentials are
absent.

B6. **Recovery boundary.** If mesh is enabled and WAL history has been pruned
without a CRDT checkpoint seed, startup fails closed. A stale XDP projection
does not lose the durable mesh event; the reconciler retries it.

B7. **Exposure boundary.** Bind mesh to a private, authenticated network.
Public internet exposure is out of scope. BGP/FlowSpec/RTBH is out of scope for
this document and is handled separately.

B8. **Rollout boundary.** No default enablement and no fleet rollout from this
document. Rollout is staged: two-node proof, three-node fault injection,
single-node canary, then fleet. Each stage has an explicit pass/fail.

## Definition of done

A mesh change is production-ready only when all of these are true:

1. Exact-SHA gates pass:
   - `cargo fmt --all --check`
   - `cargo check --workspace --all-targets`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace --lib --bins --tests`
   - `cargo test -p ramshield-mesh --all-targets`
   - `cargo test --workspace --lib --bins --tests --features mesh`
   - `cargo audit`
   - `cargo deny check`
2. Mesh acceptance tests pass, including:
   - real loopback TCP delivery;
   - forged/mismatched HMAC never enters the receive queue;
   - negative application ACK does not advance the WAL cursor;
   - recovery frame loss/chunk boundary is rejected or retried;
   - receiver recovery proof gates cursor advancement;
   - tombstone GC requires peer acknowledgment epochs;
   - duplicate, reordered, and truncated sync chunks converge.
3. Fault-injection proof passes: start two or three real node processes, kill one
   mid-burst, restart it, and verify convergence and no cursor regression. The
   transport-level two-process crash/ACK proof and the multi-process recovery-
   gap proof are committed. The recovery proof requires a receiver proof bound
   to the requested boundary; a wrong boundary does not release the cursor gate.
   The enforcement-level WAL-cursor proof is committed as a loopback integration
   test; it acknowledges a page whose reported LSN does not match the sent
   boundary and proves the cursor is retained until a matching ACK arrives. The
   negative control fails.
4. Operational proof passes:
   - config has complete `[mesh]` fields and no logged key material;
   - mesh peer connectivity is visible in metrics/logs;
   - recovery proof and cursor advancement are observable;
   - fail-closed startup is demonstrated with pruned history and no CRDT seed.
5. Owner sign-off exists for any change to pairwise HMAC, per-origin signatures,
   asymmetric keys, production default, or BGP coupling.

## Work plan

### Phase M1 - close the implementation gate

- Keep the current per-peer HMAC and WAL durability contract.
- Add the key-redaction fix for public mesh peer debug output.
- Run the exact-SHA gate list above.
- Record exact SHA, command, exit code, and test counts.

Exit: all automated gates pass and the mesh crate plus mesh feature tests are
green.

### Phase M2 - multi-process fault injection (complete)

- DONE: `mesh_fault_injection.rs` starts a real child mesh node, kills it after
  a WAL batch is applied but before its ACK, restarts it from a persisted
  snapshot, and verifies the retry converges without cursor regression.
  Negative control (ACK before persistence) fails the test.
- DONE: `wal_cursor_retains_page_when_ack_lsn_does_not_match` exercises the
  enforcement WAL cursor with a mismatched ACK LSN, proves the cursor stays
  before the page, and then advances it on the matching retry. The negative
  control (advance on any ACK) fails this test.
- DONE: `recovery_gap_requires_receiver_proof_before_cursor_release` starts a
  real child mesh process, requests a full recovery at a fixed boundary, and
  proves the receiver proof is required before the cursor gate is released.
  A wrong boundary does not release the gate.
- DONE: `duplicate_and_reordered_batches_converge_without_new_ack_lsn` starts a
  real child mesh process, resends an identical WAL batch, sends a higher LSN
  before a lower one, and proves ACKs repeat the original LSN while snapshot
  block counts stay at one.

Exit: all remaining bullets pass from a clean checkout.

### Phase M3 - operational hardening

- Confirm metrics/logging expose peer send outcomes, queue overflow, recovery
  pending, recovery proof mismatch, WAL ACK mismatch, and XDP projection repair.
- Confirm startup rejects a public bind without credentials and fails closed on
  pruned mesh history.
- Run a two-node canary with real WAL and XDP projection; observe one ban
  propagate and one restart/recovery cycle.

Exit: canary runbook records pass/fail and no silent loss or double-apply.

### Phase M4 - rollout

- Stage: two-node -> three-node -> single-node canary -> fleet.
- Roll back by disabling `mesh.enabled`; no mesh data migration is required.
- Do not enable by default or change BGP in this phase.

Exit: owner-approved rollout with rollback evidence.

## Non-goals for this task

- Pairwise HMAC migration or asymmetric signatures.
- Per-dot origin signatures or a new WAL schema.
- Production default enablement.
- BGP FlowSpec/RTBH implementation or upstream coupling.
- A full fleet rollout.

## Exact-SHA evidence

Recorded on branch `chore/untracked-cleanup` at
`30f34655df72dc7e7f3fade018718bdffbcc4d4d`, with the mesh production and
fault-injection changes applied in the worktree (no commit):

| Command | Exit | Result |
| --- | --- | --- |
| `cargo fmt --all --check` | 0 | PASS |
| `cargo check --workspace --all-targets --locked` | 0 | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | PASS |
| `cargo test --workspace --lib --bins --tests --locked` | 0 | PASS, 772 passed / 15 ignored |
| `cargo test -p ramshield-mesh --all-targets --locked` | 0 | PASS, 81 passed |
| `cargo test --workspace --lib --bins --tests --features mesh --locked` | 0 | PASS, 779 passed / 15 ignored |
| `cargo audit` | 0 | PASS, 1 allowed `rustix` warning |
| `cargo deny check` | 0 | PASS |

`--lib --bins --tests` is deliberate: `check` and `clippy` compile all targets,
while `cargo test --all-targets` also executes the standalone `hot_paths`
benchmark binary, which is a benchmark, not a test. Benchmark compilation is
already covered by the all-targets check and clippy gates.

Code-gate and dependency-audit logs for this worktree:
`/home/m/.hermes/profiles/ramshield/cache/scratch/mesh-gates-final/`.