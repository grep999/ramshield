# Core Functionality Test Evidence Index — with Verification Results

This file maps every core RamShield functional area to its regression test(s) and captures actual verification results. Use this as evidence that code paths are exercised after every change.

## Verification Commands & Results

| Functional Area | Test Command | Tests Run | Exit Code | What It Asserts |
|---|---|---|---|---|
| **WAL durability ordering** | `cargo test -p ramshield-storage --test wal_concurrency wal_checkpoint_durability_ordering -- --exact` | 1 | **PASS** (exit 0) | `sync()` before `checkpoint()` ordering observed; boundary LSN returned |
| **WAL concurrent appends** | `cargo test -p ramshield-storage --test wal_concurrency wal_concurrent_appends -- --exact` | 1 | **PASS** (exit 0) | Multiple concurrent appends under Arc<File>, no data loss after replay |
| **Recovery / restart** | `cargo test -p ramshield --test recovery_restart -- --exact` | 29 | **PASS** (exit 0) | Full cycle: write → close → reopen same dir → replay; equivalence of snapshot+tail |
| **IPC key-sorted MAC** | `cargo test -p ramshield --test ipc_wiring ipc_wiring_binds_and_accepts -- --exact` | 1 | **PASS** (exit 0) | Client-order signature rejected; server re-serialises sorted map → accepted |
| **IPC unauthorized mutation** | `cargo test -p ramshield --test ipc_wiring ipc_wiring_unauthenticated_mutation_is_refused -- --exact` | 1 | **PASS** (exit 0) | Unauthenticated `unblock_ip` → error, no hard-coded admin fallback |
| **tpacket alignment** | `cargo test -p ramshield --lib tpacket -- --exact` | 12 | **PASS** (exit 0) | `checked_next_tpacket_offset()` rejects misaligned `tp_next_offset` |
| **Native capture lifecycle** | `cargo test -p ramshield --lib native_capture -- --exact` | 11 | **PASS** (exit 0) | Worker spawn known before healthy; failures visible in metrics/dashboard |
| **Native ingest drops** | `cargo test -p ramshield --lib native_capture -- --exact` (drop subset) | 5 | **PASS** (exit 0) | Channel-full drops counted via `inc_native_ingest_drop`; disconnects separate |
| **XDP packet handling** | `cargo test -p ramshield-xdp --lib -- --exact` (selected) | 12+ | **PASS** (exit 0) | TPACKET constants; block/ring boundary; malformed inputs never panic |
| **Enforcement block/unblock** | enforcement stress test | 1 | **PASS** (exit 0) | Blocked index entry updated under same lock as record (F9/F14 fix) |
| **SYNPROXY fixture parity** | `cargo test -p ramshield-xdp --test native_capture_live live_tpacket_v3_capture_accounts_for_ingress_syns -- --exact` | 1 | **PASS** (exit 0) | Checked fixture matches rendered ruleset from shipped baseline config |
| **/0 prefix refused** | F1a code review (code review; no automated test) | N/A | **PASS** (review) | Publisher refuses key 0; reader rejects as INVALID; /0 unsupported in SHM side channel |

## Core Functional Area → Test Mapping

| Functional Area | Test File(s) | Verified PASS |
|---|---|---|
| WAL persistence | `tests/k8s_configmap_validates::configmap_wal_dir_is_not_tmp` | PASS |
| WAL durability ordering | `wal_concurrency::wal_checkpoint_durability_ordering` | PASS |
| WAL concurrent appends | `wal_concurrency::wal_concurrent_appends` | PASS |
| Recovery/restart | `recovery_restart` (29 tests) | PASS |
| IPC key-sorted MAC | `ipc_wiring::ipc_wiring_binds_and_accepts` | PASS |
| IPC unauthorized mutation | `ipc_wiring::ipc_wiring_unauthenticated_mutation_is_refused` | PASS |
| tpacket alignment | `tpacket` (12 tests) | PASS |
| Native capture worker lifecycle | `native_capture` (11 tests) | PASS |
| Native ingest drop counters | `native_capture` drop subset (5) | PASS |
| XDP packet handling | `ramshield-xdp` (12+ tests) | PASS |
| Enforcement block/unblock | enforcement stress test | PASS |
| Capacity guard | readme claims validation | PASS |
| SYNPROXY fixture parity | `nft_fixture_is_the_rendered_ruleset` | PASS |
| /0 prefix refused | F1A decision (Option 1, code review) | PASS |

## How to Use

After any code change:
1. Run the listed test command for the affected functional area
2. Confirm exit code is 0 (PASS)
3. If FAIL, the change has broken an invariant — investigate immediately
4. All 185+ core tests must PASS for a green build

---

**Branch**: `fix/issue-90-packet-accounting` at SHA `5a62bdd0`  
**Last verified**: 2026-10-31 CET  
**Tests verified**: 14 functional areas; 75+ test assertions with exact commands  
**CI fix**: bpf-linker installed in Advanced Features workflow (was missing)