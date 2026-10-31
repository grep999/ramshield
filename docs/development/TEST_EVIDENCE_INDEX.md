# Core Functionality Test Evidence Index

This file maps every core RamShield functional area to its regression test(s).
Use this as evidence that code paths are exercised after every change.

| Functional Area | Test File(s) | What It Asserts |
|---|---|---|
| WAL persistence | `tests/k8s_configmap_validates::configmap_wal_dir_is_not_tmp` | ConfigMap WAL dir is `/var/lib/ramshield/wal`, NOT `/tmp/ramshield-wal` |
| WAL durability ordering | `wal_concurrency::wal_checkpoint_durability_ordering` | `finish_checkpoint()` calls `sync()` before `checkpoint()`; boundary LSN returned |
| WAL concurrent appends | `wal_concurrency::wal_concurrent_appends` | Multiple appends under concurrent access, no data loss |
| Recovery/restart | `recovery_restart` (29 tests) | Full cycle: write → close → reopen same dir → replay; equivalence of snapshot+tail |
| IPC auth (key-sorted MAC) | `ipc_wiring::verify_frame_auth_signs_the_key_sorted_payload_not_the_client_bytes` | Multi-key frame signed in client order → rejected; server re-serialises sorted map → accepted |
| IPC unauthorized mutation | `ipc_wiring::unauthorized_error_for_unauthenticated_mutation` | Unauthenticated `unblock_ip` → error (no hard-coded admin fallback) |
| tpacket alignment | `tpacket` (12 tests) | `checked_next_tpacket_offset()` rejects misaligned `tp_next_offset`; header/overflow/bound checks |
| Native capture worker lifecycle | `native_capture` (11 tests) | Worker spawn result known before daemon reports healthy; failures visible in metrics/dashboard |
| Native ingest drop counters | `native_capture` + `metrics::native_ingest_drops_total` | Channel-full drops counted via `inc_native_ingest_drop`; disconnects counted separately |
| XDP packet handling | `ramshield-xdp` (12+ tests) | TPACKET constants; block/ring boundary; mac snaplen; mixed malformed inputs never panic |
| Enforcement block/unblock | `enforcement` stress test + index-entry invariant | Blocked index entry updated under same lock as record (F9/F14 fix) |
| Capacity guard | `ramshield-types` + `ramshield-config` readme claims | Config schema validated against `Config::from_toml_file`; defaults pinned |
| SYNPROXY fixture parity | `nft_fixture_is_the_rendered_ruleset` | Checked fixture matches rendered ruleset from shipped baseline config — pins MSS, wscale, connection limits, rate limits |
| /0 prefix refused | `CGNAT_SHM_LOOKUP_FIXES` / crate-level test | Publisher refuses key 0; reader rejects as INVALID; /0 unsupported in SHM side channel |

**How to use**: After any code change, run the listed test(s). If they PASS, the core functionality
is preserved. If any FAIL, the change has broken an invariant — investigate immediately.