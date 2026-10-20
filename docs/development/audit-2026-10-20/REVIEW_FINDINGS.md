# F-01 independent review — findings

Three independent reviewers were dispatched with deliberately different lenses, each in an
isolated context, each forbidden from modifying the repository. All three were stopped when
work was redirected to `master`; their full transcripts are preserved alongside this file.

- `review-transcripts/reviewer-1-diff-lens.log` — task-0, diff-only lens (read the artifact cold)
- `review-transcripts/reviewer-2-execution-lens.log` — task-1, execution lens (re-derive, prove
  the baseline, hunt for flakiness) — **completed**
- `review-transcripts/reviewer-3-contract-lens.log` — task-2, contract lens (audit against f1 §10/§12,
  roadmap v1.2 §2, and the REQUEST CHANGES merge-gate checklist) — **stopped before emitting
  its checklist**

---

## 1. Execution lens (task-1) — COMPLETE

**Verdict: CONFIRMED** on the implementer's claims, **with one material behavioral defect found.**

Confirmed independently:
- Environment: rustc 1.100.0-nightly, cargo 1.100.0-nightly, Linux 6.8.0-147-generic x86_64, 4 cores.
- All gates reproduced: `fmt` / `check` / `clippy -D warnings` / `test --workspace` / `doc` — exit 0.
- The baseline was independently derived (pre-change code extracted to a scratch tree); the new
  tests **genuinely fail against the pre-change code**, so they are not vacuous.
- No flakiness: the mesh suite was run 5× and again single-threaded, with no timing failures.
- The reviewer verified the working tree was **byte-identical** to its pre-review state on exit
  (`md5s match pre-review values`, `git status --short` unchanged).

### DEFECT (P1, not covered by any test) — IPv4-mapped-IPv6 source-IP mismatch

When the local mesh listener is a dual-stack IPv6 socket (`[::]:7900`), the kernel reports an
IPv4 client's remote address as the IPv4-mapped IPv6 address `::ffff:127.0.0.1`. The F-01 source-IP
binding compares `remote.ip() != configured.address.ip()`; `IpAddr::V6(::ffff:127.0.0.1)` is **not
equal** to `IpAddr::V4(127.0.0.1)`, so a legitimate peer is rejected.

Evidence collected by the reviewer:

- `PEER_ADDR=[::ffff:127.0.0.1]:40714` — the kernel's reported peer address for an IPv4 client.
- Probe `probe_mapped_spelling_peer`: `MAPPED_SPELLING_DRAINED=1` — configuring the peer with the
  mapped spelling makes the same frame succeed, isolating the cause to the address-form mismatch
  rather than the MAC.

### Test-quality weakness (P2)

Negative tests assert "receive queue empty after 150 ms" **without an in-test positive control**,
so they would also pass if the receiver accepted nothing at all. Mitigated at file level by the
positive-control tests in the same module, but each negative test should carry its own control.

---

## 2. Diff-only lens (task-0) — STOPPED, but independently confirmed the same defect

Built a standalone end-to-end crate against `ramshield-mesh` and measured real acceptance:

```
listen=127.0.0.1:39872     peer_addr=127.0.0.1:7900        -> VALID-MAC frame accepted = true
listen=[::]:39873          peer_addr=127.0.0.1:7900        -> VALID-MAC frame accepted = false   <-- defect
listen=[::]:39874          peer_addr=[::ffff:127.0.0.1]:7900 -> VALID-MAC frame accepted = true
```

and the raw socket observation:

```
ACCEPTED remote = [::ffff:127.0.0.1]:52874
remote.ip() = ::ffff:127.0.0.1
remote.ip() == configured v4 ip ? false
```

This independently reproduces the execution-lens defect by a different route, and confirms the MAC
is not at fault.

---

## 3. Contract lens (task-2) — STOPPED before emitting its checklist

It had completed the document reads (f1, roadmap v1.2, the REQUEST CHANGES review) and was in the
source-verification phase. **No verdict was produced.** The contract checklist must be re-run when
mesh work resumes.

Independently verified by the implementer in the meantime (see `PROGRESS_LOG.md` entry
`F01-INDEP-CHECK`, `2026-10-20T14:48:15Z`):
- *One MAC type cannot authorize another* — frame MAC input is a 3-element JSON array, ACK MAC
  input is a 5-element array; `WireMessage`'s untagged variants cannot both parse from one JSON
  object, and the receiver re-serializes the parsed body for verification.
- *ACK/recovery proof bound to expected peer + session + range* — sessions keyed by `sender_node_id`
  and bound to `generation`/`recovery_id`/`boundary_lsn`/`snapshot_digest`/`frame_count`;
  `complete_recovery` re-checks all of them, requires every frame, recomputes and compares the
  digest, and is idempotent on replay.

---

## 4. Status at the point of redirection

| Item | State |
|---|---|
| Verdict A — HMAC protocol hardening | PASS (confirmed independently) |
| Verdict B — peer identity isolation | PARTIAL — cross-edge isolation provided; within-edge endpoint identity not provided (symmetric-HMAC limit). Requires owner acceptance as residual risk. |
| IPv4-mapped-IPv6 defect | **FIXED** — `same_ip()` folds an IPv4-mapped IPv6 address onto IPv4 on both sides of the comparison (`crates/ramshield-mesh/src/transport.rs`). Verified RED→GREEN: with the normalization disabled both new tests fail; with it restored both pass. Commit `see git log feature/mesh-f01-hmac-edge-secrets`. |
| Negative-test positive controls | **OPEN — P2** |
| Contract audit checklist | **NOT DONE — re-run required** |
| Merge gate (REQUEST CHANGES §9, 12 items) | Not formally signed off — blocked on the above |
