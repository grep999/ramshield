# RamShield 0.6.0 — Production Reference

## Product boundary

RamShield 0.6.0 is a self-hosted Linux ingress-defense daemon. Userspace
state and the WAL are authoritative; eBPF/XDP is a kernel projection. The
product is single-node by design today and is not a cloud scrubbing service,
upstream-capacity protection system, or multi-region distributed firewall.

A release is production-qualified only when both the repository gates and the
target-environment qualification gates are green.

## State and recovery

```text
telemetry -> detection -> enforcement -> WAL/userspace authority -> XDP projection

recovery = snapshot + verified WAL tail -> userspace IP/CIDR state -> XDP reconcile
```

A committed block does not imply that the XDP mutation succeeded. Active XDP
mutation failure marks projection health stale immediately; successful
reconciliation clears it.

## Detection contract

Supported mechanisms are absolute RPS with debounce, CUSUM sustained drift,
pulse-wave correlation, subnet gates, and bounded cardinality/cold-skip
mitigation. Relative low-and-slow detection is opt-in and experimental. It
uses the prior slow baseline, maturity gating, consecutive-breach hysteresis,
and freezes the reference baseline while a breach streak is active.

Forecasting provides EWMA, Holt-Winters, Bayesian hypothesis, entropy, and
CUSUM components. Configuration rejects non-finite and out-of-range smoothing
and threshold values. Forecasting is a signal, not a universal DDoS accuracy
claim.

## Enforcement and WAL

Enforcement follows: validate -> durability barrier -> WAL append -> userspace
mutation -> XDP projection -> immediate projection-health reporting. WAL
recovery validates record integrity and continuity, repairs only permitted final
tails, and reconstructs checkpoint plus WAL-tail state. New Unix WAL segments,
checkpoint manifests, and SHM files are created owner-only (`0600`).

## IPC and dashboard

IPC has authenticated roles, bounded frames/connections, replay protection,
and timeouts. The dashboard uses password hashing, session expiry, CSRF
protection, login throttling/lockout, and trusted-proxy handling. `/healthz`
is unauthenticated for orchestration; `/metrics` is Prometheus text.
Public control-plane exposure requires an explicit authentication and
TLS/mTLS boundary.

## XDP invariant

The reconciliation cursor tracks only retained/live keys. Stale keys are
deleted without assigning them to `prev_key`. Therefore the next kernel map
lookup always uses `NULL` or a key that still exists. Do not replace this with
collect-then-delete allocation: the retained-key cursor is the simpler correct
invariant.

Native XDP must remain native; the daemon must not silently claim native mode
after falling back to SKB. Target kernels, drivers, NICs, privileges, and BPF
filesystem behavior must be qualified separately.

## SHM invariant

The CGNAT SHM projection is a fixed-size seqlock table with a four-slot bounded
probe window. Rust writers and C readers use the same probe sequence. A full
probe window returns publication failure rather than silently evicting an
unrelated live rule. TTL addition saturates. Userspace enforcement remains the
authority when the projection is saturated.

## Kubernetes

The normal Deployment is one replica, loopback-only, XDP-disabled, and has no
Linux capabilities. The node-guard DaemonSet is the explicit XDP profile with
host networking, host BPF filesystem, limited capabilities, non-root execution,
and `/healthz` probes. There is no control-plane ClusterIP service.

## Release identity

For v0.6.0 these must agree: Cargo version, changelog section, Kubernetes image
tags, Git tag, archive names, installer names, and container image version.
The release workflow gates fmt, check, Clippy, workspace tests, cargo-audit,
cargo-deny, and publishes checksums, SPDX SBOM, signatures, provenance, config,
and license assets.

## Qualification

The exact release artifact must pass:

```text
fmt --check
check --locked --all-targets --features full
clippy --locked --all-targets --features full -- -D warnings
test --workspace --locked --features full
cargo audit --no-yanked
cargo deny check
installer checksum/signature verification
upgrade + rollback state preservation
kill -9 recovery
WAL corruption/repair boundaries
XDP attach/reconcile/failure recovery on target kernel/NIC
SHM collision/expiry tests
```

A failed qualification gate is a failed release.
