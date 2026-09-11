RamForge — Adaptive brute-force login detection and blocking

Independent Rust binary that monitors external login attempts, aggregates failures per source IP, detects brute-force thresholds with AI-evasion-resistant adaptive behavioral analysis, and blocks attackers via nftables.

Features
--------
- Per-IP failure counting with sliding-window time span
- Escalating adaptive threshold: each block tightens the next threshold (max_failures - blocks), floored at adaptive_floor — defeats AI-paced low-volume attacks
- Cooldown semantics: on expiry → reported=false, failures=0; blocks/strikes persist
- NIST SP 800-63B tuned: 100-attempt research upper bound, per-source throttling
- JSON event ingest from stdin or TCP (syslog/journald)
- nftables enforcement: `nft add element inet bruteforce '{ <ip> timeout <ttl>s }'`
- Zero-alloc hot path: lock-free DashMap per-IP state
- Release binary: 1.5M, LTO + panic=abort
- 12/12 tests pass (8 unit + 4 integration)

Build
-----
cargo build --release --features full

Run
---
# stdin ingest (primary: pipe from journalctl/syslog)
cat /var/log/auth.log | ./target/release/ramforge

# TCP ingest
nc -l 514 | ./target/release/ramforge

# CLI options
./target/release/ramforge --help

Enforcement
-----------
Requires CAP_NET_ADMIN (run as root, or via systemd with CapabilityBoundingSet).
nftables set `bruteforce` created on first block; TTL expiry native in-kernel.
Never trust X-Forwarded-For; use allowlist to prevent self-lockout.

Research
--------
Threat landscape: 97% of identity attacks use password spray/brute force. AI models (PassLLM, PagPassGPT) can crack 50%+ of common passwords in seconds. Defenses: phishing-resistant passkeys (FIDO2/WebAuthn), number matching for push, adaptive rate limiting per NIST SP 800-63B-4 §5.2.2.

License
-------
MIT