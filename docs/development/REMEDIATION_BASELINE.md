REMEDIATION BASELINE
====================

Current HEAD: 159251975f09379c4a392d42a1c32717869f4e11 (master)
Date: 2026-10-30
Rust toolchain: Checking rust-toolchain.toml

Existing test commands:
- cargo test (full workspace)
- cargo test -p ramshield-storage
- cargo test -p ramshield-enforcement
- cargo test -p ramshield-mesh
- cargo test -p ramshield-xdp

CI Configuration: GitHub Actions workflows in .github/

Current Open Findings (from audit baseline):
- RS-001: WAL directory mismatch — config.baseline.toml dir="/tmp/ramshield-wal" but installer creates /var/lib/ramshield/wal; systemd PrivateTmp=true / ProtectSystem=strict prevents writing /tmp; data-loss risk on restart
- RS-003: Native capture discrepancy — >3000 SYN packets generated but only ~19 events reached application; zero reported socket drops suggests counter/accounting gap
- RS-006: Kubernetes Ansible bootstrap — role applies deployment but prerequisites (ConfigMap, ServiceAccount, PVC) may not be created in correct dependency order before kubectl apply
- RS-007: Checkpoint durability ordering — finish_checkpoint() already follows sync→checkpoint→retention; correct
- RS-008: Replay vs snapshot equivalence — existing test suite is comprehensive (checkpoint_equivalence_full_replay_vs_snapshot); coverage appears adequate
- RS-011: IPC mutation authorization — needs permanent regression gate
- RS-013: README performance claims — need reproducibility verification

Key Files Inspected:
- config.baseline.toml (wal dir = "/tmp/ramshield-wal")
- scripts/install.sh (creates /var/lib/ramshield/wal, writes config)
- scripts/install-user.sh (creates ~/.local/share/ramshield)
- deploy/systemd/ramshield.service (PrivateTmp=true, ReadWritePaths=/var/lib/ramshield)
- src/engine/native.rs (walk_block_packets, checked_next_tpacket_offset)
- ansible/roles/deploy/tasks/main.yml (cargo build → copy → systemd start, no explicit k8s prerequisites)
- ansible/roles/deploy/templates/k8s-deployment.yaml.j2 (references ConfigMap wal PVC)
- crates/ramshield-storage/src/wal.rs (finish_checkpoint ordering)
- tests/recovery_restart.rs (extensive full-snapshot equivalence tests)

Known Environmental Limitations:
- PrivateTmp=true on systemd makes /tmp per-service namespace, wiped on restart
- ProtectSystem=strict denies writes to /tmp unless in ReadWritePaths
- config.baseline.toml is THE canonical template — do not alter for production config