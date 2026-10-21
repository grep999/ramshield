# OPEN IMPLEMENTATION WORK — tracked findings

The durable task list for the issues that still need implementation. Every
entry states the current status, the required fix, and the closure criterion,
so a later pass can pick it up without re-deriving the intent.

**Three numbering namespaces.** The **roadmap's** `F-01..F-08`
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
| F5 | Unauthenticated local control plane (IPC + dashboard) | P1 on `e5caac1` | **CLOSED** — `b1a9e9e` | no |
| F3 | Native ingest drops invisible | P2 | **CLOSED** — `60ce0f1` | no |
| F4 | XDP LRU eviction residual not stated | P3 (doc) | **CLOSED** — `79f4eac` | no |
| **F1** | **CGNAT shared-memory lookup semantics** | **P1/P2** | **CLOSED** — `d1f4a81`, see `docs/development/CGNAT_SHM_LOOKUP_FIXES.md` | no — the ABI question dissolved (see below) |
| **F1a** | **A /0 rule is not representable in the SHM table** | **P2** | **OPEN** — a design decision for the owner, not a defect | no — the three options are recorded below |
| **F2** | **Native capture worker lifecycle** | **P2** | **OPEN** | **yes — restart vs. degrade vs. refuse readiness** |
| F6 | `BACKLOG.md` is not a reliable master ledger | P2 (doc) | CLOSED — the owner lifted the fence and the file was re-pointed at `master` | no — the fence was lifted by the owner on 2026-10-21 | rule |
| F7 | Performance/RSS claims need workload evidence | P2 (process) | OPEN | yes — the release-claims policy |
| **F9** | **Enforcement's block path is a lost update against detection** | **P1** | **FIXED** on `fix/f9-enforcement-lost-update` — the store has block-only mutators under one shard lock | no — the fix shape was mechanical |
| F8 | The audit was not locally reproduced | n/a (method) | not actionable | no |
| **F10** | **The IPC server MACs a key-sorted payload, not the client's bytes** | **P1** | **OPEN** — found by independent review while re-verifying I-024; the README is corrected, the server behaviour is not | yes — the fix shape is a canonicalisation decision, and it changes the wire contract |
| F11 | Crate README documentation drift (54 claims) | P2 (docs) | OPEN — 2 of 54 fixed; the rest are listed in `docs/development/DOC_DRIFT_SWEEP.md` | no — per-file documentation edits |

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
- The shape of the tri-state result is a **published C ABI change**. It needs
  an owner compatibility decision before the ABI shape is committed — this is
  the one place where the review flagged a decision that is genuinely the
  owner's to make, not a mechanical choice.
- The Rust implementation in `crates/ramshield-cgnat/src/shm.rs` is a separate
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

**Status:** OPEN
**Files:** `src/engine/native.rs`, `src/engine/boot.rs`
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

1. Handle every worker-spawn result explicitly.
2. Track expected, started, running and failed workers.
3. Define bounded restart behavior or a clear failure policy.
4. Expose native capture status through existing health/metrics mechanisms.
5. Decide whether total capture failure should refuse readiness or mark the
   service degraded.
6. Test partial startup failure, total failure, runtime exit, shutdown and
   restart exhaustion.

### Closure criterion

Capture cannot silently disappear while the service continues to imply that
the configured subsystem is functioning.

### Notes for whoever picks it up

- This is an **observability and lifecycle defect** in the optional
  native-ingest path, **not** evidence of a memory-safety vulnerability.
  `native_ingest.enabled` defaults to `false`, so the blast radius is limited
  to deployments that enable it.
- It should be prioritized **after** the CGNAT fix (F1).
- Item 5 is a **policy decision for the owner**: restart the worker, fail the
  feature, or degrade the service. The in-repo precedent is the detection
  path, which refuses readiness when its workers fail to start — following
  that precedent is a defensible default, but it changes the daemon's startup
  contract and so is worth an explicit decision rather than an inference.
- The metrics surface added by `60ce0f1` (`ramshield_native_ingest_drops_*`)
  is the natural place to hang a capture-health gauge.

---

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

**What was done:** the audit-branch material moved verbatim into the new
`docs/development/AUDIT_FINDINGS_BACKLOG.md` (verified byte-identical);
`BACKLOG.md` was re-pointed at `master` and the assessed SHA; all 33 rows
were re-stamped — 4 as `CLOSED (commit)`, 3 as `PASS (exact SHA)` on the
current SHA with the evidence named, the rest as `PASS (historical)` or
`OPEN` with the verification state stated; a new "Open on master" section
records the 11 items the file did not mention; and the 100/100 gate is
bound to a SHA with the note that it has never been satisfied on any
single SHA.

**One review claim was corrected before it was written down.** The review
stated that the parked branch `f75abb5` "contains no
`crates/ramshield-mesh` source change". That is wrong: the branch carries
625 changed lines in `crates/ramshield-mesh/src/transport.rs` plus test
changes. The conclusion the reviewer drew — that none of it is on master —
is correct, because a diff against master shows the branch is behind
master. The prose in `BACKLOG.md` states the accurate version.

The checked-in header still describes `audit-branch` as the target branch and
cites three superseded SHAs. PASS rows are pinned to a SHA that is no longer
master, so the file is useful as historical task context but unsafe as the
sole answer to "what is proven on current master?".

**Required fix:** re-point the header at `master` and the current exact SHA;
split the historical `audit-branch` notes into a clearly labelled appendix;
re-stamp every current PASS row with its exact SHA and workflow URLs.

**Not started:** a standing rule fences this file — it must not be
overwritten, and `docs/development/AUDIT_FINDINGS_BACKLOG.md` is the separate
home for new work. This entry needs that fence lifted before it can be
actioned.

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

**Not started:** the audit's claim that PR `#34` is a draft describing 50 MB
as a target was **UNVERIFIED** by the review (a remote-only artifact). The
reviewer did not contradict it but did not confirm it.

---

## F9 — Enforcement's block path is a lost update against detection

**Status:** FIXED on `fix/f9-enforcement-lost-update` — the store has two
block-only mutators, each under one shard lock, each touching only
`block_state`. The block path and the unblock path both use them, so
neither can publish a stale snapshot of detection's counters. The
property test (`a_block_never_makes_the_detection_counters_go_backwards`)
fails against the old two-lock pattern and passes with the mutators.

**Status of the original report:** found while re-verifying backlog row
I-011, and **not** what that row describes.

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

**What was done about it.** `Store::block_ip` and `Store::unblock_ip` in
`crates/ramshield-storage/src/store/write.rs`:

- `block_ip` changes the block state of a live record and leaves every
  stat field alone. With no live record it publishes the caller's
  record directly in the blocked state, so a clean state is never
  published for an IP the enforcement actor holds blocked.
- `unblock_ip` clears the block state of a live record and leaves the
  stat fields alone. A missing or expired record is left alone: an
  unblock must not create a record for an IP there is no state for.
- Both keep the store's `blocked_count` / `blocked_set` index in step
  under the same lock, and keep the TTL semantics the old
  `insert(ttl = None)` had: a blocked record does not expire under the
  store's passive expiry.

The enforcement actor's block path and unblock path
(`crates/ramshield-enforcement/src/service/enforce.rs`) now call them.
The unblock path had the same window and is fixed by the same change.

Five tests in the storage crate pin it. The property test
(`a_block_never_makes_the_detection_counters_go_backwards`) was proven
load-bearing: reimplementing `block_ip` as the old `get()` + `insert()`
makes it fail.

**Required test.** A test that races a detection `update_ip` against the
block path and asserts the stat fields are not rolled back. It must fail on
the current code.

**Not started, and not a quick fix.** This touches the block write path and
its XDP projection ordering, so it is a medium-risk change, not a bounded
one-file fix. It is recorded here rather than patched in the same pass as
the I-013/I-020/I-024 closures.

**A second, related observation** (policy, not a defect): an IP refused by
the RAM budget is dropped from detection entirely
(`crates/ramshield-detection/src/flush.rs:216-239`, `stored == false` ->
`cold_skipped`), so its `sample_count` restarts at 0 and the CUSUM warmup
never completes. The refusal is already observable
(`capacity_exceeded_count` / `capacity_exceeded_ips`); whether a
capacity-refused IP should fail open or fail closed is a posture decision
and is not proposed here.

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

**Status:** OPEN. The documentation is corrected; the server behaviour is
not, because the fix is a wire-contract decision.

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

**Why nothing caught it.** No test in the tree signs a multi-key frame
through the server. `src/ipc/server/tests/mod.rs` uses a single-key payload
(`{"type":"get_status"}`), where sort order cannot differ, so the test
passes either way and pins nothing.
`crates/ramshield-protocol/tests/replay.rs` calls `auth::verify` directly,
bypassing the server's re-serialisation entirely.

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

---

## F12 — two compiled binaries are tracked in the repository

**Severity:** MEDIUM (hygiene, not a runtime defect)

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

**What is still open.** `master` still carries both files until this is
merged. Removing them from `master` is the owner's call, like every other
change to that branch.

---

## F13 — the workspace does not compile on Windows

**Severity:** MEDIUM (build portability, not a runtime defect)

`OpenOptions::mode` is a Unix-only API. It is used unguarded in
`src/engine/checkpoint.rs:66`, `crates/ramshield-cgnat/src/shm.rs:133`
and `crates/ramwal/src/wal.rs:542`, so the crate fails to compile for a
Windows target:

```
error[E0599]: no method named `mode` found for mutable reference
              `&mut std::fs::OpenOptions` in the current scope
```

`.github/workflows/advanced.yml` runs a `windows-latest` job, so that job
is red on every branch that has run it — including master. The macOS job
passes, because `mode` exists there.

**What was done about it.** Nothing in the code: the finding is recorded
here. The branch that carries it does not touch those files, so the
Windows job fails on it exactly as it fails on master.

**What is still open.** The fix is not a one-liner in the general case.
`mode(0o600)` is how the state files and the shared-memory file are
restricted to their owner, and Windows has no equivalent mode bit: the
options are (a) a `#[cfg(unix)]` guard with a documented Windows
alternative, (b) `set_permissions` after the file is created, or (c)
dropping the Windows job and saying so. Which one is right is the
owner's call, because it changes what the permissions mean on a platform
the project may not intend to support.
