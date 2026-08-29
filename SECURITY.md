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

The IPC protocol parser is exercised by `cargo-fuzz` harnesses under
`fuzz/`. The current harnesses:

- `fuzz_ipc_parser` — arbitrary bytes → `Request` parse → must never panic
- `fuzz_config`    — arbitrary TOML → `Config::load` → must never panic,
                     out-of-range values must surface as typed `ConfigError`

Run:

```bash
cargo install cargo-fuzz
cargo +nightly fuzz run fuzz_ipc_parser -- -max_total_time=300
```

Continuous fuzzing is **not** yet wired in CI. Tracked under
[issue #124](https://github.com/grep999/ramshield/issues) — add when budget
allows. Until then, fuzzing is run manually before each release.

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
