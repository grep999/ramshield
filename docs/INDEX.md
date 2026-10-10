# RamShield Documentation Index

This is the canonical documentation map for RamShield 0.6.0. The repository is a Linux-native, single-node ingress-defense system. The userspace state and WAL are authoritative; XDP is a kernel projection.

## Operators

1. [Quickstart](QUICKSTART.md) — first installation and smoke test.
2. [Operations](OPERATIONS.md) — routine production procedures.
3. [Configuration](CONFIGURATION.md) — configuration reference and environment overrides.
4. [Troubleshooting](TROUBLESHOOTING.md) — failure-oriented diagnosis.

## Engineers

- [Architecture](ARCHITECTURE.md) — runtime topology and data flow.
- [Code bible](CODE_BIBLE.md) — crate responsibilities, ownership, invariants, concurrency and safe-change rules.
- [IPC](IPC.md) — wire/authentication contract.
- [INVARIANTS](INVARIANTS.md) — security and correctness invariants.
- [Threat classes](THREAT_CLASSES.md) — detection claims and coverage.
- [Small-scale detection](SMALL_SCALE_DETECTION.md) — opt-in relative detector scope.
- [Development](DEVELOPMENT.md) — build/test workflow.

## Security and Release

- [Security model](SECURITY_MODEL.md) — trust boundaries, secrets, capabilities and failure policy.
- [Production qualification](PRODUCTION_QUALIFICATION.md) — qualification matrix and evidence requirements.
- [Release runbook](RELEASE_RUNBOOK.md) — exact release procedure.
- [Enterprise release](ENTERPRISE_RELEASE.md) — packaging/install contract.
- [Release contract](RELEASE_CONTRACT_1.0.md) — long-term product promises.
- [Production 0.6.0](PRODUCTION_0.4.0.md) — inherited production contract plus 0.6 security-gateway additions.

## Performance and Evidence

- [Benchmarks](BENCHMARKS.md) — benchmark methodology and results.
- [Qualification matrix](QUALIFICATION_MATRIX.md) — feature-by-feature evidence.

The documentation index follows the RFC 9411 perspective: A feature is only a supported product claim when its implementation, tests, qualification evidence and documentation agree. Experimental code must remain explicitly labelled experimental. Benchmark numbers are workload-specific measurements, not universal guarantees.

## Professional Compliance

All documented features undergo a structured validation:
1. **Implementation** - Code completeness and correctness verified
2. **Testing** - Comprehensive unit/integration tests with edge cases
3. **Qualification Evidence** - Production-grade qualification procedures and results
4. **Documentation** - Complete technical and operational documentation
5. **Review** - Security audit, code review, and quality gate validation

## Enforcement Boundary

**Authoritative state:** WAL + userspace state
**Projection state:** XDP + SHM kernel projections

Never infer kernel qualification from a userspace unit test.

- [0.5.0 L7 / mesh / upstream](FEATURE_0.5_L7_MESH_UPSTREAM.md) — integrated feature expansion.

- [Defense Model](DEFENSE_MODEL.md) — autonomous packet guard, L7 boundary, upstream escalation, management-plane and mesh model.

- [Security Gateway](SECURITY_GATEWAY.md) — autonomous observation, kernel defense, WAF boundary, and upstream BGP escalation.

- [Workspace scope (A-021)](WORKSPACE_SCOPE.md) — root product vs nested `rs/`
