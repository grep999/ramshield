# Changelog

## Unreleased — Maturity hardening
- Fix synproxy nftables syntax error (meta nfproto ipv4/ipv6), restore XDP stubs, ensure E0277 guard dropped before await, and adjust config defaults for dev boot (wal.dir, enable inband fallback).

### Security
- SEC-06: Reject `prefix_len > 32` in `ramshield_ipv4_subnet_key` (nested header) before the mask shift — a shift width ≥ 32 on a 32-bit value is undefined behavior under C99 §6.5.7.
- SEC-10: HTML-escape dashboard template substitutions via `sanitize_html` so reflected error text cannot inject markup.
- SEC-14: `Lsn::next` returns `Result<Lsn, Error::LsnExhausted>` instead of panicking at `u64::MAX`, so sequence exhaustion fails closed instead of aborting the storage engine.
- SEC-16: ExaBGP FIFO bridge opens the FIFO `O_RDWR`, so a writer restart no longer delivers EOF and kills the bridge process.
- SEC-17: `[profile.release]` hardening — `lto`, `codegen-units = 1`, `panic = "abort"`, `strip`, `overflow-checks`.

- Restore the XDP packet-boundary helpers, counters, and drop-event emitter required by the Aya program.
- Make VXLAN/Geneve raw-pointer access verifier-safe.
- Replace private AF_PACKET fanout groups with a shared `PACKET_FANOUT_HASH` group and remove a per-packet mutex from the native event budget.
- Extend native overlay observation to inner IPv6 and prevent trusted outer LB/node addresses from becoming enforcement identities.
- Scope SSRF inspection away from the HTTP `Host` header and add a localhost health-check regression test.
- Make SYNPROXY ruleset generation testable, rate-limit SYNs before NOTRACK, preserve established flows under connection ceilings, and cleanly tear down on partial startup failure.
- Restore Kubernetes separation between the sandboxed server Deployment and the host-network node-guard DaemonSet; disable SYNPROXY in the distroless node profile unless a host-netfilter helper is supplied.
- Make the tracked baseline IPC configuration authenticated and have the installer replace its development credential with a unique 256-bit key.
- Make the default Docker build a sandboxed profile with host-network defenses disabled unless explicitly requested.

## 0.6.0 — Security gateway closure

- Add native Linux AF_PACKET L3/L4 observation so proxy IPC is no longer the sole telemetry source.
- Add bounded per-source SYN/UDP and aggregate packet controls in XDP; fix the previous double-counting path.
- Add Linux kernel SYNPROXY integration for selected TCP ports.
- Add a bounded deterministic HTTP/1.0/1.1 parser and WAF checks for SQLi, XSS, path traversal, command injection, SSRF, malformed and oversized requests.
- Add non-blocking ExaBGP FlowSpec and RTBH command emission through a restrictive FIFO, including recovery withdrawal.
- Require IPC HMAC credentials in every validated configuration, including loopback.
- Add deployment capabilities/documentation for native host observation and keep the distroless Kubernetes profile from silently attempting unavailable host netfilter tooling.
- Preserve the first-principles enforcement boundary: no community reputation feed becomes an enforcement authority.

## 0.5.0 — First-party threat intelligence boundary

- Enforcement decisions now carry explicit evidence provenance: local signals, trusted fleet signals, or operator action.
- Community/public reputation feeds are intentionally not representable as an authoritative enforcement source.
- Mesh-originated mitigations are marked as trusted fleet evidence.
- Manual IPC mitigations are marked as operator evidence.
- This keeps RamShield first-principles: reputation is never required for detection or blocking.
- Add an optional autonomous XDP packet guard for cold-start SYN, UDP and aggregate packet floods.
- Preserve bounded L7 metadata through the IPC ingress path; reduce decoded IPC batch size to 8,192 events.
- Increase the CGNAT shared-memory table to 256K slots with an 8-probe window to reduce churn-driven saturation.


Notable user-facing changes are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/) and releases use Semantic Versioning.

## [0.5.0] - 2026-10-06

### Feature release — integrated expansion
- Extend the existing `ConnectionEvent` telemetry contract with bounded L7 and HTTP/2 metadata while preserving legacy wire compatibility.
- Add route/method-aware L7 thresholds and HTTP/2 stream-reset detection through the existing detection and EnforcementService path.
- Activate the existing AWORSet mesh foundation with authenticated TCP delta transport, bounded anti-entropy and safe remote unblock semantics.
- Add uplink saturation detection and a vendor-neutral upstream mitigation webhook without changing the local XDP dataplane.
- Keep XDP Linux-specific while allowing the core telemetry/protocol/userspace path to participate on non-Linux systems.

### Verification contract
- Existing WAL → Store → XDP ordering remains authoritative for every local and remote block.
- Mesh traffic is HMAC authenticated and timestamp bounded.
- L7 state is bounded per IP and route; raw URL/header/body data is not retained.
- Upstream escalation is advisory/adapter-driven and cannot itself mutate local enforcement state.

## [0.4.0] - 2026-10-05

### Release hardening
- Close the v0.4.0 release spine: version identity, signed archives, SPDX SBOM, provenance attestation, dependency audit gates, upgrade/rollback qualification, and pinned Kubernetes image references.
- Surface XDP projection mutation failures immediately as stale protection health while preserving the existing reconciliation recovery path.
- Harden the experimental relative detector with finite configuration validation, prior-baseline evaluation, maturity gating, consecutive-breach hysteresis, and a frozen reference baseline during an active breach streak.
- Harden the shared-memory projection with bounded collision probing, explicit owner-only file creation, TTL saturation, and explicit saturation reporting.

### Verification contract
- The v0.4.0 tag must point at the same commit as the release candidate branch.
- Release qualification requires fmt, check, Clippy, workspace tests, dependency audit, cargo-deny, release identity, artifact verification, and platform-specific XDP qualification where XDP is enabled.

## [0.3.4] - 2026-10-05

### Enterprise release closure
- Add a single release identity, pinned signed installer, SPDX SBOM, provenance attestation, and hard dependency-audit gating.
- Make upgrade and rollback qualification fail closed on enforcement-state loss.
- Pin production Kubernetes images to the release version and document the enterprise qualification contract.

### Production hardening closure
- Seed CIDR checkpoint recovery from the snapshot and replay the WAL tail symmetrically with IP state, including tail `UnblockCidr` equivalence.
- Make stale XDP reconciliation a protection-health failure/degradation signal rather than treating attachment alone as healthy.
- Honor `xdp.mode = native` as native/driver XDP instead of silently downgrading to SKB.
- Refuse readiness when required detection workers cannot start; bound worker fan-out and IPC connections.
- Reject duplicate IPC key IDs and public plaintext dashboard binds.
- Make checkpoint snapshots private (`0600`) and align active release artifacts on the 0.3.4 version.
- Separate Kubernetes server and node-guard selectors/configuration; keep control-plane sockets loopback-only.

### Detection (opt-in)
- Relative / small-scale gate config + decision path (`relative_enabled`, default **false**): promoted IPs may block when `inst_rps >= max(floor, factor × baseline)` after `relative_min_samples`. No per-IP side map on the hot path.

### Added
- `docs/QUALIFICATION_MATRIX.md`: 17-dimension qualification matrix mapping each audit row to a concrete test artifact and live status.
- `scripts/qualification_check.sh`: automated per-row verifier (`--quick` / `--json`, exit code = fail count).
- `disk_full_append_maps_to_diskfull_error` in `crates/ramwal/`: ENOSPC fault-injected via `fail_next_diskfull()` test seam; proves `StorageFull → Error::DiskFull` mapping, poison transition, and reopen recovery.
- `docs/INVARIANTS.md`: WAL/Store/Checkpoint invariant contracts, cross-referenced from ARCHITECTURE.md.
- `wal_lsn_invariants` test: written ≥ durable, checkpoint ≤ durable assertion after every append/sync/checkpoint transition.
- Ansible `qualification` role: runs matrix checker, advisory by default, wired into both playbooks.

## [0.3.2] - 2026-09-29

### Fixed
- Checkpoint correctness: IP/CIDR absolute-deadline snapshots, barrier-guarded
  WAL→Store mutation, CIDR state in snapshot + tail replay, seed-fold
  algorithm (snapshot IPs untouched by tail stay blocked; tail UnblockIp
  removes them).
- WAL flush: checkpoint tests now drop/reopen WAL before replay to ensure
  disk-consistent reads under Durability::None.
- CIDR-only restoration: checkpoint snapshot + tail replay now restores CIDR
  state even when no IP blocks exist.
- Hard-WAL fail-closed: snapshot exists but WAL history pruned → startup
  failure (not silent partial state).
- Storage concurrency: blocked_set/blocked_count updates moved inside DashMap
  shard lock (no race window between unlock and index update).
- Startup lifecycle: main awaits engine boot pipeline readiness via oneshot
  channel; startup result is delivered at pipeline-ready (IPC/detection/enforcement
  up), not at daemon shutdown. Half-alive daemon eliminated.
- Config hardening: worker_threads validated (0 = auto/available_parallelism(),
  rejected above 4096). XDP mode validated against exhaustive set (skb/drv/native).
- Removed dead code: `apply_growth` (superseded by lock-safe `apply_growth_size_only`).

### Tests
- `tests/recovery_restart.rs`: 15 checkpoint-path tests (permanent/temp/expired
  IP + CIDR, mixed snapshot+tail, corrupt/missing snapshot, hard-WAL fail-closed,
  checkpoint barrier concurrency x8 iterations).
- `assert_store_invariants`: cfg(test) invariant checker verifying
  blocked_set/blocked_count/ttl_entries vs authoritative state.
- `stress_block_unblock_concurrent`: 8 threads × 500 random block/unblock/remove cycles.
- Invariant checker called from 5 existing tests.

### Release
- Release identity normalized: all k8s manifests, container examples, and
  documentation reference 0.3.2.
- Tag v0.3.2 with checkpoint correctness closure.

## [0.3.1] - 2026-09-28

### Fixed
- XDP failures no longer report healthy kernel protection. `allow_inband_fallback` (default `false`) controls whether XDP attach failure blocks startup vs degrades gracefully.
- `/healthz` and `/api/snapshot` expose `protection_state` (starting/protected/degraded/failed/stopping) and `xdp_configured`.
- WAL open/replay/CIDR-replay failures no longer silently degrade to volatile enforcement (`allow_volatile_fallback` default false).
- XDP builds fail when the BPF artifact cannot be produced or validated (no placeholder ELF).
- Documented XDP LRU eviction: userspace reconcile restores evicted rules (`ramshield_xdp_reconcile_successes_total`).
- Replay cache bounded per authentication key (`per_key_cap`) preventing one-key exhaustion of global cache.
- Config validation enforces minimum bounds for `max_line_length` (≥256), `max_password_length` (≥1), `max_login_attempts` (≥1), WAL retention minimums.

### Security
- Explicitly fail-closed XDP startup when `[xdp].enabled = true` and kernel dataplane cannot be attached.
- Argon2 password verification concurrency bounded via `[auth].argon2_parallelism` (default 4, Semaphore-gated).
- `/metrics` endpoint exempted from dashboard auth (Prometheus counters only, no sensitive data).
- K8s DaemonSet: `argon2-hash` secret required (`optional: false`). NetworkPolicy selectors tightened to specific pods/namespaces.

### Qualification
- `docs/QUALIFICATION_0.3.1.md`: enforcement, persistence, failure semantics, deployment sections.
- `scripts/upgrade_qualification.sh`: 0.3.0→0.3.1 upgrade and rollback test.
- `scripts/review_pipeline.sh`: added config-contract and release-metadata gates.
- `scripts/prod_smoke.sh`: documented full coverage matrix (review 31 Batch 16).
- `scripts/xdp_qual_matrix.sh`: static ELF + contract matrix; `--live` attach/detach is host-gated.
- `scripts/cap_lifecycle_check.sh`, `scripts/run_benchmarks.sh`, `scripts/pcap_replay.sh`, `scripts/verify_release.sh`.
- WAL fuzz: `crates/ramshield-storage/tests/fuzz.rs` (arbitrary segment bytes must not panic).

### Release hygiene
- `release_candidate.sh`: removed hash of missing keystore files; release gate works from clean checkout.
- Removed absolute symlink, `__pycache__` dirs, `.pyc` files from tracked source.
- `scripts/install.sh`: fixed stale `ramshield/beta/rs` path.
- Baseline recorded at `docs/qualification/0.3.1-baseline.txt`.

## [0.3.0] - 2026-09-26

### Added
- Authenticated IPC with HMAC-SHA256 frame auth, key identity, role-based authorization (Telemetry < ReadOnly < Operator < Admin), and replay protection.
- `--no-xdp` daemon flag for local/CI runs without XDP.
- Argon2-protected dashboard sessions and Prometheus `/metrics` endpoint.
- Crash-durable WAL: configurable retention, compression, fsync; replay restores live blocks before XDP reconcile; TTL-expired blocks not resurrected.
- Configurable dashboard block history size (was hardcoded 40 → `[dashboard] block_log_size`).
- XDP enforcement (AyaAyaXdpApplier wired into boot), drop attribution per-IP audit counts, zero-drop gauge.
- Emergency fast-path detection: burst-block IPs before flush.
- Bloom filter saturation self-heal for detection pipeline.
- SPOT-lite empirical extreme-quantile alarm (forecasting P2).
- Bayesian Hypothesis Framework — unified anomaly detector (EWMA variance + CUSUM).
- v6 /64 swarm gate via subnet_index cardinality; family-complete CIDR records.
- SIGINT/SIGTERM/SIGHUP trap for graceful XDP-unbind shutdown.
- Systemd unit and K8s DaemonSet deployment manifests.
- Hot-path benchmarks (subnet_rotation mode: 30 unique /24s per 15s).
- Production smoke test script.
- `ramshield doctor` subcommand (kernel/config/WAL/XDP/capability checks).
- WAL retention_max_bytes: oldest segments pruned on open/rotate.
- Total RAM in dashboard snapshot (real RSS, not harcoded 32GB ref).

### Changed
- Subnet blocking uses distinct source IPs as one gate (reduces whole-subnet reactions to a single burst) with shorter TTL than per-IP blocks.
- Protocol requests reject unknown fields instead of silent acceptance.
- Detection-first prod config: 25ms decision quantum, 250ms staleness backstop.
- Dashboard redesigned: KPI cards, pipeline flow strip, system gauges, SSE live stream, tabs, sticky header.
- Dead legacy modules/codecs removed (2.1K LOC).
- `src/` restructured: unified Store, DetectionEngine, enforcement actor with WAL-first ordering.
- Config: apply_env_overrides extracted; no-config mode honors env.
- CLI: positional config path honored, unknown flags fatal.

### Fixed
- Lock-poisoning hardened across storage paths.
- Oversize IPC connections receive typed error before close.
- IPC auth silent downgrade: reject keyless keys and answer the client.
- XDP surface apply failures: CIDR LPM trie full stops subnet mitigation (was silent).
- Detection export dropped enforcement-block telemetry.
- Storage: Store::insert is shard-locked commit; preserve subnet window baseline on clock rollback.
- CGNAT seqlock fences — payload cannot overtake odd marker.
- Check_ip consults active CIDR blocks, not just per-IP state.
- One subnet decision, one owner (no split-brain detection).
- WAL replay idempotency qualification (replay twice → same state).
- XDP reconcile qualification against userspace truth.
- Enforcement queue full → 503 response.
- Bloom caches promoted IPs, becomes observable.
- Config: invalid env override values fail startup loudly.
- Dashboard panels stay live; SSE stream recovery.
- Metrics: events_rejected_total split into clean per-class counters.
- Unwrap/expect eliminated from production modules (CI no-unwrap gate enforces).

### Performance
- Detection: multi-threaded batch processing via crossbeam channel; bloom insert via shared atomic words (kills per-flush clone); hoist dual-gate read per subnet per flush; defer bloom probe to genuinely cold IPs only; single-lookup emergency crossing check.
- Storage: subnet_index inner sets as plain HashSet (not sharded DashMap).
- IPC: BytesMut split_to replaces Vec::drain — O(1) framing, eliminates quadratic pipelining.
- Enforcement: expirations Vec→HashMap — O(1) TTL dedup replaces O(N) retain sweeps.
- WAL: retention scan only on segment rotation; 100ms fsync cap prevents thundering herd.
- Forecasting: lock-free — remove Mutex from TrafficCounters.
- Engine: multi_thread fixes IPC starvation under attack load (7× 5s timeouts→0, 75k→489k events/phase); event channel 2M→256k saves 200MB RSS.

### Documentation
- Restructured to 6-core docs set: QUICKSTART, OPERATIONS, CONFIGURATION, ARCHITECTURE, DEVELOPMENT; all content folded in from old PRODUCT/FEATURES/PRODUCTION_READINESS/ROADMAP/CAPACITY/DEPLOY/TUNING/UPGRADING files.
- README rewritten as compact GitHub landing page.
- AGENTS.md merged into DEVELOPMENT.md as coding standards.

### Security
- Key-bearing configs no longer tracked; IPC auth key rotated.
- CIDR prefix validation on deserialization.
- metrics/forecasting/config single copies eliminates stale crate divergence.

## [0.2.0] - 2026-07-31

### Added

- Initial project release notes and contributor guidance.

### Changed

- Build and verification guidance was tightened.

### Fixed

- Removed dead imports and unused constants that blocked verification.

[0.3.0]: https://github.com/grep999/ramshield/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/grep999/ramshield/releases/tag/v0.2.0