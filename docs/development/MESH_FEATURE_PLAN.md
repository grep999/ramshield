# RamShield Mesh Feature Plan

**Status:** In development; not part of the production-essential remediation backlog.
**Target branch:** `audit-branch`
**Current baseline when split:** `808f97b8032230146c0012fb928059a0f77e229a`
**Scope:** design, implementation, hardening, and eventual acceptance of the optional mesh feature. The core production backlog is tracked separately in the repository root `BACKLOG.md`.

## Product boundary

Mesh is an optional feature under active development. Mesh-specific feature completeness, convergence, protocol hardening, peer lifecycle, gossip performance, and mesh documentation are tracked here—not as blockers for the core production remediation milestone.

This separation does **not** waive normal safety obligations for any mesh code that is compiled or enabled in a deployment. Do not represent mesh as production-ready until this plan's acceptance gate is met. Keep mesh disabled in production configurations until that decision is explicit.

## Current implementation status

Recent bounded implementation changes already on `audit-branch`:

- [Outbound concurrency limit and deadline](https://github.com/grep999/ramshield/commit/74610c3731b95b6f846f692bc20d82416d62de3b)
- [Bounded anti-entropy chunks](https://github.com/grep999/ramshield/commit/237017711b76eeb76caaedd6dd60ec7767835e26)
- [Creation-time/HLC correction](https://github.com/grep999/ramshield/commit/f449ff752a44570855c91681d98ad5d1074e896c)
- [Inbound timestamp/queue validation](https://github.com/grep999/ramshield/commit/eb3acdf0267b9be12cf455b985cdfae3ab0e900c)
- [Background-task cancellation](https://github.com/grep999/ramshield/commit/0e7995471c06b229c32619e4ce836c198c568acd)
- [Enforcement retry after failed projection](https://github.com/grep999/ramshield/commit/a4c479378c1c5ea382769d0028a2ad83daeeab85)
- [Direct bind argument validation](https://github.com/grep999/ramshield/commit/c823903a14815fa0ebbe3bbd197042a98f7cc91d)
- [Tokio macros feature required for cancellation select](https://github.com/grep999/ramshield/commit/b23928055e1e9651d8b4a428d33da7f84da66556)

These commits are progress, not a feature acceptance decision. The latest recorded workflow on the baseline SHA is [run 38041528476](https://github.com/grep999/ramshield/actions/runs/38041528476); it passed Check, Build, Test, and Clippy. Formatting was not part of that workflow and remains to be verified.

## Mesh backlog

### M0 — Correctness and convergence

- [ ] **M-001: Define the CRDT invariants.** Specify add, remove/unban, re-add, concurrent updates, duplicate and reordered delivery, expiry, restart, and identity/counter behavior. Make the conflict-resolution rules explicit before changing the algorithm.
- [ ] **M-002: Tombstone garbage collection.** Prevent a long-offline peer from resurrecting stale state after tombstone retention expires. Define a safe causal-stability/epoch/repair strategy; extending a timer alone is not sufficient.
- [ ] **M-003: Property and deterministic convergence tests.** Test add/remove/re-add, concurrent nodes, delayed stale deltas, duplicates/reordering, expiry, tombstone GC, restart, and later legitimate re-bans.
- [ ] **M-004: Enforcement projection convergence.** Test remote block/unblock, local manual unblock, IP versus CIDR distinction, expiry, and XDP/store failure. Prove failed projections remain observable and retry/reconciliation eventually restores the intended state.

### M1 — Anti-entropy completeness

- [x] **M-010: Bound chunk size and wire frames.** Initial chunking and maximum-frame regression coverage committed.
- [ ] **M-011: Fair pagination beyond 4,096 entries.** Ensure every live entry and tombstone is eventually visited, rather than repeatedly sampling an arbitrary prefix of concurrent maps.
- [ ] **M-012: End-to-end convergence under large state.** Test multiple pages/chunks, repeated sync, loss/reconnect, tombstones, state exceeding a page and the configured maximum, and eventual convergence.
- [ ] **M-013: Frame-boundary tests.** Exact maximum, one byte over, delimiter split across reads, clean EOF, partial EOF, malformed JSON, and oversized input must have deterministic expected outcomes.

### M2 — Trust and overload behavior

- [x] **M-020: Basic timestamp/origin/config bounds.** Initial future-time, origin, sync-count, key-length, node-ID, and peer-count validation committed.
- [ ] **M-021: Complete protocol schema validation.** Validate all authenticated fields after deserialization, including node IDs, counter semantics, IP/tier/expiry values, sync vectors, and self-origin.
- [ ] **M-022: Authentication and replay tests.** Cover invalid HMAC, malformed envelopes, duplicates/replay, clock skew, self-origin, and valid authenticated sync.
- [ ] **M-023: Queue overload policy.** Replace undocumented drop-oldest behavior for authenticated Block/Unblock deltas with an explicit loss/recovery strategy. Prove eventual convergence and observable overload under saturation.
- [ ] **M-024: Outbound saturation behavior.** The initial 64-permit cap, 3-second send deadline, and stalled-send regression exist. Test saturation, permit release, unreachable peers, and whether local enforcement can be delayed by queued outbound work.

### M3 — Lifecycle and optional integration

- [x] **M-030: Cancellation propagation.** Initial cancellation is wired through listener, readers, anti-entropy, outbound sends, and service exit.
- [ ] **M-031: Join and failure paths.** Test task joining, startup errors, idle/partial-frame/blocked-peer shutdown, and absence of leaked background tasks.
- [ ] **M-032: Optional-feature boundary.** Prove mesh can be built/tested independently and that feature-off core builds/tests remain mesh-free; feature-on startup errors must propagate cleanly.
- [ ] **M-033: Service-level integration.** Exercise mesh-enabled block/unblock behavior, shutdown, XDP/store failures, and reconciliation as a complete service.
- [ ] **M-034: Supported configuration matrix.** Document the supported mesh feature combinations and test them without feature unification masking a broken configuration.

### M4 — Operational and security documentation

- [ ] **M-040: Threat model and deployment guide.** Document key provisioning and rotation, peer identity, network exposure, clock assumptions, replay policy, trust boundaries, and failure/overload semantics.
- [ ] **M-041: Lifecycle and incident runbook.** Document enabling/disabling mesh, startup failure diagnosis, peer health, queue saturation, reconnection, state repair, and safe rollback.
- [ ] **M-042: Mesh-specific module documentation.** Add/update the mesh README and ensure claims match tested behavior.
- [ ] **M-043: Performance baseline.** Measure merge/gossip throughput, convergence latency, memory under load, and behavior with slow or unreachable peers. Establish regression budgets from reproducible measurements.

## Execution order

1. Define CRDT invariants and solve tombstone-GC safety (M-001–M-003).
2. Finish fair pagination and prove large-state convergence (M-011–M-013).
3. Complete input trust, replay, and overload policy (M-021–M-024).
4. Finish lifecycle and service-level integration (M-031–M-034).
5. Complete threat-model docs and performance baseline (M-040–M-043).
6. Run all mesh acceptance tests and independent review on one exact SHA.

## Mesh acceptance gate

Mesh can be declared production-ready only when all of the following hold on the same exact candidate SHA:

- CRDT convergence and tombstone-GC invariants are documented and tested, including long-offline peers and subsequent legitimate bans.
- Anti-entropy reaches all entries/tombstones across multiple pages, loss, reconnect, and state larger than one page.
- Frame limits, malformed input, authentication, replay, timestamp skew, queue overload, and saturation have deterministic regression tests.
- Enforcement projection failures are visible and recover/reconcile correctly.
- Startup, cancellation, shutdown, task joining, and failure paths are tested end to end.
- Independent mesh-only and intended integrated feature configurations pass, along with formatting, strict Clippy, and relevant workspace gates.
- Threat model, deployment guide, operational runbook, and known limitations are reviewed.
- An independent reviewer approves the exact candidate SHA; unresolved correctness/security blockers prevent release.

## Working rules

Use bounded changes with regression tests. Record the base SHA, resulting SHA, changed paths, commands and exit statuses, workflow links, unresolved risks, and next step for each pass. Work only on `audit-branch`; do not merge, force-push, rebase, or modify `kiddo_fix`, PR #3, or PR #5 as part of this work.
