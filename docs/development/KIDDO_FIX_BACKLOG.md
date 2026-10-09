# kiddo_fix: focused remediation backlog

Tracking issue: https://github.com/grep999/ramshield/issues/4

## Rules
- Branch: `kiddo_fix`; base/target: `master`.
- Re-validate every finding against the live branch before patching; archive-derived patches may be stale.
- Keep each fix minimal and separately reviewable.
- Include regression coverage and exact validation commands/results.
- The reviewer works independently and inspects the actual diff and test evidence.
- Do not merge automatically. Require green CI and independent approval.
- Never report tests as passing unless they were actually executed and the output was inspected.

## Priority order to re-validate
1. Shared-memory ABI compatibility, initialization/readiness, and writer synchronization across Rust/C consumers.
2. Replay-store saturation must preserve live replay markers and fail closed.
3. Mesh frame-size limits before allocation; bounded inbound/outbound concurrency and I/O timeouts.
4. TPACKET_V3 descriptor bounds, worker readiness/error propagation, and channel-drop observability.
5. Coherent concurrent statistics and CMS decay without lost updates.
6. SYNPROXY/nftables transaction validation and reversible sysctl changes.
7. CI gates: pinned toolchain, format/check/clippy/test, dependency audit, no swallowed failures.

## Per-fix checklist
- [ ] Reproduce or write a focused regression test.
- [ ] Make the smallest safe patch on `kiddo_fix`.
- [ ] Run available targeted tests and required CI gates; record unavailable gates honestly.
- [ ] Independent reviewer checks correctness, regressions, and test evidence.
- [ ] Open a PR to `master`; no automatic merge.
