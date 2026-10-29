# Testing Documentation

Run the gates; do not trust dated tables. Every claim in this repo is backed by a
command you can re-run against the current checkout.

## Gates

```bash
cargo fmt --all -- --check
cargo clippy --workspace --locked --all-targets --features full -- -D warnings
cargo test --workspace --locked --features full
cargo audit --no-yanked
cargo deny check
bash -n scripts/*.sh
python3 -m compileall -q scripts
scripts/verify_release.sh --static
```

## Targeted suites

```bash
cargo test -p ramshield-protocol    # ReplayStore, auth
cargo test -p ramshield-detection    # forecasting, mesh, detection
cargo test -p ramshield-enforcement  # enforcement service, XDP
cargo test -p ramshield-storage      # WAL, recovery
cargo test --all-features --lib native  # TPACKET_V3 walker
```

## Evidence rule

Record results as `command`, exact `git rev-parse HEAD`, and observed output.
A pass count without a SHA is a story, not evidence. Historical audit numbers
belong in `AUDIT_REPORT.md`, not here.

## Test maintenance

- No `.unwrap()` in production paths — use `?` with context.
- Gate forecast sampling on `forecasting_worker_enabled` at engine startup.
- Release profile uses `panic = "abort"`.
- XDP map-update failure fails open; documented in `AGENTS.md`.
