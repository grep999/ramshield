# Security Policy

## Reporting a Vulnerability

We take the security of RamShield seriously. If you have discovered a security vulnerability, **please do not open a public issue.**

Instead, please email **autodafeyolo@gmail.com** with details.

We will aim to acknowledge your report within 48 hours and provide an update on the investigation and remediation steps.

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.2.x   | ✅ Yes             |
| 0.1.x   | ❌ No (EOL)        |

The 0.1.x line is no longer maintained. Sites on 0.1.x should upgrade to
0.2.x — see [CHANGELOG](CHANGELOG.md) for the migration notes (breaking:
`deny_unknown_fields` on protocol requests, subnet batch now keyed on
distinct source IPs, CUSUM warm-up allowance).

## Fuzzing

The IPC protocol parser and the config loader are exercised by `proptest`
harnesses (`crates/ramshield-protocol/tests/fuzz.rs`,
`crates/ramshield-config/tests/fuzz.rs`). 2,048 cases per harness on every
`cargo test`. The harnesses assert:

- `serde_json` deserialization of `Request`/`Response` never panics on
  arbitrary bytes.
- A signature produced from a different key is always rejected.
- `Config::from_toml_file` never panics on arbitrary bytes, and
  `Config::validate` rejects `ram_limit_mb=0` and non-power-of-two
  `shard_count`.

Continuous fuzzing (oss-fuzz) is **not** yet wired in CI. Tracked in
ROADMAP.md 0.3 — added when budget allows. Until then, fuzzing runs in
`cargo test` on every push.

## Disclosure Policy

We follow a policy of responsible disclosure. We ask that you give us a reasonable amount of time to investigate and fix the vulnerability before publicly disclosing it. We appreciate your efforts to improve the security of our project.

## Threat Model & Assumptions

RamShield is designed to operate in a **trusted network zone** (localhost or isolated management network). The following are explicit non-goals:

| Threat | Status | Mitigation |
|--------|--------|------------|
| IPC eavesdropping | ❌ Not protected | Deploy on localhost or VPC |
| IPC spoofing | ❌ Not protected | Firewall `:7890` to trusted sources only |
| Unprivileged XDP attach | ❌ Requires CAP_SYS_ADMIN | Documented requirement |
| Kernel eBPF verifier bypass | ✅ Mitigated | Minimal eBPF surface; verifier enforced |
| Memory exhaustion | ✅ Mitigated | Hard RAM limit + promotion filter |
| Blocklist replay | ✅ Mitigated | UUID `decision_id` idempotency |
| IPC channel flood | ✅ Mitigated | 2M event capacity + 503 backpressure |

## Security Best Practices for Operators

1. **Bind IPC to localhost only** — `tcp_addr = "127.0.0.1:7890"`
2. **Firewall dashboard port** — `:9999` should not be public
3. **Run as non-root user** — XDP requires `CAP_SYS_ADMIN` capability only
4. **Use dedicated NIC for XDP** — isolate from management traffic
5. **Monitor RAM usage** — alert at 80% of `ram_limit_mb`
6. **Rotate logs** — structured JSON logs via `RUST_LOG`
