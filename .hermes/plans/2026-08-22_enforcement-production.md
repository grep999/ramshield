# RamShield Production Readiness — Enforcement First

## Research findings (verified, not assumed)

**Toolchain reality (this machine):**
- clang 18.1.3 present → compiles the C XDP program to valid eBPF ELF (verified: `file rs_xdp_test.o` = "ELF 64-bit LSB relocatable, eBPF")
- bpf-linker NOT installed; needs llvm-sys ^211/^221/^231 (LLVM 21/22/23); system ships LLVM 18 → cannot install without new toolchain
- Vendored aya **0.12.0** already in ~/.cargo registry, workspace `cargo check --workspace` clean
- aya latest = 0.14.0 (rust_version 1.87 — our nightly 1.100 satisfies), but upgrading adds dep churn for zero functional gain today
- Kernel 6.8, CONFIG_BPF_LSM=y, bounding set includes cap_bpf+cap_net_admin, uid 1000
- sudo requires password → live attach needs user cooperation (staged script ready)

**Decision: aya 0.12 (vendored) + clang-built C ELF.** Zero new deps, build.rs already produces real ELF via clang fallback. ponytail: upgrade to aya 0.14 when bpf-linker/LLVM≥21 available; API surface used (Bpf::load, Xdp::attach, HashMap insert/remove/get/keys) is stable across versions.

**Critical wiring bug found:** root crate has NO dependency on `crates/ramshield-xdp` — the release binary never contained XDP code at all. `StubXdpApplier` was hardcoded in `engine/mod.rs:182`.

**Dead config found during audit:** `StorageConfig` (all WAL fields) and `AlertingConfig` have zero runtime readers post-WAL/alerting deletion. Deleted alongside.

## Enforcement semantics (design contract)

1. **Sole writer** (existing, kept): all state transitions serialize through `EnforcementService`. Detection/forecasting/IPC enqueue only.
2. **Ordering**: storage-first, then index, then dataplane. Failed storage mutation ⇒ no phantom kernel block.
3. **Fail-open at kernel, fail-closed in state**: if `bpf_map_update_elem` fails, block stays in Store + `blocked_ips` (check_ip still answers "blocked"), `xdp_applied=false` recorded, warn logged. Kernel drop is an accelerator; in-band enforcement continues. Rationale: firewall availability > kernel purity; operator sees degraded mode via metrics/logs.
4. **Load failure at boot** (`[xdp] enabled=true` but attach fails): daemon continues with StubXdpApplier + loud error. Not a crash — a DDoS filter that refuses to start protects nothing.
5. **Attach mode**: SKB (generic) default — works on veth/docker/lo. DRV_MODE opt-in via config for production NICs.
6. **Reconcile on start**: drain map keys, delete keys not in expected set, insert missing. Idempotent under restart/crash.
7. **TTL invariant (bug fixed)**: at most one expiration entry per IP. Re-block purges prior entry; Unblock purges too. Previously: Block(ttl)→Unblock→Block left stale entry that spuriously unblocked the fresh block mid-attack.
8. **IPv6 limitation**: C program drops only IPv4-matched keys; v6 blocks enforced in-band only. ponytail: add ipv6 h_proto branch when v6 telemetry exists.

## Map key contract (byte-exact)

C: `key[0] = ip->saddr` (BE u32 in register), stored as LE u64 → bytes `[d,c,b,a,0×12]`.
Rust: `BlocklistKey(u128::from(u32::from(v4)))` — verified byte-identical. Value `u8=1` ↔ `__u8`.

## Implementation order

1. Config: delete StorageConfig/AlertingConfig; add `[xdp]` (enabled/interface/mode)
2. `src/enforcement/xdp.rs`: AyaXdpApplier (load/attach/reconcile/block/unblock/detach)
3. Engine: construct real applier when enabled, StubXdp otherwise
4. Enforcement: TTL purge fix
5. Tests: RecordingApplier unit tests + proptest sequence invariant
6. Gates → commit → staged live E2E (needs sudo)
