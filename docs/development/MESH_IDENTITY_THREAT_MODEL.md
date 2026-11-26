# Mesh node-bound authentication — threat model and protocol proposal

Scope: issue #99. Mesh remains optional and production-disabled. No recovery/CRDT/WAL change.

## Threat model

Current trust root: each peer stores the HMAC key for a configured `node_id`. A peer that holds that key can produce any message/ACK attributed to that node. A compromised or misconfigured peer can therefore impersonate another node and forge ACK/completion provenance.

Assets: per-origin block decisions, recovery completion proofs, CRDT actor identity, replay state.

Adversaries:
- A: compromised peer, knows its own key and node_id.
- N: network attacker who can replay/reflect but not read keys.
- M: misconfigured operator who pastes the same key into two peers.

In scope: identity forgery, ACK/proof forgery, replay/reflection, wrong recipient/kind, stale sequence, unknown key.
Out of scope: source-IP identity, recovery persistence, CRDT/tombstone redesign, WAL retention.

## Decision

Use per-node Ed25519 signing keys. Each node keeps only its private key; peers receive only the public key bound to the configured `node_id`. No pairwise shared secrets. No mTLS in this change.

## Wire/config model

- Config: `[[mesh.peers]] node_id`, `public_key` (32-byte base64/hex), optional `private_key` only in the local node's own config. Unknown/duplicate node_id or key is a config error. Private key never logged or included in errors.
- Key id: SHA-256 of canonical public key, first 16 hex chars, for rotation diagnostics only; never used as identity.
- Rotation: add new `public_key` for same node_id with `key_epoch`; remove old only after convergence. Revocation = remove peer/key and require restart; no silent fallback.
- Migration: existing HMAC config fails closed with a clear error until migrated; no dual-verify fallback.

## Signed transcript

Domain-separated canonical bytes:
`RAMSHIELD-MESH-AUTH\0` || protocol_version || sender_node_id || recipient_node_id || direction || message_kind || session_nonce || sequence || payload_hash

Rules:
- `direction` is `REQUEST` or `REPLY`; reflection rejected.
- `recipient_node_id` must equal local node_id.
- `message_kind` is bound; ACK cannot be replayed as data.
- `session_nonce` is 32 bytes from OS RNG, new per session.
- `sequence` is monotonic per sender/session; stale/duplicate rejected.
- Verify signature before any payload decode. Unknown key, bad key id, wrong recipient/kind, stale sequence → drop and count metric; no error text containing key material.

## Failure behavior

- Missing/invalid local private key: mesh disabled, startup error if mesh enabled.
- Unknown peer key: drop, metric, no fallback.
- Signature failure: drop, metric, no ACK.
- Mesh stays production-disabled until three independent reviews pass.

## Tests required before implementation

1. A/B/C: A cannot forge B→C data, ACK, completion proof; B positive control passes.
2. Replay, reflection, wrong recipient, wrong kind, session nonce mismatch, stale sequence, unknown key, rotation/revocation.
3. Config serialization/validation/migration; no secret in logs/errors.
4. Existing mesh tests unchanged and green.

## Compatibility decision

Wire version bump is required; old HMAC frames are rejected. No plaintext private keys in peer config.