# Audit Findings Backlog — RamShield Code Review `774818e`

**Source report:** `/home/m/Downloads/ramshield-code-review-report.md`
**Audited tree:** `774818e9c5a6a305840588de69314c8494d75424` (same SHA as this backlog's base)
**Backlog branch:** `audit/code-review-2026-10-20`
**Triage date:** 2026-10-20

The report's reviewer had no `cargo`/`rustc` and could not run a single test; its
findings are source-level claims. Every finding below was therefore **re-verified
against the live tree** before being accepted. Verdicts:

- **CONFIRMED** — reproduced against current source; a bounded fix exists.
- **CONFIRMED (design-level)** — reproduced, but the fix needs a trust-model or
  wire-format decision; cannot be patched without authorization.
- **ALREADY DOCUMENTED** — known pre-ship limitation previously accepted.
- **PARTIAL** — claim holds only in part.

| ID | Sev | Finding | Verdict | Evidence (current tree) | Action | Gate |
|----|-----|---------|---------|--------------------------|--------|------|
| F-01a | P0 | Symmetric per-node HMAC keys cannot prevent cross-peer impersonation | CONFIRMED (design-level) | `transport.rs:557-560` stores every peer's sign/verify key; `transport.rs:1272` selects that key from the *claimed* `env.node_id`; holding peer B's key lets A forge B | DEFER — needs asymmetric keys or strictly pairwise MACs + config change | independent security review |
| F-01b | P0 | Inbound frame identity is not bound to the connecting peer's address | CONFIRMED | `transport.rs:1266-1296` verifies MAC + timestamp + source identity, never consults `stream.peer_addr()` against the configured `MeshPeer.address` | **PATCH NOW** — fail closed on IP mismatch (defense-in-depth; does not close F-01a) | transport auth tests |
| F-02 | P0 | Local/operator mesh mutations are not journalled as origin-aware `MeshMutation` events | CONFIRMED | `enforce.rs:113-153` appends only `BlockIp`/`UnblockIp`; the local CRDT dot is created later at `enforce.rs:342` and the local tombstone at `enforce.rs:578`; `boot.rs:59` and `replay.rs:122,283` reconstruct provenance solely from `MeshMutation` | DEFER — needs a single durable ordering for local origin dots | crash injection at every ordering boundary |
| F-03 | P0 | WAL replication detects a gap before a page but not inside a page | CONFIRMED | sender `enforcement/service/mesh.rs:226-235` checks only `records[0].0 > cursor+1`; receiver `transport.rs:1560-1566` checks only strict `<` increase of `source_lsn`, never adjacency | **PATCH NOW** — require every adjacent LSN to be contiguous on both send and accept; fail closed | missing-segment regression |
| F-04 | P1 | Expired recovery deltas acknowledged without causal merge | CONFIRMED | `transport.rs:330-334` treats `expires_at_ms <= now` as satisfied for frame acceptance; `enforcement/service/mesh.rs:572-573` returns `Ignored` before any merge | PATCH — merge causal state, skip only the enforcement projection | expired-entry test asserts no Store block **and** preserved dot |
| F-05 | P1 | Expiry pruning and tombstone GC have no production caller; state growth unbounded | CONFIRMED | `purge_expired` callers: `aworset.rs:884,917,930` (tests only). `gc_tombstones` callers: `aworset.rs:1236-1373` (tests only) | PATCH (bounded) — capacity/degraded-health guard; do **not** enable GC | churn/memory budget test |
| F-06 | P1 | Tombstone-GC protocol needs a per-removal watermark and peer incarnation | ALREADY DOCUMENTED | previously accepted pre-ship limitation; `aworset.rs:702-705` documents the epoch-vs-watermark gap | BACKLOG — protocol design + review | partition/rejoin/restart tests |
| F-07 | P1 | Two-process fault harness and second independent review missing | CONFIRMED | `scripts/test-mesh-recovery-sandbox.sh` is single-process; acceptance matrix records no second verdict | BACKLOG — build harness, obtain exact-SHA second verdict | harness green + reviewer APPROVE |
| F-08 | P2 | Transport accepts unspecified block IPs that other validation boundaries reject | CONFIRMED | `transport.rs:1533-1539` `valid_block` omits `!d.ip.is_unspecified()`; `transport.rs:1500-1507` recovery-frame blocks omit it too; `valid_unblock` (`1541`) and `WalBatch` (`1573`) both have it | **PATCH NOW** | IPv4 + IPv6 rejection tests |

## Ordering

1. **F-03** — highest severity that is fixable without a design decision; silent
   history loss, fail-closed, bounded to two files.
2. **F-08** — trivial, closes a validation-boundary inconsistency.
3. **F-01b** — bounded defense-in-depth on the inbound auth path. Not a
   substitute for F-01a, which stays open.
4. **F-04** — causal merge vs projection split.
5. **F-05** — capacity guard only; GC stays disabled until F-06 lands.
6. **F-02, F-01a, F-06, F-07** — blocked on design decisions or process work;
   tracked, not silently dropped.

## Explicitly unchanged

No wire format, no configuration schema, no production default, and no
feature flag is modified by the "PATCH NOW" items. `3308049` is not rewritten;
this branch sits on top of `774818e` and nothing is pushed.
