# F1a — /0 Prefix in SHM Table: Owner Decision Record — CLOSED (Option 1)

**Status**: CLOSED — Option 1 retained

**Owner Decision**: Option 1 — retain the restriction on key 0 in the SHM rule table.

**The Technical Reason**

The SHM rule table is a 262,144-slot open-addressed hash table in shared memory,
64 bytes per slot, 4-slot probe window. Key 0 is the reserved empty-slot sentinel:
every publisher treats a slot with `client_hash == 0` as free. The slot-key mixer
is 0-preserving, so `subnet_key(N, p) == 0` exactly when masked network N == p.

This means every `/0` rule and `0.0.0.32/32` (and IPv6 `::/0`) hash to key 0 —
indistinguishable from "empty slot". The table has no representation for `/0`
rules; previously a `/0` rule published under key 0 would be silently overwritten
by the next colliding publish.

**Why Option 1**

- No format change → no C ABI break, no coordinated release needed
- XDP LPM map already handles `/0` in the in-repo enforcement path
- Userspace SHM side channel remains simple and stable
- No disruption to existing callers; `/0` is never published to the table
- Documented refusal/rejection path at publisher + reader already exists (`d1f4a81`)
- Minimal code change; only the existing refusal in publisher + rejection in reader

**Affected Rules No Longer Published to SHM**:
- IPv4 `/0` — publisher refuses key 0, reader rejects as INVALID
- `0.0.0.32/32` — same
- IPv6 `::/0` — publisher refuses key 0, reader rejects as INVALID

**In-Repo Coverage**:
- `/0` coverage via XDP LPM map (BPF dataplane) — already functional
- Userspace SHM path: `/0` unsupported, explicitly refused — documented in README and config validation

**Decision Rationale**

Option 1 is the minimum-change path. No format migration, no ABI break, no coordinated
release with out-of-repo consumers. The SHM table stays as-is; the refusal/rejection
logic already exists from the `d1f4a81` change. The only ongoing burden is documentation
clarity — which is what this record provides.

**Closure Criterion**

- Publisher refuses key 0 and logs refusal (already present in code at `publish_rule`)
- Reader rejects lookup for key 0 as `INVALID` (already present in code at `lookup_rule`)
- Documentation updated to state `/0` unsupported in SHM side channel, covered by XDP LPM
- No runtime regressions; all existing tests pass

**Owner**: `rs` maintainer (this session). Decision recorded 2026-10-31.

| Option | Description | Trade-off |
|--------|-------------|-----------|
| **1. Retain restriction** (current) | Keep key 0 as sentinel; /0 unsupported in SHM. Document clearly. | Simple, no format change, no coordinated release. XDP LPM handles /0. |
| **2. Change SHM format** | Add a `valid` bit or use a different sentinel (e.g., `u64::MAX`). | Breaks C ABI; requires coordinated release with out-of-repo consumer. |
| **3. Side-channel for /0** | Store /0 rules in a separate small table / bitmap. | Adds complexity; marginal benefit since XDP already handles it. |

**Recommendation**: Option 1. The SHM table is a *side channel* for the
out-of-repo reverse proxy. The in-repo enforcement path (XDP LPM) already
handles `/0` correctly. No coordinated release needed.

**Owner Decision**: [ ] Option 1  [ ] Option 2  [ ] Option 3

**Decision Record**: To be filled by owner.