# Development

Build, test, and modify RamShield without needing to understand the whole repository first.

## Toolchain

Use the repository-pinned toolchain:

```bash
cat rust-toolchain.toml
```

The project currently builds with the pinned Rust nightly toolchain.

For the normal full binary:

```bash
cargo build --release --locked --features full
```

## Run locally

```bash
cp config.baseline.toml config.toml
cargo build --release --locked --features full
./target/release/ramshield --config config.toml --no-xdp
```

Use `--no-xdp` for a local run unless you are deliberately testing the kernel dataplane.

## Test

Run the workspace tests:

```bash
cargo test --workspace --locked --features full
```

Formatting:

```bash
cargo fmt --all -- --check
```

Linting:

```bash
cargo clippy --workspace --all-targets --features full -- -D warnings
```

Repository review gate, when making a broad change:

```bash
scripts/review_pipeline.sh
```

The repository also contains integration and smoke-test scripts under `scripts/`. Read the script's own header/comments before running a harness; many are deliberately environment-specific.

## Workspace layout

```text
src/
  main.rs
  cli.rs
  engine/
  ipc/
  dashboard/

crates/
  ramshield-config
  ramshield-detection
  ramshield-enforcement
  ramshield-forecasting
  ramshield-metrics
  ramshield-protocol
  ramshield-storage
  ramshield-types
  ramshield-xdp
  ramshield-cgnat
  ramshield-analytics
  ramshield-mesh

scripts/
tests/
benches/
docs/
deploy/
docker/
```

The top-level application wires the crates together. Domain logic belongs in the workspace crates.

## Changing configuration

When adding or changing a field:

1. update the schema/defaults in `ramshield-config`;
2. update `config.baseline.toml`;
3. update `docs/CONFIGURATION.md`;
4. add or update validation tests;
5. check runtime behavior if the field affects startup, XDP, WAL, IPC, or the dashboard.

Do not copy configuration defaults into multiple documents without updating the source definition.

## Changing the CLI

There are two binaries:

```text
ramshield
ramshield-cli
```

`ramshield` starts the daemon. `ramshield-cli` sends operational requests over IPC.

The daemon currently supports:

```text
--config / -c
--no-xdp
--version / -V
```

Unknown arguments are fatal.

The CLI currently provides:

```text
check
block
unblock
unblock-cidr
stats
status [--json]
info
```

When changing these, update `docs/QUICKSTART.md` and `docs/OPERATIONS.md`.

## SYNPROXY nftables integration test

The SYNPROXY rules touch host firewall state, so the optional kernel test runs
inside a fresh network namespace. Run it only on a Linux host with root
privileges, `unshare`, and `nft` installed:

```bash
sudo scripts/test_synproxy_netns.sh
```

The harness syntax-checks and loads the rules fixture in the isolated namespace,
then submits a deliberately failing multi-command nft transaction that deletes
and recreates the owned table. It verifies the transaction failure leaves the
original table and chain intact. It does not modify the host network namespace
or its firewall. This exercises nftables transaction behavior and fixture
compatibility; the normal Rust tests separately cover the daemon's fail-closed
preflight ordering.

## XDP development

XDP code lives in `ramshield-xdp` and the enforcement integration.

After rebuilding a binary used for XDP testing, re-apply file capabilities:

```bash
sudo setcap 'cap_net_admin,cap_perfmon,cap_bpf+eip' target/release/ramshield
```

Prefer isolated/netns testing for kernel-path changes where possible.

## Documentation rule

Keep documentation close to the code that owns the behavior:

```text
CLI behavior          → QUICKSTART / OPERATIONS
config schema         → CONFIGURATION
runtime/data flow     → ARCHITECTURE
failure/remediation   → TROUBLESHOOTING
build/test workflow   → DEVELOPMENT
security boundary     → SECURITY.md
release-facing change → CHANGELOG.md
```

Use plain English. State limitations directly. Do not document a feature merely because code for it exists.

## Release notes

The current package version is defined in `Cargo.toml`.

For a user-facing change:

1. update `CHANGELOG.md`;
2. verify the documented command/config behavior;
3. run the relevant tests;
4. record any compatibility or migration note.

Do not put roadmap material into `DEVELOPMENT.md`.

## Contribution principle

Prefer a small, verifiable change over broad cleanup.

A useful change should end with:

```text
implementation
→ test
→ documentation update
→ verification
```