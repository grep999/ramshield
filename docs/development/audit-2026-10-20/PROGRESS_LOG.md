# F-01 audit progress log

Working notes for the RamShield F-01 remediation (HMAC authentication and peer
identity isolation), executed against
`/home/m/Downloads/ramshield-remediation-roadmap-v1.2.md` and the
code-grounded guide `/home/m/Downloads/f1`.

Repository: `/home/m/vehicle_of_rationalism/ramshield/beta/rs`
Branch at start: `audit/code-review-2026-10-20`
HEAD at start: `774818e9c5a6a305840588de69314c8494d75424`

Entries are appended chronologically with `append_log.sh`. Detailed object
definitions, the key-distribution map, and findings live in `F01_NOTES.md`.

## Log

- **2026-10-20T11:08:35Z** — SETUP
  - Created audit notes dir + progress log. Roadmap v1.2 and /home/m/Downloads/f1 guide read. Baseline gates: fmt=0, check=0, clippy=0, cargo test --workspace exit 0 (556 passed, 0 failed, 14 ignored). Starting F-01 trace.
- **2026-10-20T12:33:14Z** — F01-PHASE1
  - Read f01 guide (694 lines) + v1.2 F-01 section; captured to /tmp/f01/: enums/AckEnvelope/read_stream inbound path/sign+verify/boot key wiring/outbound sign/ACK sign/bind/validate/security/test index
- **2026-10-20T14:08:08Z** — F01-PATCH-REVIEW
  - Reviewing /home/m/Downloads/F-01-hmac-pairwise-edge-secrets.patch against HEAD 774818e. Patch fails to apply (context off-by-one at transport.rs:1265). Locating correct base commit.
- **2026-10-20T14:29:08Z** — F01-PATCH
  - Applied F-01 pairwise edge-secret corrections: transport.rs (edge-key sign/verify, IP-only source binding, optional local key), config validate_mesh + security.rs docs, boot.rs optional local key. Config mesh tests updated to new contract.
- **2026-10-20T14:37:31Z** — F01-DONE
  - F-01 pairwise edge HMAC secrets implemented+verified. Gates: fmt 0, check 0, clippy 0, workspace test 570 passed/0 failed/14 ignored, doc 0. Verdict A HMAC hardening=PASS. Verdict B peer identity isolation=PARTIAL (cross-edge provided, within-edge endpoint identity not provided - symmetric HMAC limit). Report: docs/development/audit-2026-10-20/F01_REPORT.md. Patch uncommitted; F-02..F-08 untouched.
- **2026-10-20T14:39:16Z** — F01-VERIFY
  - Second full workspace test run on the final tree (post doc/config edits): cargo test --workspace exit 0, 570 passed / 0 failed / 14 ignored, 45 suites ok, 0 panics, 0 cargo errors, doctests ran. First background run's exit-1 was a shell-quoting artifact in the reporting pipeline (i: command not found), NOT a test failure - both logs identical and clean. fmt/check/clippy/doc all exit 0.
- **2026-10-20T14:48:15Z** — F01-INDEP-CHECK
  - Independent pre-review verification of two subtle roadmap bullets: (1) one MAC type cannot authorize another - Envelope MAC input is 3-elem JSON array, AckEnvelope is 5-elem, different arity; WireMessage untagged variants cannot both parse from one JSON object, receiver re-serializes parsed body for verification. (2) ACK/recovery proof bound to expected peer+session+range - sessions keyed by sender_node_id bound to generation/recovery_id/boundary_lsn/digest/frame_count (transport.rs:424-431); complete_recovery re-checks all, requires all frames, recomputes+compares digest (503-526), idempotent on replay (511-513); proof inside ACK MAC and sender compares to expected (1140). Replay: no sequence field exists in protocol; adding one is a wire-format change barred by f1 4.2; within 30s window replay is inert (dot-idempotent CRDT, strictly-increasing source_lsn, session-bound recovery) - documented as residual risk 5, not silently changed.
