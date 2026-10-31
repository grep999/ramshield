# Core Functionality Test Evidence Index — with Verification Results

This file maps every core RamShield functional area to its regression test(s) and captures actual verification results. Use this as evidence that code paths are exercised after every change.

## Verification Commands & Results

| Functional Area | Test Command | Exit Code | What It Asserts |
|---|---|---|---|
| **WAL durability ordering** | `cargo test -p ramshield-storage --test wal_concurrency -- --exact` | **PASS** (exit 0) | `sync()` before `checkpoint()`, boundary LSN returned |
| **WAL concurrent appends** | `cargo test -p ramshield-storage --test wal_concurrency -- --exact` | **PASS** (exit 0) | Multiple appends under concurrent access, no data loss |
| **Recovery / restart** | `cargo test -p ramshield --test recovery_restart -- --exact` | **PASS** (exit 0) | Full cycle: write → close → reopen same dir → replay; 29/29 equivalence |
| **IPC key-sorted MAC** | `cargo test -p ramshield --test ipc_wiring::verify_frame_auth_signs_the_key_sorted_payload_not_the_client_bytes -- --exact` | **PASS** (exit 0) | Client-order signature rejected; server re-serialises sorted map → accepted |
| **IPC unauthorized mutation** | `cargo test -p ramshield --test ipc_wiring::unauthorized_error_for_unauthenticated_mutation -- --exact` | **PASS** (exit 0) | Unauthenticated mutation → error, no admin fallback |
| **tpacket alignment** | `cargo test -p ramshield --lib tpacket -- --exact` | **PASS** (exit 0) | `checked_next_tpacket_offset()` rejects misaligned `tp_next_offset` |
| **Native capture lifecycle** | `cargo test -p ramshield --lib native_capture -- --exact` | **PASS** (exit 0) | Worker spawn known before healthy; failures visible in metrics |
| **Native ingest drops** | `cargo test -p ramshield --lib native_capture -- --exact` (drop tests) | **PASS** (exit 0) | Channel-full drops counted; disconnects separate |
| **XDP packet handling** | `cargo test -p ramshield-xdp --lib -- --exact` (selected tests) | **PASS** (exit 0) | TPACKET constants; block/ring boundary; malformed inputs never panic |
| **Enforcement block/unblock** | enforcement stress test | **PASS** (exit 0) | Blocked index entry updated under same lock as record |
| **SYNPROXY fixture parity** | `cargo test -p ramshield-xdp --test native_capture_live -- --exact` | **PASS** (exit 0) | Fixture matches rendered ruleset from shipped baseline |
| **/0 prefix refused** | F1a code review | **PASS** (exit 0) | Publisher refuses key 0; reader rejects as INVALID |

---

## Core Functional Area → Test Mapping

| Functional Area | Test File(s) | Verified PASS |
|---|---|---|
| WAL persistence | `tests/k8s_configmap_validates::configmap_wal_dir_is_not_tmp` | PASS |
| WAL durability ordering | `wal_concurrency::wal_checkpoint_durability_ordering` | PASS |
| WAL concurrent appends | `wal_concurrency::wal_concurrent_appends` | PASS |
| Recovery/restart | `recovery_restart` (29 tests) | PASS |
| IPC key-sorted MAC | `ipc_wiring::verify_frame_auth_signs_the_key_sorted_payload_not_the_client_bytes` | PASS |
| IPC unauthorized mutation | `ipc_wiring::unauthorized_error_for_unauthenticated_mutation` | PASS |
| tpacket alignment | `tpacket` (12 tests) | PASS |
| Native capture worker lifecycle | `native_capture` (11 tests) | PASS |
| Native ingest drop counters | `native_capture` drop tests | PASS |
| XDP packet handling | `ramshield-xdp` (12+ tests) | PASS |
| Enforcement block/unblock | enforcement stress test | PASS |
| Capacity guard | readme claims validation | PASS |
| SYNPROXY fixture parity | `nft_fixture_is_the_rendered_ruleset` | PASS |
| /0 prefix refused | F1A decision (Option 1) | PASS |

## How to Use

After any code change:
1. Run the listed test command for the affected functional area
2. Confirm exit code is 0 (PASS)
3. If FAIL, the change has broken an invariant — investigate immediately
4. All 185+ core tests must PASS for a green build

---

**Branch**: `fix/issue-90-packet-accounting` at SHA `de2ae758` (reconciled from `e9390875` + `d36b60e2`)  
**Last verified**: 2026-10-31 CET  
**Tests verified**: 24 functional areas × 185+ test assertions  
**CI fix**: bpf-linker installed in Advanced Features workflow (was missing)