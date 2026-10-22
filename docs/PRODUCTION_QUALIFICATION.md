# Production Qualification

Required on the pinned Rust toolchain and a dedicated Linux/XDP host:

```bash
cargo fmt --all -- --check
cargo check --workspace --locked --all-targets --features full
cargo clippy --workspace --locked --all-targets --features full -- -D warnings
cargo test --workspace --locked --features full
cargo audit --no-yanked
scripts/upgrade_qualification.sh
scripts/xdp_qual_matrix.sh --live
scripts/prod_smoke.sh
```

A release is NO-GO if persistent enforcement state is lost, unauthorized administration succeeds, XDP is stale while reported healthy, required workers are absent while ready, WAL corruption silently changes state, or resource behavior exceeds the published qualification envelope.

Benchmark results become product evidence only when recorded with release commit, host, kernel, NIC, configuration, workload, and result artifact.
