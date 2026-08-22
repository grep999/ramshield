# RamShield — Fix Plan for Test-Suite Findings
Branch: `refactor/unify-crates` · one commit per finding · gates before each commit:
`cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace`

Order chosen by: risk-to-users first, then blast radius (smallest diffs ship first so
later findings can lean on earlier test infrastructure).

---

## F1 — Subnet batch hair-trigger  (HIGH, false-positive surface)

**Problem.** `subnet_batch_threshold` defaults to **5 events per 500ms window**.
Any /24 accumulating >~8 events in a single flush blocks the entire /24 for
`block_ttl_secs` (3600s default). One IP sending a 10-packet burst takes down its
whole upstream range — fatal for CGNAT / corporate egress.

**Root cause.** `crates/ramshield-detection/src/lib.rs:514`
`let threshold = cfg.detection.subnet_batch_threshold as u64;`
compared against `r.total_rps` where the "window" is just the last subnet-batch
tick (500ms). Config default (`config.rs:91`) was never revisited after the
window logic changed.

**Steps, in order**
1. **Decide semantics first** (one-line decision): threshold = *unique IPs* per
   window, not raw events. A /24 with 500 events from 3 IPs is one abuser;
   40 events from 40 IPs is a coordinated swarm. Unique-IP count is the actual
   attack signal. (`subnet_table` already tracks `unique_ips` — verify; if absent,
   add a counter to `SubnetRecord`, it's a u32 + increment at merge time.)
2. **Raise default**: `default_subnet_batch_threshold()` → 50 unique IPs/window,
   and keep `subnet_window_threshold: 500` as the event-volume secondary gate:
   block requires `unique_ips ≥ 50 AND events ≥ 100` in the same window.
   Both configurable; existing TOMLs keep working via serde defaults.
3. **Add decay**: halve counters each tick instead of hard reset (2-line change in
   the tick loop) so a sustained drip can't sit forever under a spike-and-hold.
4. **Tests** (in-repo, `ramshield-detection`):
   - 10 events / 1 IP → no subnet block
   - 60 unique IPs / 120 events → subnet block fires
   - 600 events / 5 IPs → no subnet block (volume alone insufficient)
   - decay: hot window that goes quiet de-arms within 3 ticks
5. **Update `/tmp/rs99/harness.py` d02/i07 expectations** if needed, re-run suite.
6. **Commit**: `fix(detection): subnet batch keyed on unique IPs, sane default —
   10-event burst no longer blocks whole /24`

---

## F2 — Block history ring buffer too small  (MEDIUM, forensics)

**Problem.** `/api/history/blocks` capped at 40 entries (`BLOCK_LOG` const,
`metrics/src/lib.rs:9`). During floods entries scroll out in seconds.

**Steps, in order**
1. Make it configurable: `[dashboard] block_log_size = 1000` in
   `DashboardConfig` (serde default 1000). One field, plumbed through
   `Metrics::new(cfg)`.
2. Keep `VecDeque` + pop-front (correct structure); only the constant becomes a
   runtime value. No pagination — YAGNI until someone needs >10k.
3. Dashboard JS badge already reads `len`; no UI change required.
4. Test: push 1500 records → `get_block_log().len() == 1000`, oldest evicted.
5. Update README config example (one line).
6. Commit: `feat(dashboard): configurable block log size (default 1000, was 40)`

---

## F3 — Silent TTL field-typo produces permanent block  (MEDIUM, footgun)

**Problem.** Wire contract is `ttl_secs`. A client sending `ttl_seconds` or
`ttl` gets silently accepted (serde ignores unknown fields), TTL drops to
`None` → permanent block. Silent misbehavior on operator typos.

**Steps, in order**
1. **Wire-compat check first** (constraint: deployed CLIs are sacred). Enumerate
   every field the deployed senders actually use: grep repo CLI + README examples +
   nginx/Lua snippet. If any deployed sender uses an alias, add `#[serde(alias)]`
   instead of rejecting.
2. Add `#[serde(deny_unknown_fields)]` to `Request` enum in
   `protocol/src/message.rs`. This turns typos into parse errors — consistent
   with how missing fields already behave.
3. Verify error path quality: unknown-field error must surface as the standard
   `{"type":"error","code":1,"message":"parse: ..."}` — codec already maps serde
   errors; confirm no panic path.
4. Tests (in-repo `protocol`):
   - `ttl_secs` accepted (existing roundtrip test stays green)
   - `ttl_seconds` → parse error naming the field
   - extra junk field on any variant → parse error
   - all documented request shapes still decode (fuzz-lite loop over README examples)
5. Re-run full `/tmp/rs99` suite — part A/G must stay 25/25 (they assert current
   error shapes).
6. Commit: `feat(protocol)!: deny_unknown_fields on requests — TTL typos now
   fail loudly instead of blocking forever`
   (`!` because strictness is technically wire-breaking for sloppy clients).

---

## F4 — Oversized batch resets connection without error  (LOW)

**Problem.** Batches pushing the line over `max_connection_bytes` (~1MB) get a
TCP reset mid-request. Correct limit enforcement; unfriendly shape.

**Steps, in order**
1. Read framing code (`src/ipc/` line reader). Confirm behavior: reader hits cap
   → aborts connection. Changing to read-discard-then-error risks buffering
   unbounded garbage — do NOT read past the cap.
2. Minimal fix: on cap-hit, write the typed error response **before** closing:
   `{"type":"error","code":413,"message":"line exceeds max_connection_bytes"}`.
   Two lines where the close happens; no new buffering.
3. Test (in-repo integration): send oversized line → expect error frame then EOF,
   not bare RST. If TCP state makes this flaky, mark `ignore` with a comment and
   cover the code path with a unit test on the reader.
4. Commit: `fix(ipc): emit 413-style error frame before closing oversize lines`

---

## F5 — Detection latency & re-offense  (NO FIX NEEDED)

Measured 1.0–1.2s worst case = one batch window + one subnet tick. That's the
design working. Re-offense re-blocks within one wave. TTL lift verified.
**Action:** none. Record numbers in docs/DOCUMENTATION.md performance section
(2 lines) so the next person doesn't re-measure. Commit folded into F1's docs.

---

## Execution order & dependencies

```
F3 (protocol strictness)   ── independent, smallest risk of semantic change,
                              unlocks trustworthy tests for everything else
F2 (block log size)        ── independent, trivial
F1 (subnet semantics)      ── biggest behavioral change, do after F2/F3 give
                              better observability + stricter protocol
F4 (oversize error frame)  ── independent, tiny
F5 (docs only)             ── rides along with F1
```

Each step: implement → gates green → targeted test update → atomic commit.
After all: full `/tmp/rs99` suite re-run (expect 109/109 with updated F1
expectations), live E2E smoke (attack_nexus red_team_full), push master.

Estimated diff sizes: F3 ~15 lines, F2 ~20 lines, F1 ~60 lines + tests, F4 ~10 lines.
