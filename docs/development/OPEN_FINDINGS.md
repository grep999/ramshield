# OPEN IMPLEMENTATION WORK — tracked findings

The durable task list for the issues that still need implementation. Every
entry states the current status, the required fix, and the closure criterion,
so a later pass can pick it up without re-deriving the intent.

**Linux only.** RamShield is an eBPF/XDP ingress daemon and targets Linux. The macOS and Windows CI jobs were removed, and with them the F13 finding that the workspace does not compile for Windows - it was a demand for support nobody asked for, not a defect.

**Four numbering namespaces.** The **roadmap's** `F-01..F-08`
(`/home/m/Downloads/ramshield-remediation-roadmap-v1.2.md`) and the **21
October audit's** `F1..F8`
(`/home/m/Downloads/ramshield_master_audit_2026-10-21.md`) are different
lists and must not be confused. This file tracks the **audit's** numbering.
The roadmap's own `F-02` and `F-03` remain parked and unauthorised.
**F9** is neither: it was found later, in this repository, while re-verifying
backlog row I-011, and is tracked here only so it is not lost. **F10** was
found the same way, while re-verifying backlog row I-024.

Each audit finding was independently verified against the source in
`docs/development/MASTER_AUDIT_2026_10_21_REVIEW.md`. The audit itself was
remote-only — its author could not clone the repository and ran no test,
clippy, C SHM, live XDP or crash-injection run — so its severities are
engineering judgments, not test results, and the review's adjusted severities
are the ones used here.

## Status

| ID | Finding | Severity | Status | Owner decision needed |
|---|---|---|---|---|
| F5 | Unauthenticated local control plane (IPC + dashboard), as of `e5caac1` | P1 | **CLOSED** — `b1a9e9e` | no |
| F3 | Native ingest drops invisible | P2 | **CLOSED** — `60ce0f1` | no |
| F4 | XDP LRU eviction residual not stated | P3 (doc) | **CLOSED** — `79f4eac` | no |
| **F1** | **CGNAT shared-memory lookup semantics** | **P1/P2** | **CLOSED** — `d1f4a81`, see `docs/development/CGNAT_SHM_LOOKUP_FIXES.md` | no — the ABI question dissolved (see below) |
| **F1a** | **A /0 rule is not representable in the SHM table** | **P2** | **OPEN** — a design decision for the owner, not a defect | yes — retain the representation, change it, or use a separate mechanism |
| **F2** | **Native capture worker lifecycle** | **P2** | **FIXED** — on `master` as `15e0551` (PR #58): the workers are started where their outcome is known, a failed capture is retried a bounded number of times, and a dead capture is visible in the metrics, the dashboard and the health reason | no — the owner decided: degrade, keep serving |
| F6 | `BACKLOG.md` is not a reliable master ledger | P2 (doc) | CLOSED — the owner lifted the fence and the file was re-pointed at `master` | no — the fence was lifted by the owner on 2026-10-21 |
| F7 | Performance/RSS claims need workload evidence | P2 (process) | OPEN | yes — the release-claims policy |
| **F9** | **Enforcement's block path is a lost update against detection** | **P1** | **FIXED** — `682435a`, merged from PR #43: the store has two block-only mutators under one shard lock, each touching only `block_state`, and the blocked index is updated under that same lock. PR #39 was closed unmerged | no |
| F8 | The audit was not locally reproduced | n/a (method) | not actionable | no |
| **F10** | **The IPC server MACs a key-sorted payload, not the client's bytes** | **P1** | **FIXED by the owner's decision** — the server's rule stands: the MAC covers the frame minus its `auth` object, re-serialized compact and key-sorted; the CLI signs exactly that | yes — the fix shape is a canonicalisation decision, and it changes the wire contract |
| F11 | Crate README documentation drift (62 claims) | P2 (docs) | **FIXED** — all 62 claims are fixed; every crate README the sweep covers is now pinned by a test | no |
| F22 | `ramshield-analytics` declares two dependencies it never uses | P3 (hygiene) | **FIXED** — on `master` as `35f4557` (PR #57, the code commit `f0b84c6`): `ramshield-types` and `ramshield-storage` are gone from `crates/ramshield-analytics/Cargo.toml`; the crate has no workspace dependency at all | no |
| F24 | The removal paths drop the blocked index entry after the shard lock is released | P3 (latent, narrow) | **FIXED** — on `master` as `6474bc4` (PR #56, the code commit `fcf1be4`): `Store::remove` and `evict_batch` now book under the shard lock, through the entry API | no |
| F23 | `forecasting.min_entropy` is a config knob nothing reads | P3 (hygiene) | **FIXED** — the daemon runs the forecaster and the forecaster reads the knob | no |
| F25 | `ramshield-protocol` declares a dependency on `ramshield-storage` it never uses | P3 (hygiene) | **FIXED** — on `master` as `1315236` (PR #63, the code commit `4fbf680`): the dependency is gone from `crates/ramshield-protocol/Cargo.toml`; the crate declares only `ramshield-types` | no |
| F26 | `FAIL_SAFE_BEHAVIOR.md` claims a global fail-closed posture the dataplane does not have | P2 (doc) | **FIXED** — the document states each subsystem's own posture, and `tests/fail_safe_claims.rs` pins it to the source | no |
| F27 | `scripts/prod_smoke.sh` claims checks it does not perform, and it deletes an env-overridable path | P1 (data loss) | **FIXED** — the script does what it claims and deletes only its own directory | no — the cleanup needs a guard, the checks need to exist |
| F28 | The advanced CI workflow runs the benchmarks as tests and masks the benchmark build failure | P2 (CI) | **FIXED** — the benchmark build failure fails the job | no |
| F29 | The Subnet Stress Pin gate asserts against a fixed four-second sleep | P2 (CI) | **FIXED** — the script now waits for the counters to settle | no |
| F30 | `block_ip` trusts the caller to pass a blocked fallback; the only guard is a `debug_assert!` | P3 (hardening) | **FIXED** — a clean fallback is refused and publishes nothing | no |
| F31 | The dashboard's rate readouts read keys the SSE writer never sets | P2 (operator-visible) | **FIXED** — the rate readouts read the keys the stream writes | no |
| F32 | The dashboard's auth counters go to a Metrics instance that is never exported | P1 (control-plane telemetry) | **FIXED** — the auth state shares the engine's metrics instance, so the CSRF and lockout counters reach the exporter | no |
| F33 | The shipped server ConfigMap sets an ipc.max_connections the daemon refuses to start with | P1 (deployment) | **FIXED** — the ConfigMap sets 1,024, the value the validator accepts (`1d57839`, PR #68) | no |
| F34 | The Kubernetes probes target the pod IP while the daemon binds loopback | P1 (deployment) | **FIXED** — the probes address the loopback bind the daemon actually uses | no |
| F35 | The ramshield-cli binary is a stub, yet the release ships it and the installer requires it | P1 (operator tooling) | **FIXED** — the pre-deletion implementation is restored and works | no |
| F36 | The 403 rejections are counted but never exported | P2 (metrics) | **FIXED** — the 403 rejections are exported | no |
| F37 | The store-RAM refusals are counted but no surface exposes them | P2 (metrics) | **FIXED** — the store's RAM refusals are exported and shown | no |
| F38 | The XDP-divergence stale flag is erased 250 ms after it is raised | P2 (health signal) | **FIXED** — the age tick no longer clears the flag a failed apply raised | no |
| F39 | The "Drops vs Ingress" chart never plots drops | P2 (operator-visible) | **FIXED** — the chart plots the real drop rate | no |
| F40 | The blocklist gauge compares a lifetime counter with a hardcoded capacity | P2 (operator-visible) | **FIXED** — the blocklist row is labelled as what it shows | no |
| F41 | Two dashboard rows are overwritten with `n/a` by the SSE path | P2 (operator-visible) | **FIXED** — the SSE path no longer overwrites the two rows | no |
| F42 | The release gate greps for a dependency-audit flag the workflow no longer uses | P2 (CI) | **FIXED** — the gate checks the flags the workflow runs | no |
| F43 | The installer writes a unit that grants the daemon write access it must never have | P2 (security) | **FIXED** — the installer no longer grants /proc/sys write access | no |
| F44 | The Ansible deploy role renders into a directory that does not exist and applies a stub template | P2 (deployment) | OPEN — The k8s deploy path cannot work: the rendered Deployment mounts no config, the `ramshield-admin` Secret is absent, and the static manifests in `deploy/k8s/` are never applied. The default `latest` tag also violates the pinned-tag policy `scripts/verify_release.sh:12` enforces. | no |
| F45 | Two exported series miss the `ramshield_` namespace | P3 (metrics) | **FIXED** — every exported series carries the namespace | no |
| F46 | The pipeline stage bars mix units | P3 (display) | **FIXED** — the pipeline stages share their units | no |
| F47 | The quarantine table fabricates the severity and tier of every row | P3 (display) | **FIXED** — the quarantine table shows the record's reason | no |
| F48 | `/metrics` bypasses the session auth that guards every other route | P3 (exposure) | OPEN — An unauthenticated metrics endpoint on a port the network policy opens to other namespaces. | no |
| F49 | The backlog gate's open-row parser never matches BACKLOG.md | P3 (CI) | **FIXED** — the gate's parser reads the table BACKLOG.md has | no |
| F50 | Three scheduled workflows can never run | P3 (CI) | **FIXED** — the three dead scheduled workflows are deleted | no |
| F51 | The qualification docs tell operators to run a cargo-audit invocation that does not exist | P3 (docs) | **FIXED** — the docs name the flags the workflow runs (PR #83) | no |
| F12 | Two stray compiled binaries tracked at the repository root | P2 (hygiene) | **CLOSED** — removed and gitignored in `f900664` | no |
| F14 | The store's `insert()` publishes a record and then updates the blocked index, with the shard lock released in between | P2 (latent) | **FIXED** — the vacant branch of `insert()` and of `update_ip()` now publish with `insert_entry`, which keeps the shard lock until the guard is dropped, and the index and the size accounting land in that critical section | no |
| F15 | The Kubernetes DaemonSet asks for a `Bidirectional` mount propagation without `privileged` | P1 | **FIXED** — the DaemonSet drops the Bidirectional propagation and asks for no privileged container | no |
| F16 | The ansible quality gate reported a red CI run as a pass | P2 | **FIXED** — the role now compares the run's conclusion with `success` and exits 1 otherwise | no |
| F17 | The dashboard's quarantine table had 5 headers and 6 cells, and its CSV export dropped the Time column | P2 | **FIXED** — the surplus cell is gone and the export takes all five columns | no |
| F18 | `scripts/attack_nexus.py`'s `stop` command does not stop the flood | P3 | **FIXED** — the flood polls the flag the command sets | no |
| F19 | Two helper scripts read `config.prod.toml.example`, which does not exist on master | P3 | **FIXED** — the dead read is gone and the other script reads the baseline | no |
| F20 | `scripts/network_create.py`'s `dual` family produces IPv4 only | P3 | **FIXED** — `dual` now emits a /24 and a /64 per index | no |
| F21 | `scripts/xdp_netns_sim.py` ignores `--dashboard` in its pre-flight check | P3 | **FIXED** — the pre-flight uses the parsed dashboard URL | no |

---

## F1 — CGNAT shared-memory lookup semantics

**Status:** CLOSED — implemented, gated, reviewed and pushed as `d1f4a81`.
The report is `docs/development/CGNAT_SHM_LOOKUP_FIXES.md`; the two
independent reviews are `CGNAT_SHM_LOOKUP_REVIEW.md` and
`CGNAT_SHM_LOOKUP_DELTA_REVIEW.md`.

**The ABI question dissolved.** It was flagged as needing an owner
compatibility decision, on the reasoning that a tri-state return type
changes a published C API. It does not: the header is `static inline`, so
every consumer compiles it in. The tri-state was added **alongside** the
existing functions, whose signatures and behaviour are unchanged — `true` only
on a match, `false` otherwise, exactly as before. No consumer breaks, and
there is no ABI change to decide.

**The scope was corrected by the investigation.** The C header is the only
rule-lookup path in this repository; the Rust `ramshield-cgnat` crate has
**no read path at all** — it only publishes, and `get_slot` is used inside
`publish_rule`, in one test and one bench. So the earlier review's suggested
"align the Rust read path" step is a no-op, and the review's hedge about
that path being unverified resolves into something stronger.

**A third defect was found that neither the audit nor the review had
reported:** a /0 prefix masks to network 0, which hashes to key 0 — the
reserved empty-slot sentinel — and the publisher treats a zero-hash slot as
free. A rule published under that key sat in a slot every other publisher
considered empty, so the next colliding publish overwrote it, and a
prefix-0 lookup was routed to key 0 with no error. The publisher now
refuses the reserved key, and the lookup rejects it as `INVALID` rather
than routing it into the table.

The reserved key is not only the /0 case: the slot-key mixer is
0-preserving, so a key is 0 exactly when the masked network address equals
its prefix length — every /0, and `0.0.0.32/32` among others.

**The original finding, for the record:**
**Files:** `crates/ramshield-cgnat/include/ramshield_shm.h`, issue `#30`,
draft PR `#32`
**Severity:** P1/P2

### The defect

The C reader still returns a boolean. It retries a bounded number of times
when the sequence counter is odd or changes during a read, but it ultimately
returns `false` when it cannot obtain a coherent snapshot.

That means the API still cannot distinguish a genuine miss from a contended
read.

The IPv4 wrapper also still converts an invalid prefix length greater than 32
to key zero and passes that key into the generic lookup.

The independent review adds an important qualification: **no in-repository
production caller of this C header was established.** The published C API
still has the contract defect, but the separate Rust SHM implementation must
not be assumed to share it without inspecting that implementation
independently.

### Required fix

1. Give callers distinct `FOUND`, `MISS`, `CONTENDED` and invalid-input
   results.
2. Reject invalid prefixes before lookup.
3. Audit all callers and define their response to contention.
4. Preserve bounded retries.
5. Add deterministic contention and prefix-boundary tests.
6. Verify C/Rust ABI layout and run the relevant SHM stress gates.

### Closure criterion

A contended lookup cannot masquerade as a coherent miss, and the result SHA
has passing C/Rust regression tests plus independent review.

### Notes for whoever picks it up

- **Do not count draft PR `#32` as resolved** merely because its title
  describes the intended fix. It is a draft, and its own description says it
  has not been locally compiled or tested in that orchestration pass.
- - The Rust implementation in `crates/ramshield-cgnat/src/shm.rs` is a separate
  implementation and was **not** verified by the review. Inspect it
  independently; do not assume the defect is present or absent there.
- The `shm-abi` gate in the local 14-gate runner and the C ABI/stress checks
  in CI are the relevant gates.

---

## F1a — a /0 rule is not representable in the SHM table (residual of F1)

**Status:** OPEN — a design gap, deliberately not fixed in F1.

**"Unexpressible", defined.** A rule the policy language can state but the
SHM rule table has no representation for. It is a *representational*
limitation of one data structure — not a missing feature, not a bug in the
enforcement path, and not something the reader can work around.

**What the table is.** A 262 144-slot open-addressed table in shared
memory, 64 bytes per slot, 4-slot probe window. A slot holds a rule:
`client_hash`, `expires_at_ms`, two challenge-seed words, `max_rps`,
`tier`, `flags`. The slot index is
`subnet_key(masked_network, prefix_len) & (CAPACITY-1)`. The Rust crate
only writes; the C header in `crates/ramshield-cgnat/include/ramshield_shm.h`
is the only reader, and its consumer is an out-of-repo native reverse
proxy. It is a side channel: in-repo enforcement goes through the XDP LPM
map and is unaffected by anything in this table.

**Why a /0 is unexpressable.** Key `0` is the **empty-slot sentinel** — a
slot whose `client_hash == 0` is free, and the publisher picks the first
such slot. But the slot-key mixer is 0-preserving, so
`subnet_key(N, p) == 0` exactly when the masked network `N` equals the
prefix length `p`. The key that would mean "the default route" is the
same key that means "no rule here". There is no spare bit in the slot to
tell them apart: `client_hash` is the only discriminator, and `0` is
taken. Every `/0` masks to network 0, so every `/0` lands on the sentinel.
So does `0.0.0.32/32`, and so can an IPv6 subnet whose u128 key truncates
to 0 in u64.

**What F1 did.** F1 closed the lookup contract, and with it the
reserved-key hazard: the publisher refuses `client_hash == 0` and the
reader rejects it with `INVALID`. The consequence is that **a /0 rule is
now inexpressible end to end** — the table says so instead of storing
something invisible.

**What it does NOT affect.** The in-repo enforcement path: a /0 CIDR block
is still enforced through the normal channel, and the userspace path
remains authoritative.

**Why it was not fixed here:** supporting /0 needs a different key
derivation, not a patch. The reserved sentinel and the key space are
coupled in the on-disk layout, so changing the derivation is a format
decision, and the format is shared with an out-of-repo consumer.

**The three options, with their consequences.** This is a format decision
for the owner, not a patch:

| option | what it means | consequence |
|---|---|---|
| **Retain the restriction** | `/0` stays unsupported in this side-channel format | nothing changes in the format; the cheapest and the safest. The consumers must be told, and the in-repo enforcement path keeps covering `/0`. Cheapest and safest. |
| **Change the representation** | give `/0` an explicit encoding that does not collide with the reserved key | the key derivation changes on both sides and the parity vectors are re-pinned. The format is shared with an out-of-repo consumer, so this needs **coordinated compatibility work** — the consumer must be updated in the same release, or it will look up the wrong slot. |
| **Use a separate representation** | carry a default-route rule in its own field or mechanism rather than in the table | the table format stays frozen, but a second mechanism has to be defined, published, read and enforced by the consumer. More moving parts, no compatibility break. |

What must not happen is silently reinterpreting the reserved key: a
consumer outside this repository would keep using the old key space.

**Required fix, when it is wanted:** the owner picks an option, then the C
and Rust derivations change together and the parity vectors are re-pinned.
This is a follow-up, not a defect in the shipped behaviour.

---

## F2 — Native capture worker lifecycle

**Status:** FIXED (P2) — on `master` as `15e0551` (PR #58, the code
commit `80fa333`); the policy half is the owner's and it is decided:
**degrade, keep serving**.

**Files:** `src/engine/native.rs`, `src/engine/boot.rs`,
`src/engine/mod.rs`, `src/dashboard/mod.rs`,
`crates/ramshield-metrics/src/`
**Severity:** P2

### The defect

The drop-accounting fix (`60ce0f1`) is good, but it addresses a different
problem from worker lifecycle.

Current master still:

- collects thread-spawn results and uses `handles.into_iter().flatten()`,
  silently discarding thread-creation errors;
- logs a warning when a worker's capture function exits with an error, then
  allows that worker to terminate;
- discards thread join results;
- lacks a durable capture-health signal and a defined restart or degraded-state
  policy.

The supervisor itself can start successfully even though its workers
subsequently fail to start.

**Why it matters:** the detection-worker startup path already refuses
readiness when those workers fail (`src/engine/boot.rs` — detection refuses
ready state), while the native capture path does not provide equivalent
assurance. That asymmetry is the defect.

### Required fix

1. **Handle every worker-spawn result explicitly.** Done. The workers are
   now created in `spawn()` instead of inside the supervisor thread, so
   the spawn result is known before the daemon is told the capture path
   started. `start_workers` counts the starts and the failures; a run in
   which no worker started returns an error instead of a supervisor that
   looks healthy.
2. **Track expected, started, running and failed workers.** Done.
   `ramshield_native_capture_workers_expected`,
   `ramshield_native_capture_workers_started`,
   `ramshield_native_capture_workers_running` and
   `ramshield_native_capture_worker_failures_total`, plus the same
   numbers in the dashboard snapshot and in `/api/healthz`.
3. **Define bounded restart behavior or a clear failure policy.** Done.
   `capture_slot` makes at most three capture attempts, a second apart,
   and stops at once when the daemon is shutting down.
4. **Expose native capture status through existing health/metrics
   mechanisms.** Done: the four Prometheus series, the dashboard snapshot
   and `/api/healthz`.
5. **Decide whether total capture failure should refuse readiness or mark
   the service degraded.** Done — the owner decided on 2026-10-22:
   **degrade, keep serving.** Capture is observation, not enforcement:
   the detection pipeline has a second event source (the IPC path feeds
   the same channel) and XDP/netfilter remain the enforcement layers, so
   a telemetry fault must not become an enforcement outage. The daemon
   reports `Degraded` with the health reason `native capture stopped`,
   keeps `is_healthy` true and keeps serving. The node DaemonSet enables
   capture and the systemd unit has `Restart=on-failure`, so refusing
   readiness at runtime would have turned a telemetry fault into a crash
   loop.
6. **Test partial startup failure, total failure, runtime exit, shutdown
   and restart exhaustion.** Done — `src/engine/tests/native_capture.rs`,
   eleven tests. The spawner and the capture attempt are parameters, so
   every case is produced deterministically, without a NIC and without a
   race.

### Closure criterion

Capture cannot silently disappear while the service continues to imply that
the configured subsystem is functioning.

**Met.** A capture that never started, or that died, is visible in the
metrics, in the dashboard snapshot and in the health reason, and the
service says `Degraded`.

### Notes for whoever picks it up

- This is an **observability and lifecycle defect** in the optional
  native-ingest path, **not** evidence of a memory-safety vulnerability.
  `native_ingest.enabled` defaults to `false`, so the blast radius is limited
  to deployments that enable it.
- It should be prioritized **after** the CGNAT fix (F1).
- Item 5 was a **policy decision for the owner**: restart the worker, fail
  the feature, or degrade the service. The in-repo precedent is the
  detection path, which refuses readiness when its workers fail to start.
  The owner decided on 2026-10-22 that the capture path **degrades the
  service and keeps serving**; the reasoning is in the section above.
- The metrics surface added by `60ce0f1` (`ramshield_native_ingest_drops_*`)
  is the natural place to hang a capture-health gauge.

---

## F3 — Native ingest drops were invisible

**Status:** CLOSED — `60ce0f1`, "the native capture path must not lose
observation events silently".

**The defect.** The native capture path sends without blocking, so a
saturated or closed channel drops the event rather than stalling the
capture thread - the correct overload policy for a real-time ingest path.
But the result was discarded (`let _ = tx.try_send(ev);`), so no counter
moved. A repo-wide search showed `Metrics::inc_shed` was never called
anywhere, so the pre-existing `ramshield_events_shed_total` was
permanently 0 as well. Under a flood - the exact condition the design is
built for - the operator could not tell that observation events were
being lost.

**The fix.** Producer-side drop counters, and the
`ramshield_native_ingest_drops_total` metric wired to them. The report
is `docs/development/NATIVE_INGEST_DROPS_FIXES.md`; the review is
`docs/development/NATIVE_INGEST_DROPS_REVIEW.md`.

## F4 — The XDP LRU eviction residual was not stated

**Status:** CLOSED — `79f4eac`, "verify the 21 Oct master audit, and state
the LRU eviction residual".

**The defect.** The 21 October audit's finding F4 is an accepted design
trade-off: the XDP dataplane evicts by LRU under memory pressure. The
only actionable residue was wording - the behaviour was real and correct,
but nothing in the repository said so, so a reader of the code could
reasonably conclude the opposite.

**The fix.** The behaviour is stated where the eviction happens and in the
documentation that describes the dataplane. The review is
`docs/development/MASTER_AUDIT_2026_10_21_REVIEW.md`.

## F5 — The local control plane was unauthenticated

**Status:** CLOSED — `b1a9e9e`, "the control plane requires a verified
principal for mutations".

**The defect.** The IPC was unauthenticated by default, and the exposure
was a *write*, not a read. With the default configuration
(`ipc.auth_keys` empty, `ipc.require_auth` false), `parse_frame` accepted
a frame carrying no auth envelope and returned `principal = None`;
`process_request` then skipped the role table entirely and the actor fell
back to a hard-coded "admin". A local process could therefore issue
`unblock_ip` and remove an active mitigation from a customer under
attack. The behavioural proof, taken before the fix, is the server's own
answer:

```
{"type":"ok","message":"unblock queued for 203.0.113.7"}
```

The bind defaults to `127.0.0.1:7890` and `validate()` refuses a public
bind without HMAC keys, so the surface was loopback-only - but loopback
is not a boundary between mutually untrusted local processes.

**The fix.** A mutation requires a verified principal with a role. The
report is `docs/development/MASTER_SCAN_FIXES.md`.

## F6 — `BACKLOG.md` is not a reliable master release ledger

**Status:** CLOSED (P2, documentation) — the owner lifted the standing fence
on 2026-10-21 and the file was re-pointed at `master`.
**Files:** `BACKLOG.md`, `docs/development/AUDIT_FINDINGS_BACKLOG.md` (new)
**Review:** `docs/development/BACKLOG_VALIDATION_AGAINST_MASTER.md`

The independent review found the header stale on all four cited facts, 18 of
33 rows carrying PASS/closed statuses that no longer described master, 4
rows describing defects the work since had closed, and 4 items open on
master that the file never mentioned. It also found that the fence's
designated successor file, `docs/development/AUDIT_FINDINGS_BACKLOG.md`,
did not exist on master at all — it existed only on the parked mesh branch.

**What was done:** the audit-branch material moved into the new
`docs/development/AUDIT_FINDINGS_BACKLOG.md` under a new history header (the
status tables stayed in `BACKLOG.md`, so the two files are not byte-identical);
`BACKLOG.md` was re-pointed at `master` and the assessed SHA; all 33 rows
were re-stamped — 4 as `CLOSED (commit)`, 3 as `PASS (exact SHA)` on the
current SHA with the evidence named, the rest as `PASS (historical)` or
`OPEN` with the verification state stated; a new "Open on master" section
records the 11 items the file did not mention; and the 100/100 gate is
bound to a SHA with the note that it has never been satisfied on any
single SHA.

**One review claim was corrected before it was written down.** The review
stated that the parked branch `feature/mesh-f01-hmac-edge-secrets` (unpublished) "contains no
`crates/ramshield-mesh` source change". That is wrong: the branch carries
625 changed lines in `crates/ramshield-mesh/src/transport.rs` plus test
changes. The conclusion the reviewer drew — that none of it is on master —
is correct, because a diff against master shows the branch is behind
master. The prose in `BACKLOG.md` states the accurate version.

**The report's original finding, for the record:** the checked-in header
described `audit-branch` as the target branch and cited three superseded
SHAs, and PASS rows were pinned to a SHA that was no longer master, so the
file was useful as historical task context but unsafe as the sole answer to
"what is proven on current master?".

**Required fix — done:** re-point the header at `master` and the current
exact SHA; split the historical `audit-branch` notes into a clearly labelled
appendix; re-stamp every current PASS row with its exact SHA and workflow
URLs. The standing fence was lifted by the owner on 2026-10-21, and
`docs/development/AUDIT_FINDINGS_BACKLOG.md` is now the home for new work.

---

## F7 — Performance and memory claims require workload evidence

**Status:** OPEN (P2, process — no code change)
**Files:** `config.baseline.toml`, issue `#35`, draft PR `#34`

The baseline config's `ram_limit_mb = 1024` and `pre_aggs_max_size = 1_000_000`
are **configuration ceilings**, not measured RSS. The "~100 MB working set"
wording is a sizing rationale, not an observed figure.

**One instance is now closed by measurement.** The CGNAT table's
"(<15ns)" comment in `crates/ramshield-detection/src/lib.rs` was a cost
claim with no measurement behind it. `scripts/shm_lookup_bench.c` now
measures the lookup cost by probe distance and over a mixed workload, and
the comment carries the measured figures instead of the unsupported one.
That is the standard this finding asks for, applied to the one claim the
audit could point at. What is left is the policy: no RSS or throughput
figure goes public without the workload and the machine recorded.

**Required fix:** before any RSS or throughput figure is published, record the
exact SHA, build profile and features, toolchain and config hash; the machine,
CPU, RAM, kernel, driver and NIC; the workload generator, packet sizes and
rates, unique-source cardinality and duration; process RSS and peak RSS
separately from kernel/XDP maps and page cache; CPU, latency, drops, detection
quality and recovery behaviour; and repeated runs with raw output. Keep
sub-20ns microbenchmark language out of public claims unless a reproducible
benchmark supports it on the actual implementation and target machine.

**Open question, now checkable:** the audit's claim that PR `#34` is a draft describing 50 MB
as a target was **UNVERIFIED** by the review (a remote-only artifact). The
reviewer did not contradict it but did not confirm it.

---

## F9 — Enforcement's block path is a lost update against detection

**Status:** FIXED (P1) — merged as `682435a` (PR #43). Found while
re-verifying backlog row I-011, and **not** what that row describes.

**Where it came from.** Backlog row I-011 asks whether "cardinality-driven
LRU eviction" can erase an attacker's CUSUM/pulse-wave history. Its premise
is wrong: there is no LRU eviction. `crates/ramshield-storage/src/store/capacity.rs:23-51`
evicts only on TTL expiry, production inserts carry no TTL
(`crates/ramshield-storage/src/lib.rs:91-99`), and under RAM pressure the
store **refuses net-new keys** rather than evicting live ones
(`crates/ramshield-storage/src/store/write.rs:103-152`). The row's stated
mechanism does not exist in the code.

**The real defect, in the same area.** The enforcement actor's block path is
a read-modify-write across two lock acquisitions:

- `crates/ramshield-enforcement/src/service/enforce.rs:244-283` does
  `store.get(&cmd.ip)` -> mutate `block_state` -> `store.insert(...)`.
- `store.get()` returns `None` for an expired record
  (`crates/ramshield-storage/src/store/access.rs:4-10`), so the
  `unwrap_or(IpRecord { cusum_s: 0.0, sample_count: 0, ... })` default
  **replaces the record and erases its detection history**.
- Between the `get()` and the `insert()`, a concurrent detection flush can
  run `update_ip` and advance `cusum_s`, `sample_count` and the pulse
  counters. The enforcement `insert()` then writes back its stale snapshot,
  **rolling those counters back**. This is the mirror image of the P0 fixed
  in `write.rs:156-173` — that fix made `update_ip` mutate in place so it
  cannot clobber `block_state`; the same window still lets enforcement
  clobber the *stat* fields.

**Why it matters.** Detection's own state is the attacker's history. If a
block lands concurrently with a flush, the attacker's accumulated evidence
is rewound, and `CUSUM_WARMUP_SAMPLES = 6`
(`crates/ramshield-detection/src/rate_tracker.rs:18`) may never be reached,
so `hot || cusum_fired || pulse_fired || relative_fired`
(`crates/ramshield-detection/src/merge.rs:140-163`) never fires for that IP.

**Required fix.** Give the block path the same single-shard-lock treatment
`update_ip` has: one mutate that touches only `block_state` and leaves the
stat fields alone, instead of `get()` + `insert()`. The store's own
contract already says the mutation must not touch `block_state`
(`write.rs:165-168`) — a second, block-only mutator is the missing piece.
It must also not resurrect a *clean* record for an IP that is still
blocked, which is what the expired-record default does today.

**Required test — met.** A test that races a detection `update_ip` against
the block path and asserts the stat fields are not rolled back. Six tests
in the storage crate pin it, including the property test
(`a_block_never_makes_the_detection_counters_go_backwards`), which was
proven load-bearing: reimplementing `block_ip` as the old `get()` +
`insert()` makes it fail.

**A defect in the first version of the fix, found by independent review and
corrected before it landed.** `block_ip`'s vacant branch published the
record with `insert`, which releases the shard lock, and updated the
blocked index afterwards. A concurrent unblock could land in that window
and decrement a counter that was still zero - a `u64` underflow - leaving
the index holding a key whose record was clean. The vacant branch now uses
`insert_entry`, which keeps the lock, and updates the index inside it. The
test that pins it
(`the_blocked_index_is_updated_under_the_same_lock_as_the_record`) fails in
186 of 200 rounds against the old code, 121 of them on the very first
round; with the fix it passes every round.

**A second, related observation** (policy, not a defect): an IP refused by
the RAM budget is dropped from detection entirely
(`crates/ramshield-detection/src/flush.rs:216-239`, `stored == false` ->
`cold_skipped`), so its `sample_count` restarts at 0 and the CUSUM warmup
never completes. The refusal is already observable
(`capacity_exceeded_count` / `capacity_exceeded_ips`); whether a
capacity-refused IP should fail open or fail closed is a posture decision
and is not proposed here.

**A correction to the commit message.** The commit message of `7457150`
says the new property test is deterministic. An independent re-review of
that fix found it is not: the test fails the old code in 186 of 200
rounds, and 121 of those failures are on round 0 - it is a probabilistic
test with a high hit rate, not a deterministic one. The commit is merged,
so the message cannot be amended; this note is the correction. The
deterministic part of the fix is covered by the single-threaded tests
(`a_block_does_not_roll_back_the_detection_counters`,
`an_unblock_does_not_roll_back_the_detection_counters`), which fail on
the old code every time.

**What the same re-review found next door.** The re-review also found
that `update_ip` still had the shape F9 removed from `block_ip`: the
vacant branch published the record without updating the blocked index,
and the occupied branch released the shard lock before the size
accounting. That is fixed with F14.

**What a later review of the F-14 fix found.** The same review found
the eviction paths doing the same thing - `Store::remove` and
`evict_batch` updated the blocked index after the shard lock was
released, which could drop a live block from the XDP map. That is F24,
and it is fixed. It also noted that three of the F-9 tests are
sequential, so they assert the right invariants but do not fail against
the old two-acquisition pattern; the property test and the F-14 index
test are the two that are load-bearing. That is recorded here so no one
quotes the four sequential tests as proof the lost update cannot come
back.

---

## F8 — The audit was not locally reproduced

**Status:** not actionable against the code.

The audit is accurate about its own method: it states that it did not run
`cargo test`, `cargo clippy`, the C SHM tests, the live XDP tests or
crash-injection tests. The practical consequence is that its P1/P2 labels are
unverified judgments, and it caps the confidence of every other finding.

**Before any P1 item is closed**, require the corresponding gate on one exact
SHA and treat the audit's severity labels as hypotheses.

---

## F10 — The IPC server MACs a key-sorted payload, not the client's bytes

**Status:** FIXED by the owner's decision (P1). The server's rule stands: the
MAC covers the frame minus its `auth` object, re-serialized compact and
key-sorted. The documentation already says so, and the restored CLI signs
exactly that.

**What the server does.** `verify_frame_auth`
(`src/ipc/server/auth.rs:94-115`) parses the frame into a
`serde_json::Value`, removes the `auth` object, and re-serialises the rest
with `serde_json::to_vec`. The signature is checked against **those** bytes.

`serde_json` is built without `preserve_order` (`Cargo.lock`: the
`serde_json` package's dependencies are `itoa`, `memchr`, `serde`,
`serde_core`, `zmij` — no `indexmap`), so `Value`'s map is a `BTreeMap` and
that re-serialisation is **key-sorted**. The server never sees the
client's key order.

Verified here, not assumed: a probe signing `{"zebra":1,"apple":2}` and
re-serialising it produced `{"apple":2,"zebra":1}`.

**The consequence.** A client that signs the frame in its own key order
produces a different MAC input and is rejected with "signature mismatch".
The rule a client must follow is: sign the compact, **key-sorted**
serialisation of the frame without its `auth` object. `docs/IPC.md:125`
happens to do this (`json.dumps(req, separators=(',',':'), sort_keys=True)`),
which is why the repo's own example works — and why the bug went unnoticed.

**Why nothing caught it, at the time.** Before `f900664` no test in the tree
signed a multi-key frame through the server. `src/ipc/server/tests/mod.rs`
used a single-key payload (`{"type":"get_status"}`), where sort order
cannot differ, so the test passed either way and pinned nothing.
`crates/ramshield-protocol/tests/replay.rs` calls `auth::verify` directly,
bypassing the server's re-serialisation entirely.

**What catches it now.** `src/ipc/server/tests/mod.rs` carries
`verify_frame_auth_signs_the_key_sorted_payload_not_the_client_bytes`, which
signs a multi-key frame and asserts a client-order signature is rejected.

**The client-side face.** `scripts/auth_e2e.py:43` signs with
`json.dumps(body, separators=(",", ":"), ensure_ascii=False)`, which keeps
the dict's insertion order, while the server re-serialises a sorted map. The
script only ever signs a one-key payload, so it does not fail today - but any
second key in a different order would. `scripts/final_integration.py:37`
already passes `sort_keys=True`, so the two runners disagree with each other
today.

That is the concrete cost of the decision: if the owner picks "keep the
server's sorted canonicalisation", the fix is one argument in
`scripts/auth_e2e.py` and the contract is documented. If the owner picks
`preserve_order`, the server changes instead. Until then this finding stays
open - it is one decision, not two.

**What was done about it.** The README now states the real rule
(`crates/ramshield-protocol/README.md`), a guard test
(`readme_states_the_key_sorted_signing_rule`) fails if it drifts back, and
`verify_frame_auth_signs_the_key_sorted_payload_not_the_client_bytes` in
`src/ipc/server/tests/mod.rs` exercises a multi-key frame end to end: the
same signature is accepted for either wire order, and a signature over the
client's own order is rejected.

**What is still open.** The server behaviour itself is unchanged, and that
is a decision for the owner, not a defect to be patched in the same pass:

- **Keep the sorted canonical form** and document it as the contract.
  Clients must sort. No wire change; any existing client that does not
  sort is broken today and would stay broken.
- **Enable `preserve_order`** so the server MACs the client's own bytes.
  That makes "sign the bytes you sent" true, at the cost of a dependency
  feature and of two clients that order differently producing different
  MAC inputs for the same logical frame.
- **Canonicalise explicitly** — define a canonical byte form, sign that,
  and say so in the protocol. The most work, the least surprise.

The current README documents the first option, which is what the code
actually does. The choice of which option to *enforce* is the open part.

**The client-side face of this, in the helper scripts.** Two of the
repository's own scripts sign frames: `scripts/final_integration.py`
signs with `sort_keys=True`, and `scripts/auth_e2e.py:43` does not. The
second is not a live failure - the only payload it signs is
`{"type": "get_status"}`, one key, so the client's bytes and the
server's sorted bytes are the same bytes - but it would break the
moment the script signs a multi-key frame. If the owner picks the
sorted canonical form, that one line becomes `sort_keys=True`, and
this finding can be closed as "the contract is documented and the
clients conform".

---

## F11 — Crate README documentation drift

**Status:** FIXED (P2) — all 62 claims are fixed; every README the sweep covers is pinned by a test.

**What this is.** The I-025 sweep read every claim in the crate READMEs
against the source and listed the ones that were false. The per-claim
list is `docs/development/DOC_DRIFT_SWEEP.md`, which names each claim,
the quoted README line, the code that says otherwise, and the fix.

**What is fixed.** All eleven `ramshield-config` claims, four of the
five `ramshield-xdp` ones, all thirteen `ramshield-types` ones and all
eight `ramshield-storage` ones, plus one `ramshield-forecasting` claim.
The config crate's eleven are pinned by a test rather than by prose:
`crates/ramshield-config/tests/readme_template.rs` extracts the ```toml
block out of the README and loads it through `Config::from_toml_file`,
which parses and validates. The old template failed that test. The types
and storage crates are pinned the same way:
`crates/ramshield-types/tests/readme_claims.rs` and
`crates/ramshield-storage/tests/readme_claims.rs` check the README's
dependency list against `Cargo.toml`, its "read by" list against the
workspace manifests, every type it shows against the crate's public API,
and its test count against the tests that exist. The old READMEs failed
the first two. The metrics, analytics and forecasting crates are pinned
the same way and more: `crates/ramshield-metrics/tests/readme_claims.rs`,
`crates/ramshield-analytics/tests/readme_claims.rs` and
`crates/ramshield-forecasting/tests/readme_claims.rs` also check the
Prometheus series the README names against the exporter, the block
modules against the modules actually passed to `record_block_ip`, the
reason tokens against `BlockReason::as_str()`, the memory sizes against
`size_of`, the import paths against the crate's real module layout, the
config defaults against the config crate, and the field lists against the
structs.

**What was fixed.** All 62 claims. The `ramshield-forecasting` README was
the last one: an audit of all 64 claims in the file found 32 false ones,
including a `HoltWinters` struct with the wrong field list, a period that
defaulted to 120 ticks where the config says 3,600, "three independent
models" where the models are chained, a `CusumState` with four public
fields where all four are private, an `EwmAVar` described as "3 floats"
where it is four fields, a threshold of 0.95 where the code uses 0.75, a
dependency on `ramshield-enforcement` the crate does not have, and a
diagram that implied the z-score alone blocks.

**Closure criterion.** Every claim in the sweep is either marked fixed, or
the README says what the code says.

## F12 — two compiled binaries are tracked in the repository

**Status:** CLOSED — removed in `e668995`, merged to master via `f900664`;
both names are in `.gitignore`.

**Severity:** P2 (hygiene, not a runtime defect)

`validate_mesh_fix` and `verify_boot_fix` are ELF executables of about
4.4 MB each, committed at the root of the repository in `415f75f`
("docs: update README files and add code health backlog"). Nothing in
the tree references either of them, and no build target produces them at
the root: they are build output that was committed by accident.

They also carry the builder's home directory in their debug info
(`/home/m/.rustup/toolchains/...`), which is how the dev-path check
found them.

**What was done about it.** Both files are removed and both names are in
`.gitignore`, so a rebuild cannot put them back. The dev-path check no
longer skips binary files (`git grep -I` skipped them, so a path could
hide in one); the check reports a binary hit as "Binary file ... matches".

**What is still open.** Nothing. `git ls-tree -r origin/master` lists
neither name, and `.gitignore` carries both.

---

## F14 — the store's `insert()` updates the blocked index after releasing the shard lock

**Status:** FIXED (P2) — the vacant branch of `insert()` and of
`update_ip()` now publish under the shard lock, and the size accounting
moved inside it too.

**What it was.** The same shape as the defect F9 fixed, one function
over. `Store::insert` published a record with `v.insert(...)`, which
releases the shard lock, and only then updated the blocked index:

    crates/ramshield-storage/src/store/write.rs:130  v.insert(new_entry);
    crates/ramshield-storage/src/store/write.rs:131  if new_blocked {
    crates/ramshield-storage/src/store/write.rs:132      self.blocked_count.fetch_add(1, Ordering::Relaxed);
    crates/ramshield-storage/src/store/write.rs:133      self.blocked_set.insert(key, ());
    crates/ramshield-storage/src/store/write.rs:134  }

A concurrent unblock landing in that window would decrement a counter
that is still zero, and leave the index holding a key whose record was
clean — the exact failure F9's fix removed from `block_ip`.

**Why it was not live.** The only callers that can insert a *blocked*
record are the startup recovery paths: the WAL replay
(`crates/ramshield-enforcement/src/replay.rs:186`, called from
`src/engine/boot.rs:470-483`) and the checkpoint restore
(`src/engine/checkpoint.rs:259`). Both run before the enforcement actor
serves traffic, so no unblock can be in flight. The live detection path
does not insert: it mutates in place with `update_ip`
(`crates/ramshield-detection/src/merge.rs:31`), which by contract never
touches `block_state`.

**What an independent re-review of the F9 fix found.** `update_ip` had
the same shape in two places, and one of them was live:

- The vacant branch published the caller's record and never touched the
  blocked index at all, so a blocked record published there was invisible
  to every reader that asks the index. The store's own invariant - every
  blocked record is in `blocked_set`, every entry in `blocked_set` is a
  blocked record - was broken by construction. The only caller passes
  `BlockState::Clean`, so nothing was broken in practice.
- The occupied branch released the shard lock before
  `apply_growth_size_only`, so the capacity gate could be momentarily
  exceeded by a concurrent insert. This path **is** live: the detection
  flush calls `update_ip` on every new IP.

**What changed.** `crates/ramshield-storage/src/store/write.rs`:

- `insert()`'s vacant branch publishes with `v.insert_entry(...)` instead
  of `v.insert(...)`. `insert_entry` returns an `OccupiedEntry` that
  holds the shard's write guard, so the lock is released only when the
  guard is dropped - after the blocked index, the TTL count and the size
  accounting have landed. The index update goes through the same
  `sync_blocked_index` helper the block and unblock paths use.
- `update_ip()`'s vacant branch does the same, and it now syncs the
  blocked index for the record it publishes, using the record's own
  block state.
- `update_ip()`'s occupied branch moves `apply_growth_size_only` inside
  the lock, before the guard is dropped, so the capacity gate and the
  bytes it accounts for cannot disagree.

**The test.** `an_update_that_publishes_a_blocked_record_indexes_it` in
`crates/ramshield-storage/src/tests/mod.rs` publishes a blocked record
through `update_ip` on a fresh key and asserts that the counter and the
index both know about it, then runs the store's own invariant check. It
fails on the old code with "the blocked record was not counted". The
concurrent case is covered by the store's existing stress test
(`stress_block_unblock_concurrent`), which asserts the same invariants
after eight threads have hammered insert, unblock and remove.

**What was still open, and is now fixed too.** The removal paths had the
same shape: `Store::remove` and `evict_batch` took the record out with a
call that releases the shard lock, and only then decremented the counter
and dropped the index entry. A concurrent `block_ip` for the same key can
publish a blocked record and index it in between, and the removal then
deletes the index entry of a record that is blocked. That was F24, and it
is fixed with this change.

---

## F15 — the Kubernetes DaemonSet asks for a Bidirectional mount propagation it cannot have

**Status:** FIXED — the DaemonSet drops the Bidirectional propagation and asks for no privileged container

**What it is.** `deploy/k8s/daemonset.yaml:93-97` mounts the BPF
filesystem with `mountPropagation: Bidirectional`, and the file has no
`privileged: true` anywhere. The container instead drops every
capability and adds back four (`NET_ADMIN`, `BPF`, `PERFMON`,
`NET_RAW`), with `allowPrivilegeEscalation: false`.

**Why it fails.** Kubernetes validates the propagation mode against the
container's `privileged` field, and a bidirectional mount is allowed
only in a privileged container. The object is rejected at
`kubectl apply`, before any kubelet or runtime sees it. Adding
`SYS_ADMIN` to the capability list does not help: the check is on
`privileged`, not on the capabilities.

**Why the repository does not catch it.** The only manifest check in
the qualification document is `kubectl apply --dry-run=client -f
deploy/k8s/`, and a client-side dry run does not run server-side
validation. No workflow applies these manifests.

**The tension the owner has to settle.** The mount's comment says the
propagation is there so "BPF pins created by the container are visible
to the host and other pods". Host-visible pins need a privileged
container, and privileged undoes the capability minimisation the same
file does deliberately. The two cannot both hold. The options are
roughly: run the DaemonSet privileged and require the node to allow it;
or keep the hardening and give up host-visible pins, which changes what
the XDP deployment model can do.

**Relation to I-012.** Backlog row I-012 is the capability review of
this same manifest (`hostNetwork: true` at `daemonset.yaml:21`,
`NET_ADMIN` at `:41`). This is a distinct, hard-blocking defect in the
same file, not a duplicate.

---

## F16 — the ansible quality gate reported a red CI run as a pass

**Status:** FIXED (P2) — the role now compares the run's conclusion

**What it was.** `ansible/roles/quality/tasks/main.yml` polls each
workflow for the pushed SHA and exited 0 as soon as a run was
*completed*:

    if [ "$status" = "completed" ]; then
      echo "$wf_name -> $conclusion"
      exit 0
    fi

The conclusion was read and echoed, never compared. `ci_failed` was
then derived from the non-zero exit code, and the only non-zero exit was
the poll timeout, so a failed workflow produced `rc 0`,
`ci_failed: false`, and no abort. The failure triage was gated on
`rc != 0` as well, so it never ran for a failed workflow either - only
for a timeout.

**What changed.** The block now exits 0 only when the conclusion is
`success`, and exits 1 otherwise. A failed or cancelled workflow fails
the gate and runs the triage.

**The check.** `tests/ansible_quality_gate.rs` runs the role's own shell
block against a stubbed `gh` that reports a given conclusion, and
asserts the exit code: 0 for `success`, 1 for `failure` and for
`cancelled`. Against the old role it failed all three.

**Reachability.** The `quality` role is wired into the ansible
playbooks, but nothing in the repository's CI runs ansible, so this was
an operator-driven post-push gate, not a release gate. The sibling
`ansible/playbook-quality-replay.yml` already had the correct check
(`if [ "$status" != "success" ]`); the role did not.

---

## F17 — the dashboard's quarantine table had five headers and six cells

**Status:** FIXED (P2) — the surplus cell is gone and the export takes every column

**What it was.** `src/dashboard/static/index.html` declares five
headers (`TARGET IP`, `DETECTION MECHANISM`, `SEVERITY`, `ACTIVE TIER`,
`TIME LOGGED`) and `loadBlocks` gave every row six cells - the surplus
was a trailing em dash. The CSV export declared five column names
(`IP,Module,Severity,Tier,Time`) and sliced four cells out of every
row, so every exported row silently lost its Time. The two placeholder
rows disagreed with each other as well: the static one spanned five
columns and the empty-state one spanned six.

The mismatch came back on every rebuild: the feed is repainted every
5 s, and every repaint built the rows the same way.

**What changed.** The trailing cell is removed, the empty state spans
five columns, and the export takes all five.

**The check.** `the_quarantine_table_and_its_csv_export_agree_on_the_column_count`
counts the table's headers, the cells the row builder produces, the
columns the CSV declares, the CSV's slice bound and the placeholder
spans, and asserts they are the same number. Against the old page it
failed with six cells against five headers.

---

## F18 — `scripts/attack_nexus.py`'s `stop` command does not stop the flood

**Status:** FIXED (P3) — the flood polls the flag the command sets

**What it was.** The interactive `shell` subcommand's `stop` command set
the outer `StopFlag`, but the flood thread polls a different flag: the
thread is started with a fresh `local_stop`, and `stop.clear()` clears
the outer one. The `stop` command is therefore a no-op and the flood
runs to its duration. `quit` has the same problem.

The flood thread now polls the flag the `stop` command sets, so the
command stops the flood instead of running to its duration. The
interactive shell is the only path that reaches this: no workflow and
no Makefile target runs the script.

---

## F19 — two helper scripts read a config example that does not exist

**Status:** FIXED (P3) — the dead read is gone and the other script reads the baseline

**What it was.** `config.prod.toml.example` was renamed to
`config.baseline.toml` and then deleted; it is not on master. Two
scripts still read it:

- `scripts/auth_e2e.py:74` reads it and never uses the result - the
  config it writes is built from scratch a few lines later. The read is
  dead code that happens to raise `FileNotFoundError` first.
- `scripts/final_integration.py:78` reads it and does use it, and its
  `.replace()` targets (`0.0.0.0:7890`, `0.0.0.0:9999`,
  `/var/lib/ramshield/wal`) are not the strings the baseline file
  contains (`127.0.0.1:7890`, `127.0.0.1:9999`, `/tmp/ramshield-wal`),
  so the script fails whichever file it is pointed at.

The dead read in `auth_e2e.py` is gone - the config it wrote was built
from scratch anyway, so the read only ever raised `FileNotFoundError`.
`final_integration.py` now reads `config.baseline.toml` and replaces the
strings that file actually contains (`127.0.0.1:7890`,
`127.0.0.1:9999`, `/tmp/ramshield-wal`).

---

## F20 — `scripts/network_create.py`'s `dual` family produces IPv4 only

**Status:** FIXED (P3) — `dual` emits both families

**What it is.** `build_plan` picks the IPv4 branch when
`family != "v6"`, so `dual` is identical to `v4`: it emits no IPv6
networks at all. The sibling generator `scripts/unique_network_generator.py`
handles `dual` correctly, which is what makes this one an outlier.

`dual` now emits a /24 and a /64 per index, the way the sibling
generator does. `scripts/test_network_tools.py` has a case for it:
against the old generator it fails with `dual produced only {4}`. The
CLI still never asks for `dual` - `main()` always passes `v4` - so this
is a library-API fix.

---

## F21 — `scripts/xdp_netns_sim.py` ignores `--dashboard` in its pre-flight check

**Status:** FIXED (P3) — the pre-flight uses the parsed dashboard URL

**What it is.** `--dashboard` is parsed and stored, and it is used for
the counters, but the pre-flight snapshot request is hardcoded to
`http://127.0.0.1:9999/api/snapshot`. With any other dashboard URL the
pre-flight raises an uncaught `URLError` instead of printing the
intended `FAIL: xdp_active=false`.

The pre-flight now builds the request from the parsed URL, so a
non-default `--dashboard` is used for the snapshot as well as for the
counters.

---

## F22 — the analytics crate declares two dependencies it never uses

**Status:** FIXED (P3) — on `master` as `35f4557` (PR #57, the code commit
`f0b84c6`); both path dependencies are gone.

**What it was.** `crates/ramshield-analytics/Cargo.toml` declared
`ramshield-types` and `ramshield-storage` as path dependencies. No file
under `crates/ramshield-analytics/src/` mentions either: the crate is
`hll.rs`, `cms.rs`, `welford.rs` and a `lib.rs` that only declares the
three modules. The two dependencies cost build time and misled a reader
into thinking the crate is wired to the store or to the shared types.

**What changed.** The `[dependencies]` section is empty. The README's
dependency paragraph now says so, and
`crates/ramshield-analytics/tests/readme_claims.rs` pins the declared
list against `Cargo.toml` - so the two cannot drift apart again. The
guard had to learn one thing: a crate with no dependencies has no
`[dependencies]` section at all, and the old parse asserted one existed.

`Cargo.lock` loses the two edges with it, and the storage crate's README
follows: its "read by" list named `ramshield-analytics` as a reader, and
it no longer is one. The storage crate's own guard caught that the moment
the dependency was dropped.

## F23 — `forecasting.min_entropy` is a config knob nothing reads

**Status:** FIXED — the daemon constructs the forecaster and the forecaster reads the knob.
never reads it.

**Severity:** P3 (hygiene)

`crates/ramshield-config/src/sections/forecasting.rs` has
`min_entropy: f64`, defaulted to 2.0 and validated in
`crates/ramshield-config/src/validate.rs` ("must be finite and >= 0"). No
file under `crates/ramshield-forecasting/src/` mentions it: the entropy
gate in `tick_hw` is a hard-coded event count (`total > 100` before the
Shannon entropy is computed at all), and the entropy value itself is only
an input to the Bayesian likelihood.

An operator who sets `min_entropy` in the configuration gets no error and
no effect. The forecasting README now says that `min_entropy` is
validated but never read.

**Closure criterion.** The forecaster honours `min_entropy`, or the
config crate drops the field and its validation.

## F24 — the removal paths drop the blocked index entry after the shard lock is released

**Status:** FIXED (P3) — `Store::remove` and `evict_batch` now take the
record out through the entry API and do their bookkeeping under the shard
lock.

**What it was.** `Store::remove`
(`crates/ramshield-storage/src/store/access.rs:30`) took the record out
with `self.inner.remove(key)`. DashMap's `remove` releases the shard's
write lock when it returns the value, so the bookkeeping that followed ran
with no lock held:

    let freed = ...;
    self.ram_bytes.fetch_sub(freed, Ordering::Relaxed);
    ...
    if e.value.is_blocked() {
        self.blocked_count.fetch_sub(1, Ordering::Relaxed);
        self.blocked_set.remove(key);
    }

`evict_batch` (`crates/ramshield-storage/src/store/capacity.rs:23`) had the
same shape with `e.remove_entry()`.

**The interleaving.**

1. The record is blocked: the counter is 1, the index holds the key.
2. The removal takes the record out. The lock is released. The counter is
   still 1 and the index still holds the key.
3. The enforcement actor calls `block_ip` for the same key. The key is
   vacant, so `block_ip` publishes a blocked record, increments the
   counter to 2 and inserts the index entry - all under the shard lock,
   which the removal no longer holds.
4. The removal's bookkeeping lands: the counter goes back to 1, and
   `blocked_set.remove(key)` drops the index entry of a record that is
   blocked.

The counter is right; the index is wrong, and it stays wrong until the
next block or unblock for that key. The dashboard's blocked list
(`get_blocked_ips`) and the counter then disagree.

**How narrow it is.** Two things have to line up.

- The removal paths are barely reachable with a *blocked* record.
  `Store::remove` is called only by the WAL replay
  (`crates/ramshield-enforcement/src/replay.rs:143`), at startup, before
  the enforcement actor serves traffic. `evict_batch` is live - the
  detection subnet scan runs the janitor
  (`crates/ramshield-detection/src/subnet_scan.rs:70`) - but it only
  evicts records whose TTL has passed, and the block path clears the TTL,
  so a blocked record is never expired. A blocked record with a TTL only
  exists after a checkpoint restore (`src/engine/checkpoint.rs:259`).
- The removal's window is a handful of instructions between the
  `inner.remove` and the `blocked_set.remove`. The blocker's publish has
  to land inside it, which in practice means the removal thread is
  preempted there.

**What changed.** Both removals go through the entry API
(`self.inner.entry(key)`), so the shard's write guard is held while the
counter, the index and the size accounting are updated, and the record
leaves the map inside that critical section. The subnet-index update
stays after the removal, where it was, so the lock order does not change.

**The test.** `a_re_block_concurrent_with_a_removal_keeps_its_index_entry`
in `crates/ramshield-storage/src/tests/mod.rs` hammers one key from two
threads - one removing, one blocking - and checks after every removal
that a record that is still there and still blocked is in the index. The
removal is the only writer that can drop an index entry and the block
path only ever adds one, so the check is not racy in the thread that runs
it. **It is a race, not a proof:** with 200,000 iterations it caught the
old code in 2 runs out of 5, and it never fails on the fixed code (5 runs,
all green).

## F25 — `ramshield-protocol` declares a dependency on `ramshield-storage` it never uses

**Status:** FIXED (P3) — on `master` as `1315236` (PR #63, the code commit
`4fbf680`); the dependency is out of
`crates/ramshield-protocol/Cargo.toml` and the crate declares only
`ramshield-types`. Found while fixing F22, which was the same defect in
the analytics crate.

**What it is.** `crates/ramshield-protocol/Cargo.toml` declares
`ramshield-storage` as a path dependency. No file under
`crates/ramshield-protocol/src/` mentions `ramshield_storage`: the crate
is the wire protocol - `auth.rs`, the message types and the replay store -
and it uses `ramshield-types`, `serde`, `serde_json`, `hmac`, `sha2`,
`hex` and `ahash`.

The storage crate's own README says so: its "read by" list names
`ramshield-protocol` and notes that the dependency is declared without
being used in source. The same sentence, the same defect.

**Why it matters.** The dependency costs build time and drags the whole
storage crate into the protocol crate's dependency graph. It also makes
the storage README's "read by" list lie in the other direction: it claims
a reader that does not exist.

**What closed it.** The same change F22 got: the path dependency is
dropped, the storage README's "read by" list loses `ramshield-protocol`,
and the two readme guards (`crates/ramshield-storage/tests/readme_claims.rs`
and `crates/ramshield-protocol/tests/readme_protocol.rs`) keep them in
step - the storage guard fails if the README's list and the manifests
disagree, which is exactly how the stale entry was caught.

**Closure criterion.** The dependency is gone, or the crate uses it. **Met.**



## F30 — `block_ip` trusts the caller to pass a blocked fallback

**Status:** FIXED — a clean fallback is refused and publishes nothing
**Files:** `crates/ramshield-storage/src/store/write.rs`,
`crates/ramshield-enforcement/src/service/enforce.rs`
**Severity:** P3 (hardening)

Found by the independent review of the F-14 fix.

### The defect

`Store::block_ip` copies the block state out of the caller's fallback
record and publishes it:

```rust
let state = fallback.block_state.clone();
debug_assert!(
    matches!(state, BlockState::Blocked { .. }),
    "block_ip publishes a blocked record"
);
```

The guard is a `debug_assert!`, so it is compiled out in release. The
same function then does `sync_blocked_index(key, false, true)`, which
inserts the key into `blocked_set` and increments `blocked_count`. A
caller that passed a `Clean` fallback would therefore produce a record
that says clean while the index says blocked - exactly the invariant
the F-9 and F-14 work is about.

The only production caller passes the right thing:
`crates/ramshield-enforcement/src/service/enforce.rs:269` builds
`block_state: BlockState::Blocked { reason, since_ns: now_ns }`
unconditionally, not under `debug_assertions`. So this is not a live
defect; it is a contract that only a debug build checks.

### Required fix

1. Either derive the published state inside the store, so it cannot be
   wrong, or make the check a real error in release.
2. Keep a test that a clean fallback cannot produce a blocked index
   entry with a clean record.

### Closure criterion

The release build cannot publish a clean record for a key the blocked
index lists as blocked.

---

## F29 — The Subnet Stress Pin gate asserts against a fixed four-second sleep

**Status:** FIXED (P2) — the script waits for the drain instead of racing it.
**Files:** `scripts/subnet_stress.py`
**Severity:** P2 (CI)

Found when the master workflow was checked after the F-2 merge: the
Subnet Stress Pin run on `4b88864` was red, and the commit it ran was
docs only.

### The defect

`scripts/subnet_stress.py` sent 4,800 events, slept four seconds, and
then asserted `ips_tracked >= 800`. The daemon flushes the detection
aggregates on a 250 ms tick and scans the subnet table on a ~500 ms tick,
so the four seconds is a race against the runner, not a bound on the
work. On the failing run the daemon had ingested 308 of the 4,800 events
in one batch and had tracked 206 IPs, against the threshold of 800; the
run before it on the same tree had ingested all 4,800 and tracked 2,395.

The same script is the "Subnet Stress Pin" job on every master push and
on the nightly schedule, so a busy runner can turn the master red for a
reason that has nothing to do with the change under test.

### Required fix

1. Wait for the counters to settle before asserting on them, with a
   deadline, instead of sleeping a fixed interval.
2. Do not make the event count the target: the daemon drops events under
   pressure, so "ingested == sent" is not reachable on a loaded runner.
   The master's own run on `67c7ccd` ingested 1,310 of 4,800 events and
   still had to pass.

### Closure criterion

The gate asserts on a settled daemon, not on a sleep.

---

## F26 — `FAIL_SAFE_BEHAVIOR.md` claims a global fail-closed posture the dataplane does not have

**Status:** FIXED (P2) — the document states each subsystem's own
posture, and `tests/fail_safe_claims.rs` pins it to the source.
**Files:** `FAIL_SAFE_BEHAVIOR.md`, `crates/ramshield-xdp/ramshield-xdp-bpf/src/main.rs`,
`tests/fail_safe_claims.rs`
**Severity:** P2 (doc)

Found by the 2026-10-22 master audit.

### The defect

`FAIL_SAFE_BEHAVIOR.md:7` says:

> RamShield follows a **fail-closed** security posture across all production
> code paths. The guiding principle is: **any uncertainty or failure
> defaults to blocking traffic, never allowing it through.**

The dataplane does not behave that way, and the document says so itself
nine lines later: `### 1. Kernel-Level Fail-Open (XDP Dataplane)`
(`FAIL_SAFE_BEHAVIOR.md:11`). The BPF program agrees with the section, not
with the summary: `crates/ramshield-xdp/ramshield-xdp-bpf/src/main.rs`
returns `XDP_PASS` on the parse-failure and unsupported-packet paths
(lines 130, 192, 197, 236) and `XDP_DROP` only on the paths it can
classify (182, 189, 226, 233).

Fail-open on a malformed or unsupported packet is the right operational
choice: dropping every packet the parser cannot read would be a
self-inflicted outage. The problem is the global sentence, which an
operator can read as "RamShield blocks on any uncertainty".

The document also dates itself: "All findings and implementations are
based on the code audit performed on 2026-10-13" (line 3), so it predates
the current dataplane.

### Required fix

1. Restate the posture per subsystem: what happens on a parse failure, a
   map failure, a worker failure, a storage failure and a shutdown.
2. Delete the global "never allowing it through" sentence.
3. Re-point the document at the current code, not the 2026-10-13 audit.

### What landed

The summary now says there is no single fail-closed posture and lists what
each subsystem does: the XDP dataplane fails open on packets it cannot
parse or classify, a *map update* failure still returns `Err` and keeps
the caller's in-band state, the authentication path rejects a wrong
signature, an out-of-skew timestamp and a replayed frame, the store
refuses a net-new key under the RAM budget rather than evicting a live
one, and the native capture path degrades the service and keeps serving.

The document's provenance line ("based on the code audit performed on
2026-10-13") now says it was last checked against the source on
2026-10-22, the dead `.hermes/plans/...` link points at the current
independent review, the gate table says the dataplane is fail-open by
design, and the cross-crate action item says the check is that each
subsystem states its own behaviour, not that they agree.

`tests/fail_safe_claims.rs` pins the four claims to the source: the BPF
program's `XDP_PASS` and `XDP_DROP` paths, the auth module's three
rejections, the store's `CapacityExceeded`, and the capture path's
degraded health reason and exported gauge. It fails if the global
sentence comes back.

### Closure criterion

No sentence in the document claims a behaviour the code does not have.

---

## F27 — `scripts/prod_smoke.sh` claims checks it does not perform, and it deletes an env-overridable path

**Status:** FIXED — the script does what it claims and deletes only its own directory
**Files:** `scripts/prod_smoke.sh`
**Severity:** P1 (data loss)

Found by the 2026-10-22 master audit.

### The defect

The header advertises a coverage the script does not have
(`scripts/prod_smoke.sh:11`):

> unblock → verify absent → CIDR block → verify → TTL expiry → verify

What the script actually does:

- **Unblock**: it sends `unblock_ip` and checks the response says
  `unblock queued` (`:139-140`). It never checks that the IP is gone.
- **CIDR**: there is no CIDR request in the script at all. The blocks it
  sends are `block_ip` for `203.0.113.7` and `203.0.113.99` (`:112`,
  `:154`).
- **TTL expiry**: nothing waits for a TTL to expire. The TTLs it sets are
  300 seconds (`:112`, `:154`) and the script does not outlive them.

The cleanup is the sharper edge. `WAL_DIR` comes from the environment
(`:20`, `WAL_DIR="${WAL_DIR:-/tmp/ramshield-prod-smoke-wal}"`) and the
script then runs `rm -rf "$WAL_DIR"` (`:37`) before it starts. An operator
who has `WAL_DIR` exported at a real data directory loses it, and the
loss happens before the test says anything.

### Required fix

1. Refuse to delete a directory that is not a fresh temporary one: create
   a `mktemp -d` directory by default, and refuse to `rm -rf` a path that
   does not carry the smoke test's own name in it.
2. Verify the unblock: after the unblock, assert the IP is absent.
3. Send and verify a CIDR block.
4. Wait for a short TTL to expire and assert the block is gone.
5. Fail the script when the resulting state differs from the expectation.

### Closure criterion

Every scenario the header advertises is performed end to end and asserted,
and the script cannot delete a directory it did not create.

---

## F28 — The advanced CI workflow runs the benchmarks as tests and masks the benchmark build failure

**Status:** FIXED — the benchmark build failure fails the job
**Files:** `.github/workflows/advanced.yml`
**Severity:** P2 (CI)

Found by the 2026-10-22 master audit.

### The defect

The primary CI workflow deliberately does not use `--all-targets` for its
test step, because the repository has already had a long-running
benchmark harness executed by the test command. The advanced workflow
still does exactly that:

- `.github/workflows/advanced.yml:39` — `cargo build --all-targets --verbose`
- `.github/workflows/advanced.yml:40` — `cargo test --all-targets --verbose`

and its benchmark job masks the failure that would tell anyone the
benchmarks no longer compile:

- `.github/workflows/advanced.yml:74` — `cargo bench --no-run || true`

`|| true` makes a broken benchmark build invisible: the job is green and
nothing says the benchmark targets stopped compiling.

### Required fix

1. Use the primary workflow's test-target policy in the advanced
   workflow.
2. Run the benchmarks in their own bounded job.
3. Remove the blanket `|| true`, or record a narrowly justified skip for
   the one target that cannot build.

### Closure criterion

No CI job runs the benchmark harness as a test, and a benchmark build
failure is visible.

---

## F31 — The dashboard's rate readouts read keys the SSE writer never sets

**Status:** FIXED — the rate readouts read the keys the stream writes
**Files:** `src/dashboard/static/index.html`
**Severity:** P2 (operator-visible)

Found by the 2026-10-22 full-codebase audit.

### What it is

`index.html:525` writes `drops_total_pps`, `ingest_total_pps` and `shed_total_pps` into `dSlots`, but the readouts at `:534`, `:540` and `:542` read `drops_pps`, `ingest_pps` and `shed_pps`. `fmt` returns `'0'` for an undefined key, so every rate on the dashboard is 0 at any traffic. The `ewma` table at `:555` defines the short names, but `:525` updates the long ones.

**Why it matters.** An operator watching a live attack sees `0 drops/s` and `0 events/s` on the headline KPIs and the overview rows.

### Closure criterion

The rate values change when the counters change, and no key is read that is not written.

---

## F32 — The dashboard's auth counters go to a Metrics instance that is never exported

**Status:** FIXED (P1) — the auth state shares the engine's metrics instance, so the
CSRF and lockout counters reach the exporter.
**Files:** `src/dashboard/mod.rs, src/dashboard/auth.rs`
**Severity:** P1 (control-plane telemetry)

Found by the 2026-10-22 full-codebase audit.

### What it is

`src/dashboard/mod.rs:43` passes a fresh `Arc::new(Metrics::new())` into `AuthState::new`. The exported instance is `state.engine.metrics` (`:154`). The CSRF and lockout counters are written to the auth instance (`mod.rs:346`, `:360`, `auth.rs:417`, `:443`) and the exporter reads them from the engine instance (`prometheus.rs:482-489`, `:203-211`).

**Why it matters.** `ramshield_csrf_blocked_total`, `ramshield_csrf_allowed_total`, the lockout counter and the Argon2 wait gauge stay 0 in `/metrics` forever, so control-plane abuse is invisible to Prometheus.

### Closure criterion

After one blocked CSRF request and one lockout the corresponding series is greater than zero.

---

## F33 — The shipped server ConfigMap sets an ipc.max_connections the daemon refuses to start with

**Status:** FIXED (P1) — `deploy/k8s/configmap.yaml` sets `max_connections =
1024`, the value the validator accepts.
**Files:** `deploy/k8s/configmap.yaml`
**Severity:** P1 (deployment)

Found by the 2026-10-22 full-codebase audit.

### What it was

`deploy/k8s/configmap.yaml:20` set `max_connections = 4096`; `crates/ramshield-config/src/validate.rs:313-315` rejects anything above 1,024. The node-guard ConfigMap used 1,024 (`configmap-node.yaml:20`). The auditor reproduced it: the extracted ConfigMap, with only the WAL dir and ports rewritten, made the binary exit 1 with `ipc.max_connections should not exceed 1,024`.

**Why it mattered.** `kubectl apply -f deploy/k8s/` put the server pod into CrashLoopBackOff at boot. `scripts/verify_release.sh` only checked the loopback addresses in that file, so the release gate passed.

### Closure criterion

The shipped ConfigMap passes `Config::validate()`, and a gate asserts it.

---

## F34 — The Kubernetes probes target the pod IP while the daemon binds loopback

**Status:** FIXED — the probes address the loopback bind the daemon actually uses
**Files:** `deploy/k8s/deployment.yaml, deploy/k8s/daemonset.yaml, deploy/k8s/configmap.yaml`
**Severity:** P1 (deployment)

Found by the 2026-10-22 full-codebase audit.

### What it is

Both ConfigMaps set `[dashboard] http_addr = "127.0.0.1:9999"` (`configmap.yaml:24`, `configmap-node.yaml:25`) and `src/dashboard/mod.rs:80` binds that address verbatim. The probes in `deployment.yaml:70-93` and `daemonset.yaml:77-88` are `httpGet` on the named port with no `host:`, so the kubelet connects to the pod IP. The manifests also disagree with each other: `daemonset.yaml:56` says the dashboard binds `0.0.0.0:9999` and `networkpolicy.yaml` opens 9999 to other namespaces.

**Why it matters.** The pod never passes its startup probe and is killed and restarted, so the control plane never comes up on the shipped manifests even once F-33 is fixed.

### Closure criterion

`kubectl apply` of `deploy/k8s` yields a pod that becomes Ready with no restarts.

---

## F35 — The ramshield-cli binary is a stub, yet the release ships it and the installer requires it

**Status:** FIXED — the pre-deletion implementation is restored and works
**Files:** `src/cli.rs`
**Severity:** P1 (operator tooling)

Found by the 2026-10-22 full-codebase audit.

### What it is

`src/cli.rs` is five lines: it prints `RamShield CLI` and exits 0. `Cargo.toml:35-37` declares the binary, `release.yml:50,56` tars it into both archives, `scripts/install.sh:44` fails the install when it is missing and `:52` installs it, and `README.md:69-80` plus `docs/OPERATIONS.md:16-52` document `status`, `stats`, `check`, `info`, `block`, `unblock` and `unblock-cidr`. The 231-line implementation was deleted in `a6fe284` and re-added as the stub in `918732b`; `git show 59d4bc6:src/cli.rs` is the pre-deletion version. `scripts/upgrade_qualification.sh:70-85` drives the qualification with exactly those subcommands and greps for `"blocked": true`.

**Why it matters.** `ramshield-cli block 1.2.3.4 --reason manual --ttl 300` prints `RamShield CLI` and exits 0, so an operator believes a block was applied when nothing happened.

### Closure criterion

Either the CLI works as documented, or the binary, its packaging, the installer check and the documented commands are removed in the same change.

---

## F36 — The 403 rejections are counted but never exported

**Status:** FIXED — the 403 rejections are exported
**Files:** `crates/ramshield-metrics/src/prometheus.rs, src/ipc/server/handlers.rs`
**Severity:** P2 (metrics)

Found by the 2026-10-22 full-codebase audit.

### What it is

`src/ipc/server/handlers.rs:59-60` increments `inc_rejected(1)` and `inc_ipc_authz_rejections(1)` on a 403. The exporter publishes `ramshield_ipc_event_drops_total`, `ramshield_ipc_auth_rejections_total` and `ramshield_ipc_rejected_connections_total`, and no series for `ipc_authz_rejections`, so the breakdown the field's own doc comment promises (`crates/ramshield-metrics/src/lib.rs:37-43`) does not sum to the aggregate.

**Why it matters.** The gap between `events_rejected_total` and its exported breakdown equals the 403 count, and no series explains it.

### Closure criterion

The exported breakdown sums to the aggregate, and 403s have their own series.

---

## F37 — The store-RAM refusals are counted but no surface exposes them

**Status:** FIXED — the store's RAM refusals are exported and shown
**Files:** `crates/ramshield-detection/src/flush.rs, crates/ramshield-metrics/src/prometheus.rs`
**Severity:** P2 (metrics)

Found by the 2026-10-22 full-codebase audit.

### What it is

`crates/ramshield-detection/src/flush.rs:218-225` increments `capacity_exceeded_count` and `capacity_exceeded_ips`; neither field appears in the exporter, and the dashboard's "Cap Exceeded" row is the literal `'n/a'` (`src/dashboard/static/index.html:544`). `docs/development/OPEN_FINDINGS.md` states the refusal "is already observable (`capacity_exceeded_count` / `capacity_exceeded_ips`)", which the code contradicts.

**Why it matters.** Under the RAM budget an IP is dropped from tracking entirely and every displayed counter stays flat, while the ledger says the event is visible.

### Closure criterion

The refusal appears in `/metrics` and on the dashboard row that exists for it.

---

## F38 — The XDP-divergence stale flag is erased 250 ms after it is raised

**Status:** FIXED — the age tick no longer clears the flag a failed apply raised
**Files:** `crates/ramshield-enforcement/src/service/run.rs, crates/ramshield-metrics/src/xdp_health.rs`
**Severity:** P2 (health signal)

Found by the 2026-10-22 full-codebase audit.

### What it is

`crates/ramshield-enforcement/src/service/enforce.rs:333` calls `mark_xdp_projection_stale()` on a failed XDP apply. The 250 ms tick in `service/run.rs:34,63` calls `tick_reconcile_age`, which recomputes the flag in `xdp_health.rs:77-82` as `(last == 0 || age > XDP_PROJECTION_STALE_SECS)` - zero whenever the last reconcile worked recently, which is the normal case.

**Why it matters.** Apply failure sets stale for a fraction of a second; the health computation in `src/engine/mod.rs:232` never observes the divergence. Only the separate failure counter remains as evidence.

### Closure criterion

After a failed apply the stale flag stays set until a successful reconcile.

---

## F39 — The "Drops vs Ingress" chart never plots drops

**Status:** FIXED — the chart plots the real drop rate
**Files:** `src/dashboard/static/index.html`
**Severity:** P2 (operator-visible)

Found by the 2026-10-22 full-codebase audit.

### What it is

The panel is titled "Packet Pipeline Velocity (Drops vs Ingress)" (`index.html:346`) and the only telemetry push is `:476` `pushTelemetry(0, ingress)` - the drop argument is a literal 0. The SSE path has the drop totals in `d.xdp.v4_drops_total` / `v6_drops_total` (`:520`) and does not use them.

**Why it matters.** The red drop series stays glued to the axis while the ingress series moves, contradicting the panel's own title.

### Closure criterion

The red series tracks the XDP drop counters.

---

## F40 — The blocklist gauge compares a lifetime counter with a hardcoded capacity

**Status:** FIXED — the blocklist row is labelled as what it shows
**Files:** `src/dashboard/static/index.html, src/dashboard/mod.rs`
**Severity:** P2 (operator-visible)

Found by the 2026-10-22 full-codebase audit.

### What it is

The row is labelled "Blocklist" with `0 / 102400` (`index.html:253`) and rendered as `fmt(snap.blocked_total,0)+' / '+fmt(x.map_capacity,0)` (`:541`). `snap.blocked_total` is the cumulative `blocks_total` counter, not current map occupancy, and `x.map_capacity` is the literal `102400` (`src/dashboard/mod.rs:502`) while the real cap is `BLOCKLIST_CAP` (`crates/ramshield-xdp/ramshield-xdp-bpf/src/main.rs:89-109`).

**Why it matters.** After 102,400 lifetime blocks the bar pins at 100% forever, and with a non-default `BLOCKLIST_CAP` the denominator is wrong too.

### Closure criterion

The row shows current map occupancy, or is labelled as what it actually is.

---

## F41 — Two dashboard rows are overwritten with `n/a` by the SSE path

**Status:** FIXED — the SSE path no longer overwrites the two rows
**Files:** `src/dashboard/static/index.html`
**Severity:** P2 (operator-visible)

Found by the 2026-10-22 full-codebase audit.

### What it is

The 5-second refresh computes the "Subnet Bitmap" and "Cap Exceeded" rows (`index.html:478`), but the 100 ms SSE path `renderStream` (`:544`) sets both to the literal `'n/a'`, and it runs 10 times a second.

**Why it matters.** The "Subnet Bitmap" row shows a number for at most one frame after each refresh and `n/a` the rest of the time - effectively never visible.

### Closure criterion

The rows hold a value continuously.

---

## F42 — The release gate greps for a dependency-audit flag the workflow no longer uses

**Status:** FIXED — the gate checks the flags the workflow runs
**Files:** `scripts/verify_release.sh`
**Severity:** P2 (CI)

Found by the 2026-10-22 full-codebase audit.

### What it is

`scripts/verify_release.sh:14` requires the workflow to contain `cargo audit --locked`; `.github/workflows/dependency_audit.yml:20` runs `cargo audit --no-yanked`. The auditor ran the gate on the audited tree: `FAIL dependency audit gate` and `release gate: 27 pass, 1 fail` (exit 1). `deny.toml:4` sets `yanked = "deny"`, so the workflow's flag is the deliberate policy and the script's expectation is the stale half.

**Why it matters.** Every operator who runs the documented release gate gets a non-zero exit and a false dependency-audit failure, which erodes trust in the other 27 checks.

### Closure criterion

`scripts/verify_release.sh --static` exits 0 on master, and the workflow's flags match the script's expectation.

---

## F43 — The installer writes a unit that grants the daemon write access it must never have

**Status:** FIXED — the installer no longer grants /proc/sys write access
**Files:** `scripts/install.sh, deploy/systemd/ramshield.service`
**Severity:** P2 (security)

Found by the 2026-10-22 full-codebase audit.

### What it is

`scripts/install.sh:87` writes a `ReadWritePaths=` line for three `/proc/sys` tunables, and systemd's `ReadWritePaths=` is additive, so the unit the installer writes grants them. The tracked unit `deploy/systemd/ramshield.service:32` grants no `/proc/sys` write path, and `src/engine/synproxy.rs:150-151` states the daemon never writes `/proc/sys` and must never try.

**Why it matters.** The two ways to install RamShield produce different sandboxes, and the looser one is the default the installer writes.

### Closure criterion

`install.sh` and `deploy/systemd/ramshield.service` produce equivalent units, and neither grants `/proc/sys` write access.

---

## F44 — The Ansible deploy role renders into a directory that does not exist and applies a stub template

**Status:** OPEN
**Files:** `ansible/roles/deploy/tasks/main.yml, ansible/roles/deploy/templates/k8s-deployment.yaml.j2`
**Severity:** P2 (deployment)

Found by the 2026-10-22 full-codebase audit.

### What it is

`ansible/roles/deploy/tasks/main.yml:57` renders into `{{ project_dir }}/deploy/rendered/deployment.yaml`; no task creates that directory, and `:61` applies whatever is in it. The template `k8s-deployment.yaml.j2` is 22 lines: a Deployment with an image tag (default `latest`) and `--config /etc/ramshield/config.toml`, and no volumes, ConfigMap mount, WAL PVC, probes, secret reference or env.

**Why it matters.** The k8s deploy path cannot work: the rendered Deployment mounts no config, the `ramshield-admin` Secret is absent, and the static manifests in `deploy/k8s/` are never applied. The default `latest` tag also violates the pinned-tag policy `scripts/verify_release.sh:12` enforces.

### Closure criterion

The k8s deploy path applies the same manifest set as `deploy/k8s/` with a pinned image tag, and the rendered directory is created by the role.

---

## F45 — Two exported series miss the `ramshield_` namespace

**Status:** FIXED — every exported series carries the namespace
**Files:** `crates/ramshield-metrics/src/prometheus.rs`
**Severity:** P3 (metrics)

Found by the 2026-10-22 full-codebase audit.

### What it is

`prometheus.rs:203` exports `auth_lockout_total` and `:209` exports `auth_verification_wait_ms`; every other series in the file carries the `ramshield_` prefix.

**Why it matters.** The two series collide with another exporter's names in a shared Prometheus.

### Closure criterion

Every exported series is namespaced.

---

## F46 — The pipeline stage bars mix units

**Status:** FIXED — the pipeline stages share their units
**Files:** `src/dashboard/mod.rs`
**Severity:** P3 (display)

Found by the 2026-10-22 full-codebase audit.

### What it is

`src/dashboard/mod.rs:489` emits the cold stage with `unit: "IPs"` and `ingress: snapshot.pipeline.batched`, which is `metrics.batches_total` (a count of flushes) while `drops` is an IP count; `:490` mixes the cumulative `promoted`/`blocked` with `merged`, which is `stats.ips_tracked`, a current gauge. The frontend derives a utilisation percentage across the mixed units (`index.html:507-509`).

**Why it matters.** The stage bars are not meaningful: the numbers contradict the labels.

### Closure criterion

Each stage's ingress and drops share a unit.

---

## F47 — The quarantine table fabricates the severity and tier of every row

**Status:** FIXED — the quarantine table shows the record's reason
**Files:** `src/dashboard/static/index.html`
**Severity:** P3 (display)

Found by the 2026-10-22 full-codebase audit.

### What it is

`index.html:484` renders every row with the constants `'CRITICAL'` and `'Tier 3 (XDP)'`, while the record carries a real `reason` (`crates/ramshield-metrics/src/types.rs`, filled by `record_block_ip` in `history.rs:49-64`).

**Why it matters.** The table misrepresents the reason each block was issued for.

### Closure criterion

The severity column shows the record's reason, and the tier column no longer claims a tier the record does not carry.

---

## F48 — `/metrics` bypasses the session auth that guards every other route

**Status:** OPEN
**Files:** `src/dashboard/auth.rs`
**Severity:** P3 (exposure)

Found by the 2026-10-22 full-codebase audit.

### What it is

`src/dashboard/auth.rs:334` exempts `/metrics` from `require_auth` together with `/healthz`, `/livez` and `/login`. The exporter emits no per-IP data, so the exposure is operational counters only, but `deploy/k8s/networkpolicy.yaml` admits 9999 from selected namespaces.

**Why it matters.** An unauthenticated metrics endpoint on a port the network policy opens to other namespaces.

### Closure criterion

The exemption is documented as intentional, or `/metrics` is restricted.

---

## F49 — The backlog gate's open-row parser never matches BACKLOG.md

**Status:** FIXED — the gate's parser reads the table BACKLOG.md has
**Files:** `backlog.yml`
**Severity:** P3 (CI)

Found by the 2026-10-22 full-codebase audit.

### What it is

`.github/workflows/backlog.yml:77-88` counts rows with an awk that looks for a `## P0` heading and a trailing `| OPEN` cell. `BACKLOG.md` has no such heading - priority is a column (`:71`) and the status cell is free prose (`:75`). The auditor ran the workflow's exact awk on the committed file: every row yields an empty status cell, so the step summary prints "OPEN P0 rows: 0" and publishes those zeros.

**Why it matters.** The Backlog Gate's own summary misstates the open backlog: `A-038` is OPEN while the gate reports zero open P0/P1 rows.

### Closure criterion

The parser reads the priority column and the status cell of the real table format, and the reported counts match a hand-count.

---

## F51 — The qualification docs tell operators to run a cargo-audit invocation that does not exist

**Status:** FIXED — the docs name the flags the workflow runs
**Files:** `docs/PRODUCTION_QUALIFICATION.md, docs/PRODUCTION_0.4.0.md`
**Severity:** P3 (docs)

Found by the 2026-10-22 F-42 review.

### What it was

`docs/PRODUCTION_QUALIFICATION.md:10` and `docs/PRODUCTION_0.4.0.md:99` listed `cargo audit --locked` in the operator-facing qualification checklist. `--locked` is not a cargo-audit flag: cargo-audit 0.22.2 rejects the invocation with `error: unexpected argument '--locked' found` and exits 2. The stale expectation came from the same 2026-10-06 change that left `scripts/verify_release.sh` auditing `--locked` while the workflow ran `--no-yanked` (F-42).

**Why it mattered.** An operator following the qualification checklist ran a command that cannot succeed, and the failure looks like a broken toolchain rather than a wrong flag.

### Closure criterion

The checklist names the invocation the dependency-audit workflow actually runs.

---

## F50 — Three scheduled workflows can never run

**Status:** FIXED — the three dead scheduled workflows are deleted
**Files:** `helper.yml, error_healer.yml, health_dashboard.yml`
**Severity:** P3 (CI)

Found by the 2026-10-22 full-codebase audit.

### What it is

`.github/workflows/helper.yml:11`, `error_healer.yml:11` and `health_dashboard.yml:11` gate their job on `if: github.ref == 'refs/heads/feature/ramshield-advanced'`. No such branch exists on the remote, and for a `schedule` event GitHub evaluates `github.ref` as the default branch, which is `master`. The same dead branch gates `advanced.yml:5`.

**Why it matters.** The `*/5`, `*/30` and `*/15` cron jobs are skipped on every tick and the three agent scripts never run in CI; the only signal is the absence of activity.

### Closure criterion

The scheduled workflows either target the default branch or are deleted.

---
