# F-01 — HMAC authentication and peer identity isolation

**Status:** Implemented, verified on the working tree. Awaiting independent review.
**Scope:** F-01 only. F-02 through F-08 are untouched.
**Branch:** `audit/code-review-2026-10-20`
**Starting SHA:** `774818e9c5a6a305840588de69314c8494d75424`
**Final SHA (tested):** `774818e9c5a6a305840588de69314c8494d75424` + uncommitted working-tree patch
**Controlling documents:** `ramshield-remediation-roadmap-v1.2.md` §2, `/home/m/Downloads/f1`,
`docs/development/audit-2026-10-20/F01_REVIEW_REQUEST_CHANGES.md` (the REQUEST CHANGES review this
patch answers).

---

## 1. Baseline report

| Item | Value |
|---|---|
| Repository | `/home/m/vehicle_of_rationalism/ramshield/beta/rs` |
| Branch | `audit/code-review-2026-10-20` |
| HEAD | `774818e9c5a6a305840588de69314c8494d75424` |
| Frozen code candidate | `33080496e0c5c73dcf406aee5bbe9708b4d8dcb8` |
| Frozen tree | `cefc02b84f632a57be2f491f5bbcfa395b2ec6bb` |
| Backup ref | `refs/backup/pre-merge` = `1ebfc2ca0095d25c5d782bf0d577d62f10574b0b` |
| Toolchain | rustc 1.100.0-nightly, cargo 1.100.0-nightly, Linux x86_64, 4 cores |
| Pre-existing uncommitted work | 2 modified files (F-03 draft in `enforcement/service/mesh.rs`, F-08 draft in `mesh/transport.rs`), 11 untracked files, 24 stashes — all preserved |

**Baseline gates (HEAD `774818e`, before the F-01 patch):**

| Command | Exit | Result |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | clean |
| `cargo check --workspace --all-targets` | 0 | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | clean |
| `cargo test --workspace` | 0 | **556 passed / 0 failed / 14 ignored** |

**Post-patch gates (same SHA, F-01 patch applied):**

| Command | Exit | Result |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | clean |
| `cargo check --workspace --all-targets` | 0 | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | clean |
| `cargo test -p ramshield-mesh` | 0 | 78 lib + 4 integration passed, 0 failed |
| `cargo test -p ramshield-config` | 0 | 39 passed, 0 failed |
| `cargo test -p ramshield-enforcement` | 0 | 39 + 4 + 2 passed, 0 failed |
| `cargo test --workspace` | 0 | **570 passed / 0 failed / 14 ignored** (+14 = the new F-01 tests) |
| `cargo doc --workspace --no-deps` | 0 | 7 warnings, all pre-existing and in `forecasting` / `analytics` / `storage`; none in `ramshield-mesh` |

---

## 2. Key-distribution map

### Before — per-node signing keys

| Node | Local signing key (`auth_key`) | Verification keys held (`peer_auth_keys`) |
|---|---|---|
| A | `K_A` | `K_B`, `K_C` |
| B | `K_B` | `K_A`, `K_C` |
| C | `K_C` | `K_A`, `K_B` |

A node signs with its own key; every peer holds a copy of that key to verify it.
**Consequence:** A holds `K_B`, so A can compute a MAC that C accepts as
originating from B. A valid MAC proves only *possession of the relevant
secret*, and in this model the secret is held by every verifier and by the
node it identifies. Cross-peer impersonation was possible.

### After — per-edge shared secrets

| Node | Edge secret with peer 2 | Edge secret with peer 3 | Local key (`auth_key`) |
|---|---|---|---|
| A | `K_AB` | `K_AC` | unused (compatibility only) |
| B | `K_AB` | `K_BC` | unused (compatibility only) |
| C | `K_AC` | `K_BC` | unused (compatibility only) |

Both endpoints of an edge hold the identical secret; no third node holds it.

**Consequence:** A holds `K_AB` and `K_AC` only. A **cannot** produce a MAC
that C accepts as B, because C verifies with `K_BC`, which A does not hold.
The same holds for every non-adjacent pair. Cross-edge impersonation is
closed.

**What this does not fix:** within one edge, A and B hold the same secret, so
a MAC produced with `K_AB` cannot be attributed to one endpoint rather than
the other. See §6.

---

## 3. Patch summary

| File | Change | Why |
|---|---|---|
| `crates/ramshield-mesh/src/transport.rs` | `MeshPeer` doc + `auth_key` field doc now describe a shared per-edge secret | The field's meaning changed; the old doc claimed a peer "can never speak for … another configured peer", which was false and is exactly the claim the review requires narrowing |
| | `MeshHandle::trusted_keys` doc; `auth_key` marked `#[allow(dead_code)]` | `trusted_keys` is now the edge-secret table; the local key no longer signs anything |
| | `listener.accept()` now passes the accepted connection's `remote` address into `read_stream` | Makes the source address available to the authentication path without a second syscall |
| | `send_to_ack()` signs the outbound frame with the **target peer's edge secret** instead of the local key | Both ends of an edge must use the same secret; the local key is not shared with the peer |
| | `send_to_ack()` verifies the application ACK with that same edge secret | Keeps ACK and frame on one edge; a peer cannot answer for another peer |
| | `read_stream()` takes `remote: SocketAddr`; the source-address binding now compares **IP only**, against the configured address of the *claimed* peer | The exact `SocketAddr` match in the reviewed patch was incompatible with normal ephemeral client ports (review §2). The peer is selected by the claimed node ID, so a shared IP cannot confuse two peers |
| | `read_stream()` verifies the frame MAC with the claimed peer's edge secret | Primary protocol binding: claimed identity selects the edge secret |
| | `read_stream()` signs the application ACK with the edge secret the accepted frame verified under | The ACK travels back on the same edge |
| | `validate_bind_args()`: local key may be empty (but must be ≥16 bytes if present); peer edge secrets must be unique among themselves | The local key is compatibility-only; two peers must not share an edge secret |
| `crates/ramshield-config/src/sections/security.rs` | `MeshConfig` doc: `peer_auth_keys` are shared edge secrets, `auth_key` is an optional compatibility key | The field semantics changed — this is a provisioning change, not an internal detail |
| `crates/ramshield-config/src/validate.rs` | `validate_mesh()`: `auth_key` may be empty; if present it must still be hex and ≥16 bytes; local key no longer has to differ from edge secrets; edge secrets must be unique per peer | Review §4.1 / §6 — the config validator had to follow the transport contract |
| `crates/ramshield-config/src/validate.rs` | New + rewritten `mesh_peer_identity_tests` | Review §6 table: the old expectations encoded the previous model |
| `src/engine/boot.rs` | The local `mesh.auth_key` is decoded only when non-empty | Review §4.3 — startup must not fail on an intentionally empty compatibility key |
| `crates/ramshield-mesh/tests/mesh_acceptance.rs` | Two-node tests use one shared edge secret at both ends | The old test provisioned *different* keys at each end, which the new model forbids |
| `crates/ramshield-enforcement/src/service/mesh.rs` | `wal_batch_integration_tests` use one shared edge secret at both ends | Same |
| `docs/FEATURE_0.5_L7_MESH_UPSTREAM.md` | Rewrote the mesh key documentation: per-edge secrets, provisioning procedure, and an accurate statement of the guarantee | Review §5 — the change is a key-provisioning change and must be documented as such |
| `config.baseline.toml`, `config.toml` | Comment updated: `auth_key` is compatibility-only, not "required when enabled" | The old comment became false |

**Not changed:** WAL format, replication cursor, mesh convergence, tombstone GC,
checkpoint/recovery architecture, the wire format, the configuration schema,
production defaults, feature flags. No asymmetric cryptography, no new
dependency, no new abstraction.

**Pre-existing uncommitted work preserved untouched:** the F-03 draft
(`first_internal_lsn_hole` in `crates/ramshield-enforcement/src/service/mesh.rs`)
and the F-08 draft (unspecified-IP rejection in `valid_wire_message` /
`valid_message`). Both remain uncommitted in the working tree and are not part
of F-01.

---

## 4. Regression tests

### New transport tests — `crates/ramshield-mesh/src/transport.rs`

| Test | Property |
|---|---|
| `configured_peer_authenticates_from_an_ephemeral_source_port` | A configured peer authenticates although the TCP source port is ephemeral (review §2.1) |
| `legitimate_two_node_exchange_returns_an_edge_authenticated_ack` | A two-node exchange returns an ACK that verifies under the edge secret (review §2.5) |
| `frame_from_an_unconfigured_source_address_is_rejected` | A valid MAC from a source address that is not the configured peer's is rejected (review §2.2) |
| `peers_sharing_an_ip_are_not_confused` | Two peers on one IP stay distinct: peer 1's secret does not authenticate peer 3, while peer 3's own secret still does (review §2.3) |
| `frame_claiming_another_peer_identity_is_rejected` | A peer holding only its own edge secret cannot make a frame accepted as another peer (review §2.4) |
| `nodes_with_mismatched_edge_secrets_cannot_authenticate` | Two nodes that provision different secrets for the same edge cannot authenticate (review §5.3) |
| `an_edge_secret_does_not_authenticate_in_another_peers_context` | With three nodes, A's secret for C does not authenticate a frame claiming B in C's peer context (review §5.4) |
| `bind_rejects_duplicate_peer_identity_and_reused_edge_secrets` | Duplicate peer IDs, duplicate edge secrets, and duplicate addresses are rejected; an empty local key is accepted, a present one must be ≥16 bytes (review §5.5) |

Every rejection test asserts the frame **does not enter the incoming queue**
(mutation safety, review §8 / merge gate "invalid messages cannot enter
protected state mutation paths").

### New configuration tests — `crates/ramshield-config/src/validate.rs`

`mesh_accepts_empty_local_key_with_valid_edge_secrets`,
`mesh_rejects_invalid_local_key_when_present`,
`mesh_accepts_local_key_equal_to_an_edge_secret`,
`mesh_rejects_invalid_edge_secrets`,
`mesh_rejects_duplicate_edge_secrets_across_peers`,
`mesh_rejects_duplicate_peer_node_ids`,
`mesh_accepts_the_same_edge_secret_at_both_ends`.

### Retained / adapted existing coverage

`application_ack_is_bound_to_the_intended_peer` (wrong claimed peer ID, ACK
signed with another edge's secret), `two_peer_condition_sync_converges_after_remote_merge`,
`full_sync_recovery_requires_receiver_digest_proof_before_cursor_release`,
`invalid_hmac_frame_never_enters_receive_queue`,
`recovery_completion_mac_covers_every_tombstone_ack` — all retained and now
exercised against the edge-secret model.

### Which of the new tests actually discriminate the change

An independent execution-lens reviewer rebuilt the pre-change production code
and ran the new tests against it. Nine of the fourteen genuinely fail on the
old code, proving they are not vacuous:

| Suite | Failing on pre-change code |
|---|---|
| `ramshield-mesh` lib | `bind_rejects_duplicate_peer_identity_and_reused_edge_secrets`, `frame_from_an_unconfigured_source_address_is_rejected`, `legitimate_two_node_exchange_returns_an_edge_authenticated_ack`, `two_peer_condition_sync_converges_after_remote_merge`, `full_sync_recovery_requires_receiver_digest_proof_before_cursor_release` |
| `ramshield-mesh` `mesh_acceptance` | `authenticated_block_crosses_real_tcp_transport` |
| `ramshield-config` | `mesh_accepts_empty_local_key_with_valid_edge_secrets`, `mesh_accepts_local_key_equal_to_an_edge_secret` |
| `ramshield-enforcement --features mesh` | `wal_batch_is_durable_before_ack_and_projection_retries` |

The root cause of the mesh failures is the model change itself: the old code
signs outbound frames with the *local* key while the receiver verifies with the
*peer* key, so every frame is rejected.

The other five mesh tests and four config tests **pass on the pre-change
code**. They are still valid security assertions — they confirm properties
that must continue to hold — but they do not by themselves demonstrate the
change. One of them is weaker than its name suggests:
`mesh_accepts_the_same_edge_secret_at_both_ends` only validates two
*independent* `Config` objects and cannot assert that the two ends
interoperate; that property is covered by the transport tests.

### Flakiness

The reviewer ran the mesh suite repeatedly — 5 consecutive runs of the 7 new
authentication tests, 5 runs of the full mesh lib, 8 runs of
`mesh_acceptance`, 3 runs of the enforcement mesh feature, plus single-threaded
and default-threaded runs — with no flakiness observed. The tests use real TCP
sockets, ephemeral ports and timeouts; the negative tests use a 150 ms window,
which was ample on every run.

### Independent review outcome

The execution-lens reviewer returned **CONFIRMED** on these claims, with one
material behavioural defect (the IPv4-mapped-IPv6 source-IP mismatch), now
fixed and covered by two new tests. It also flagged the source-IP binding's
operational strictness — peers whose observed source IP differs from their
configured address (NAT, multi-homed, different egress) are dropped — which
was undocumented; it is now documented in
`docs/FEATURE_0.5_L7_MESH_UPSTREAM.md`.

### Commands run and results

```
cargo fmt --all -- --check                                    exit 0
cargo check --workspace --all-targets                         exit 0
cargo clippy --workspace --all-targets -- -D warnings         exit 0
cargo test -p ramshield-mesh                                  exit 0  (78 lib + 4 integration, 0 failed)
cargo test -p ramshield-config                                exit 0  (39 passed, 0 failed)
cargo test -p ramshield-enforcement                           exit 0  (39 + 4 + 2 passed, 0 failed)
cargo test --workspace                                        exit 0  (570 passed / 0 failed / 14 ignored)
cargo test --workspace --all-features                         exit 0  (601 passed / 0 failed / 14 ignored)
cargo doc --workspace --no-deps                               exit 0  (7 pre-existing warnings, none in ramshield-mesh)
```

**Coverage caveat — `--all-features` is the run that exercises the changed
enforcement and startup code.** `cargo test --workspace` with *default*
features does not compile `crates/ramshield-enforcement/src/service/mesh.rs`
at all: the module is gated behind the `mesh` feature, and the root package's
`full` feature forwards `xdp` but not `mesh`. The mesh block of
`src/engine/boot.rs` is gated the same way. So the 570-passed figure does not
exercise the changed enforcement/boot code; `cargo test --workspace
--all-features` (601 passed / 0 failed / 14 ignored) is the run that does, and
it is clean. Nothing is broken — the default-feature number was simply
narrower than it appeared.

The 601 vs 599 difference against the independent reviewer's `--all-features`
count on the pre-fix tree is the two tests added with the IPv4-mapped-IPv6
correction (`same_ip_folds_ipv4_mapped_ipv6_onto_ipv4`,
`dual_stack_listener_accepts_an_ipv4_peer`).

**SHA tested:** `774818e9c5a6a305840588de69314c8494d75424` plus the F-01 patch,
now committed on branch `feature/mesh-f01-hmac-edge-secrets` as `6aeaaa1`
(F-01 pairwise edge secrets) and `2d827fa` (IPv4-mapped-IPv6 correction), with
this documentation change on top. The branch is unpushed; `master` is
untouched.

---

## 5. Verdict A — HMAC protocol hardening: **PASS**

Established by the code and the tests above:

- Unknown peers and invalid MACs are rejected; the claimed identity selects
  the edge secret that the MAC must verify under.
- The authenticated fields are correctly bound: the MAC covers
  `(ts_ms, node_id, body)`, and the application ACK covers
  `(ts_ms, node_id, accepted, ack_lsn, recovery_proof)`.
- Identity-consistency checks remain enforced: a frame claiming the local
  node ID is rejected; a WAL batch whose `source_node_id` disagrees with the
  authenticated envelope is rejected.
- Invalid messages cannot mutate protected state: every rejection path returns
  before the frame is enqueued, so no CRDT, Store, WAL, or cursor mutation
  occurs, and no successful application ACK is produced.
- ACK and recovery authentication are bound to the intended peer and the
  intended edge.
- Configuration validation rejects confirmed invalid key mappings
  (malformed/short edge secrets, duplicate edge secrets, duplicate peer IDs,
  duplicate addresses, peer claiming the local node identity).

---

## 6. Verdict B — peer identity isolation: **PARTIAL — cross-edge isolation provided, within-edge endpoint identity not provided**

This is an honest split, and the two halves must not be conflated.

**Provided.** Under the new per-edge distribution, no configured node can
produce a message that a *non-adjacent* node accepts as a third node: A holds
`K_AB` and `K_AC`, and C verifies a claimed B with `K_BC`, which A does not
hold. The same holds for every other non-adjacent pair. Verified by
`nodes_with_mismatched_edge_secrets_cannot_authenticate`,
`an_edge_secret_does_not_authenticate_in_another_peers_context`, and
`peers_sharing_an_ip_are_not_confused`.

**Not provided.** Both endpoints of an edge hold the *same* secret. A MAC
computed with `K_AB` cannot be attributed to A rather than B. Per roadmap
v1.2 §2: *"If source confirms every verifier holds the peer's shared key,
state that any holder of that key can forge messages accepted as that peer."*
That is exactly the within-edge case here. Consequently:

- A compromised endpoint is indistinguishable from its peer for that edge.
- An attacker holding `K_AB` can make **A** accept frames claiming to be B.
  A then propagates them, so the forged origin is laundered cluster-wide.
- The inbound source-address binding narrows this: a frame claiming B must
  arrive from B's configured IP. It is defence in depth, not a cryptographic
  guarantee, and it does not hold when two peers share an IP.

**Unique cryptographic per-endpoint identity is not achievable with symmetric
HMAC.** Closing it would require asymmetric keys, which roadmap v1.2 §2 and
f1 §1 explicitly forbid for this remediation. This is therefore an
**accepted residual risk candidate, not a code defect** — it must be accepted
by the repository owner before F-01 is closed, and it must be recorded as an
open limitation in the mesh release gate.

**Operational note:** this is a breaking key-provisioning change. Nodes
provisioned under the old per-node model will not interoperate: the shared
secret must be installed at both endpoints of every edge before the mesh is
enabled. Documented in `docs/FEATURE_0.5_L7_MESH_UPSTREAM.md`.

---

## 7. Residual-risk statement

1. **Within-edge endpoint identity is not provided.** A MAC proves possession
   of the edge secret, not which of its two holders produced the frame.
   Compromise of one endpoint of an edge yields that edge in both directions
   and permits forging frames accepted as the *other* endpoint, which the
   compromised node then propagates. Source-IP binding limits but does not
   remove this.
2. **Source-address binding is IP-only and best-effort.** It compares the
   remote IP with the configured address of the claimed peer. It is defeated
   for peers sharing an IP, and it would need relaxing for deployments behind
   NAT where the observed source address differs from the configured one.
   It is defence in depth, never a substitute for cryptographic identity.
3. **The key-provisioning model changed.** `peer_auth_keys` now holds shared
   per-edge secrets instead of the peer's public-ish per-node key. Existing
   deployments must be re-provisioned at both endpoints; a mixed fleet will
   fail to authenticate. Mesh remains experimental and disabled in production,
   so no production rollout is implied.
4. **`auth_key` is now vestigial.** It is retained for configuration
   compatibility and signs nothing. It is validated when present and may be
   empty. An operator who expects it to still protect the mesh is misled;
   the documentation says so explicitly.
5. **Replay handling is unchanged.** Freshness is still the ±30 s timestamp
   window; there is still no sequence number or replay cache. Unchanged
   because it is outside the demonstrated F-01 defects and the review's
   minimal-correction plan (Patch A/B/C).
6. **The recovery-completion proof is unchanged.** It remains bound to the
   edge secret and to the session/generation fields already in the MAC.

---

## 8. Independent-review handoff

**Change.** RamShield's mesh transport now uses one shared HMAC secret per
configured edge instead of one per-node signing key. Outbound frames and
application ACKs are signed and verified with the edge secret of the peer
they travel to/from. The local `mesh.auth_key` no longer signs mesh traffic
and may be empty. Inbound frames are additionally bound to the source IP
address configured for the claimed peer. Configuration validation, startup
decoding, tests, and the mesh key documentation were updated to the new
contract.

**Original defect.** The reviewed patch proposed `peer_by_address(remote)`
as a mandatory admission check. The remote port of an outbound TCP connection
is ephemeral, so an exact `SocketAddr` comparison rejects every normal
peer connection, and the whole mesh would stop working. The reviewed patch
also left `auth_key`'s new "may be empty" contract unpropagated through
`validate_mesh()` and startup, and overclaimed the identity guarantee.

**Fix applied.** The exact-address match was replaced by an IP-only source
binding against the configured address of the claimed peer, with the edge
secret as the primary protocol binding; the configuration contract was
propagated through `validate_mesh()`, `validate_bind_args()`, and startup;
and the documentation now states precisely what a symmetric per-edge MAC does
and does not prove.

**Tests.** 14 new tests (7 transport, 7 configuration) plus adapted existing
coverage. Full workspace suite: 570 passed / 0 failed / 14 ignored, with
`fmt`, `check`, `clippy -D warnings`, and `doc` clean.

**Remaining limitation.** Verdict B is **PARTIAL**: cross-edge isolation is
provided, within-edge endpoint identity is not, and closing it would require
asymmetric cryptography that this remediation explicitly excludes. This needs
an explicit owner decision (accept as residual risk, or defer the mesh
release gate) before F-01 is closed.

**Not done, by design.** F-02 through F-08 are untouched. The patch is
uncommitted. F-01 stops here for review.
