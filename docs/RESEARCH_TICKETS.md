# Research Tickets (Pulse-friendly)

Format:
- [ticket-id] short title — informs task_id — 1-line action

None yet. Created by ramshield-research cron run.

- [R-T01] Append-only SHA-256 chained audit log — informs roadmap/Q3-AuditLog-SHA256 — add `src/audit.rs` with `AppendOnlyLog::append` + `verify_chain` using `sha2` + `ed25519-dalek`; reuse key for tokio-rustls node identity.
- [R-T02] IoT Gateway Shielding — informs roadmap/Q5-IoT-Gateway — add `embassy` + `riscv` + `cortex-m` to `Cargo.toml` with `no_std` feature; cross-compile with `cargo-embed` for ARM Cortex-M / RISC-V targets; size-optimize with `panic=abort` + `lto=fat`.
- [R-T03] Kubernetes Operator for auto-scaling — informs roadmap/Q5-Kubernetes-Operator — add `kube-rs` + `kube-runtime` to `Cargo.toml`; implement `Reconciler` for `RamShieldDeployment` CRD; watch Pod metrics via metrics-server; scale via Scale subresource; bundle CRD manifests with helm/kustomize.
